// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! The pool and the baseline migration against a real PostgreSQL server.
//!
//! Every test needs a server and is ignored in a normal test run. Run them with
//!
//! ```text
//! IMPLUVIUM_TEST_DATABASE_URL=postgres://USER:PASSWORD@HOST:5432/postgres \
//!   cargo test -p server-db --test server_db_postgres -- --ignored --test-threads=1
//! ```
//!
//! The user must be allowed to create and drop databases, and the server must
//! require password authentication. Each test creates its own database and
//! drops it at the end, also when the test fails.

use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use server_db::{
    ConnectFailure, Database, DatabaseUrl, DbError, InitialisationState, MIGRATOR, MigrateFailure,
    PoolOptions,
};
use sqlx::{AssertSqlSafe, Connection, PgConnection};

const URL_VARIABLE: &str = "IMPLUVIUM_TEST_DATABASE_URL";

/// The URL of the maintenance database. A missing variable fails the test:
/// an explicitly requested database test must never pass without a database.
fn admin_url() -> String {
    match std::env::var("IMPLUVIUM_TEST_DATABASE_URL") {
        Ok(url) if !url.is_empty() => url,
        _ => panic!(
            "{URL_VARIABLE} is not set: these tests need a PostgreSQL server \
             and must not pass without one"
        ),
    }
}

/// Replaces the database name at the end of a URL.
fn with_database(url: &str, name: &str) -> String {
    let (base, query) = match url.split_once('?') {
        Some((base, query)) => (base, Some(query)),
        None => (url, None),
    };
    let (head, _) = base
        .rsplit_once('/')
        .expect("the URL ends in a database name");
    assert!(
        head.contains("://") && !head.ends_with("//"),
        "the URL ends in a database name"
    );
    match query {
        Some(query) => format!("{head}/{name}?{query}"),
        None => format!("{head}/{name}"),
    }
}

/// Splits `scheme://user:password@host:port/...` into user and host part.
fn user_and_host(url: &str) -> (String, String) {
    let (scheme, rest) = url.split_once("://").expect("a URL with a scheme");
    let authority = rest.split(['/', '?']).next().expect("an authority");
    let (userinfo, host) = authority.rsplit_once('@').expect("a URL with credentials");
    let user = userinfo.split(':').next().expect("a user name");
    (format!("{scheme}://{user}"), host.to_owned())
}

fn options() -> PoolOptions {
    PoolOptions {
        max_connections: 2,
        ..PoolOptions::default()
    }
}

/// Drops the test database when it goes out of scope, also during a panic.
struct DropDatabase {
    admin_url: String,
    name: String,
}

impl Drop for DropDatabase {
    fn drop(&mut self) {
        let admin_url = self.admin_url.clone();
        let name = self.name.clone();
        // A destructor cannot await, so a short-lived runtime on its own thread does it.
        let _ = std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("a runtime to drop the test database");
            // A server that stops answering must not hang the test run.
            let cleanup = async {
                if let Ok(mut connection) = PgConnection::connect(&admin_url).await {
                    let statement = format!("DROP DATABASE IF EXISTS \"{name}\" WITH (FORCE)");
                    let _ = sqlx::query(AssertSqlSafe(statement))
                        .execute(&mut connection)
                        .await;
                }
            };
            let _ = runtime.block_on(tokio::time::timeout(Duration::from_secs(10), cleanup));
        })
        .join();
    }
}

/// A fresh, empty database with a connected pool.
struct TestDb {
    db: Database,
    _drop: DropDatabase,
}

static COUNTER: AtomicU32 = AtomicU32::new(0);

impl TestDb {
    async fn new() -> Self {
        let admin_url = admin_url();
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("a clock after 1970")
            .as_nanos();
        let name = format!(
            "impluvium_test_{}_{nanos:x}_{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::SeqCst)
        );
        let mut connection = PgConnection::connect(&admin_url)
            .await
            .expect("the maintenance database accepts the test connection");
        sqlx::query(AssertSqlSafe(format!("CREATE DATABASE \"{name}\"")))
            .execute(&mut connection)
            .await
            .expect("the test user may create databases");
        let guard = DropDatabase {
            admin_url: admin_url.clone(),
            name: name.clone(),
        };
        let url = DatabaseUrl::new(with_database(&admin_url, &name));
        let db = Database::connect(&url, &options())
            .await
            .expect("the pool connects to the new database");
        Self { db, _drop: guard }
    }

    async fn migrated() -> Self {
        let test = Self::new().await;
        test.db
            .migrate()
            .await
            .expect("the baseline migration applies");
        test
    }

    async fn text_column(&self, sql: &'static str) -> Vec<String> {
        sqlx::query_scalar::<_, String>(sql)
            .fetch_all(self.db.pool())
            .await
            .expect("the catalog query runs")
    }

    async fn number(&self, sql: &'static str) -> i64 {
        sqlx::query_scalar::<_, i64>(sql)
            .fetch_one(self.db.pool())
            .await
            .expect("the count query runs")
    }

    async fn run(&self, sql: &str) {
        // Test statements are written in this file; none carries outside input.
        sqlx::query(AssertSqlSafe(sql.to_owned()))
            .execute(self.db.pool())
            .await
            .unwrap_or_else(|error| panic!("statement failed: {sql}: {error}"));
    }

    /// Runs a statement that must fail and returns its SQLSTATE.
    async fn rejected(&self, sql: &str) -> String {
        match sqlx::query(AssertSqlSafe(sql.to_owned()))
            .execute(self.db.pool())
            .await
        {
            Ok(_) => panic!("statement was accepted: {sql}"),
            Err(sqlx::Error::Database(error)) => error
                .code()
                .map_or_else(String::new, |code| code.into_owned()),
            Err(other) => panic!("unexpected failure of {sql}: {other:?}"),
        }
    }
}

const UNIQUE_VIOLATION: &str = "23505";
const CHECK_VIOLATION: &str = "23514";

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires PostgreSQL (IMPLUVIUM_TEST_DATABASE_URL)"]
async fn a_fresh_database_migrates() {
    let test = TestDb::new().await;
    test.db
        .migrate()
        .await
        .expect("the baseline migration applies");
    let applied = sqlx::query_as::<_, (i64, bool)>(
        "SELECT version, success FROM _sqlx_migrations ORDER BY version",
    )
    .fetch_all(test.db.pool())
    .await
    .expect("the history is readable");
    assert_eq!(applied, vec![(1, true)]);
    assert_eq!(MIGRATOR.migrations.len(), 1);
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires PostgreSQL (IMPLUVIUM_TEST_DATABASE_URL)"]
async fn migrating_twice_changes_nothing() {
    let test = TestDb::migrated().await;
    test.run("INSERT INTO instance_identity (layout_version) VALUES (1)")
        .await;
    test.db.migrate().await.expect("the second run is accepted");
    assert_eq!(
        test.number("SELECT count(*) FROM _sqlx_migrations").await,
        1
    );
    assert_eq!(
        test.number("SELECT count(*) FROM instance_identity").await,
        1
    );
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires PostgreSQL (IMPLUVIUM_TEST_DATABASE_URL)"]
async fn the_schema_has_exactly_the_expected_tables() {
    let test = TestDb::migrated().await;
    let tables = test
        .text_column(
            "SELECT table_name::text FROM information_schema.tables \
             WHERE table_schema = 'public' AND table_type = 'BASE TABLE' ORDER BY 1",
        )
        .await;
    assert_eq!(
        tables,
        [
            "_sqlx_migrations",
            "content_key_binding",
            "instance_identity",
            "instance_initialisation",
            "ticket_key_generation",
        ]
    );
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires PostgreSQL (IMPLUVIUM_TEST_DATABASE_URL)"]
async fn the_schema_has_exactly_the_expected_constraints() {
    let test = TestDb::migrated().await;
    // Primary key, unique and check constraints; not-null constraints are
    // catalog entries only on newer servers and are checked by the column tests.
    let mut found = test
        .text_column(
            "SELECT c.conname::text FROM pg_constraint c \
             JOIN pg_class t ON t.oid = c.conrelid \
             JOIN pg_namespace n ON n.oid = t.relnamespace \
             WHERE n.nspname = 'public' AND c.contype IN ('p', 'u', 'c') \
             AND t.relname NOT LIKE '\\_sqlx%'",
        )
        .await;
    found.sort();
    let mut expected = vec![
        "content_key_binding_ck_algorithm",
        "content_key_binding_ck_key_id",
        "content_key_binding_ck_public_key",
        "content_key_binding_ck_retired",
        "content_key_binding_pk",
        "content_key_binding_uq_public_key",
        "instance_identity_ck_layout_version",
        "instance_identity_ck_singleton",
        "instance_identity_pk",
        "instance_initialisation_ck_completed",
        "instance_initialisation_ck_order",
        "instance_initialisation_ck_singleton",
        "instance_initialisation_ck_state",
        "instance_initialisation_pk",
        "ticket_key_generation_ck_generation",
        "ticket_key_generation_ck_key_id",
        "ticket_key_generation_pk",
        "ticket_key_generation_uq_switch_at",
    ];
    expected.sort();
    assert_eq!(found, expected);

    let indexes = test
        .text_column(
            "SELECT indexname::text FROM pg_indexes \
             WHERE schemaname = 'public' AND tablename NOT LIKE '\\_sqlx%'",
        )
        .await;
    assert!(
        indexes
            .iter()
            .any(|name| name == "content_key_binding_uq_active"),
        "{indexes:?}"
    );

    let nullable = test
        .text_column(
            "SELECT table_name::text || '.' || column_name::text FROM information_schema.columns \
             WHERE table_schema = 'public' AND table_name <> '_sqlx_migrations' \
             AND is_nullable = 'YES' ORDER BY 1",
        )
        .await;
    assert_eq!(
        nullable,
        [
            "content_key_binding.retired_at",
            "instance_initialisation.completed_at"
        ]
    );
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires PostgreSQL (IMPLUVIUM_TEST_DATABASE_URL)"]
async fn single_row_tables_reject_a_second_row() {
    let test = TestDb::migrated().await;
    test.run("INSERT INTO instance_identity (layout_version) VALUES (1)")
        .await;
    assert_eq!(
        test.rejected("INSERT INTO instance_identity (layout_version) VALUES (1)")
            .await,
        UNIQUE_VIOLATION
    );
    assert_eq!(
        test.rejected(
            "INSERT INTO instance_identity (singleton, layout_version) VALUES (false, 1)"
        )
        .await,
        CHECK_VIOLATION
    );
    assert_eq!(
        test.rejected("UPDATE instance_identity SET layout_version = 0")
            .await,
        CHECK_VIOLATION
    );
    test.run("INSERT INTO instance_initialisation (state) VALUES ('started')")
        .await;
    assert_eq!(
        test.rejected("INSERT INTO instance_initialisation (state) VALUES ('started')")
            .await,
        UNIQUE_VIOLATION
    );
    assert_eq!(
        test.rejected(
            "INSERT INTO instance_initialisation (singleton, state) VALUES (false, 'started')"
        )
        .await,
        CHECK_VIOLATION
    );
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires PostgreSQL (IMPLUVIUM_TEST_DATABASE_URL)"]
async fn only_one_content_key_binding_is_active() {
    let test = TestDb::migrated().await;
    test.run(
        "INSERT INTO content_key_binding (key_id, public_key, algorithm) \
         VALUES (decode('0001020304050607', 'hex'), decode(repeat('ab', 32), 'hex'), 'ed25519')",
    )
    .await;
    assert_eq!(
        test.rejected(
            "INSERT INTO content_key_binding (key_id, public_key, algorithm) \
             VALUES (decode('0807060504030201', 'hex'), decode(repeat('cd', 32), 'hex'), 'ed25519')"
        )
        .await,
        UNIQUE_VIOLATION
    );
    // Retiring the binding frees the slot.
    test.run("UPDATE content_key_binding SET retired_at = now()")
        .await;
    test.run(
        "INSERT INTO content_key_binding (key_id, public_key, algorithm) \
         VALUES (decode('0807060504030201', 'hex'), decode(repeat('cd', 32), 'hex'), 'ed25519')",
    )
    .await;
    assert_eq!(
        test.number("SELECT count(*) FROM content_key_binding")
            .await,
        2
    );
    assert_eq!(
        test.number("SELECT count(*) FROM content_key_binding WHERE retired_at IS NULL")
            .await,
        1
    );
    // The key may be bound once, whatever its state.
    assert_eq!(
        test.rejected(
            "INSERT INTO content_key_binding (key_id, public_key, algorithm, retired_at) \
             VALUES (decode('0807060504030201', 'hex'), decode(repeat('cd', 32), 'hex'), 'ed25519', now())"
        )
        .await,
        UNIQUE_VIOLATION
    );
    for statement in [
        // wrong key id length
        "INSERT INTO content_key_binding (key_id, public_key, algorithm, retired_at) \
         VALUES (decode('00', 'hex'), decode(repeat('ee', 32), 'hex'), 'ed25519', now())",
        // wrong public key length
        "INSERT INTO content_key_binding (key_id, public_key, algorithm, retired_at) \
         VALUES (decode('0001020304050607', 'hex'), decode(repeat('ee', 31), 'hex'), 'ed25519', now())",
        // unknown algorithm
        "INSERT INTO content_key_binding (key_id, public_key, algorithm, retired_at) \
         VALUES (decode('0001020304050607', 'hex'), decode(repeat('ee', 32), 'hex'), 'rsa', now())",
        // retired before it was created
        "INSERT INTO content_key_binding (key_id, public_key, algorithm, created_at, retired_at) \
         VALUES (decode('0001020304050607', 'hex'), decode(repeat('ee', 32), 'hex'), 'ed25519', now(), now() - interval '1 day')",
    ] {
        assert_eq!(
            test.rejected(statement).await,
            CHECK_VIOLATION,
            "{statement}"
        );
    }
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires PostgreSQL (IMPLUVIUM_TEST_DATABASE_URL)"]
async fn the_initialisation_state_matches_its_completion_time() {
    let test = TestDb::migrated().await;
    for statement in [
        "INSERT INTO instance_initialisation (state) VALUES ('finished')",
        "INSERT INTO instance_initialisation (state, completed_at) VALUES ('started', now())",
        "INSERT INTO instance_initialisation (state) VALUES ('completed')",
        "INSERT INTO instance_initialisation (state, started_at, completed_at) \
         VALUES ('completed', now(), now() - interval '1 day')",
    ] {
        assert_eq!(
            test.rejected(statement).await,
            CHECK_VIOLATION,
            "{statement}"
        );
    }
    test.run("INSERT INTO instance_initialisation (state) VALUES ('started')")
        .await;
    test.run("UPDATE instance_initialisation SET state = 'completed', completed_at = now()")
        .await;
    assert_eq!(
        test.number("SELECT count(*) FROM instance_initialisation WHERE state = 'completed'")
            .await,
        1
    );
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires PostgreSQL (IMPLUVIUM_TEST_DATABASE_URL)"]
async fn ticket_key_generations_are_checked() {
    let test = TestDb::migrated().await;
    let insert = |generation: &str, key_id: &str, switch: &str| {
        format!(
            "INSERT INTO ticket_key_generation (generation, key_id, switch_at) \
             VALUES ({generation}, decode('{key_id}', 'hex'), {switch})"
        )
    };
    let at = |year: u32| format!("'{year}-01-01T00:00:00Z'");
    test.run(&insert("1", "0001020304050607", &at(2030))).await;
    test.run(&insert("4294967295", "0807060504030201", &at(2031)))
        .await;
    for statement in [
        insert("0", "0001020304050607", &at(2032)),
        insert("4294967296", "0001020304050607", &at(2032)),
        insert("2", "00", &at(2032)),
    ] {
        assert_eq!(
            test.rejected(&statement).await,
            CHECK_VIOLATION,
            "{statement}"
        );
    }
    assert_eq!(
        test.rejected(&insert("2", "0001020304050607", &at(2031)))
            .await,
        UNIQUE_VIOLATION,
        "a switch time is used once"
    );
    assert_eq!(
        test.rejected(&insert("1", "0001020304050607", &at(2033)))
            .await,
        UNIQUE_VIOLATION,
        "a generation is used once"
    );
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires PostgreSQL (IMPLUVIUM_TEST_DATABASE_URL)"]
async fn an_edited_applied_migration_is_refused() {
    let test = TestDb::migrated().await;
    // The recorded checksum no longer matches the embedded file, as if the file
    // had been edited after it was applied.
    test.run(
        "UPDATE _sqlx_migrations SET checksum = decode(repeat('00', 48), 'hex') \
         WHERE version = 1",
    )
    .await;
    let error = test
        .db
        .migrate()
        .await
        .expect_err("the changed migration is refused");
    assert!(
        matches!(error, DbError::Migrate(MigrateFailure::ChecksumMismatch)),
        "{error:?}"
    );
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires PostgreSQL (IMPLUVIUM_TEST_DATABASE_URL)"]
async fn an_applied_migration_unknown_to_this_build_is_refused() {
    let test = TestDb::migrated().await;
    test.run(
        "INSERT INTO _sqlx_migrations (version, description, success, checksum, execution_time) \
         VALUES (999, 'from a newer version', true, decode(repeat('00', 48), 'hex'), 0)",
    )
    .await;
    let error = test
        .db
        .migrate()
        .await
        .expect_err("the unknown migration is refused");
    assert!(
        matches!(error, DbError::Migrate(MigrateFailure::UnknownApplied)),
        "{error:?}"
    );
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires PostgreSQL (IMPLUVIUM_TEST_DATABASE_URL)"]
async fn a_wrong_password_is_reported_without_leaking_it() {
    const WRONG: &str = "wrong-pw-marker-c0ffee";
    let admin = admin_url();
    let (user_part, host_part) = user_and_host(&admin);
    let database = admin.rsplit_once('/').expect("a database name").1;
    let url = DatabaseUrl::new(format!("{user_part}:{WRONG}@{host_part}/{database}"));
    let error = Database::connect(&url, &options())
        .await
        .expect_err("the server must require password authentication and refuse the wrong one");
    assert!(
        matches!(error, DbError::Connect(ConnectFailure::Authentication)),
        "{error:?}"
    );
    let host = host_part
        .rsplit_once(':')
        .map_or(host_part.as_str(), |(host, _)| host);
    for text in [error.to_string(), format!("{error:?}")] {
        assert!(!text.contains(WRONG), "the password leaked: {text}");
        assert!(!text.contains(host), "the host leaked: {text}");
    }
    assert!(!format!("{url:?} {url}").contains(WRONG));
}

fn now_micros() -> i64 {
    i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("a clock after 1970")
            .as_micros(),
    )
    .expect("a time that fits")
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires PostgreSQL (IMPLUVIUM_TEST_DATABASE_URL)"]
async fn the_instance_identity_is_read() {
    let test = TestDb::migrated().await;
    assert_eq!(test.db.instance_identity().await.unwrap(), None);
    test.run("INSERT INTO instance_identity (layout_version) VALUES (7)")
        .await;
    let identity = test.db.instance_identity().await.unwrap().expect("a row");
    assert_eq!(identity.layout_version, 7);
    assert_eq!(identity.instance_id.len(), 36, "{}", identity.instance_id);
    // The server clock and the test clock agree to within a minute.
    assert!((identity.created_at_micros - now_micros()).abs() < 60_000_000);
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires PostgreSQL (IMPLUVIUM_TEST_DATABASE_URL)"]
async fn only_the_active_content_key_binding_is_read() {
    let test = TestDb::migrated().await;
    assert_eq!(test.db.active_content_key_binding().await.unwrap(), None);
    test.run(
        "INSERT INTO content_key_binding (key_id, public_key, algorithm) \
         VALUES (decode('0001020304050607', 'hex'), decode(repeat('ab', 32), 'hex'), 'ed25519')",
    )
    .await;
    let binding = test
        .db
        .active_content_key_binding()
        .await
        .unwrap()
        .expect("an active binding");
    assert_eq!(binding.key_id, [0, 1, 2, 3, 4, 5, 6, 7]);
    assert_eq!(binding.public_key, [0xab; 32]);
    assert_eq!(binding.algorithm, "ed25519");
    test.run("UPDATE content_key_binding SET retired_at = now()")
        .await;
    assert_eq!(test.db.active_content_key_binding().await.unwrap(), None);
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires PostgreSQL (IMPLUVIUM_TEST_DATABASE_URL)"]
async fn ticket_key_generations_are_listed_lowest_first() {
    let test = TestDb::migrated().await;
    assert!(test.db.ticket_key_generations().await.unwrap().is_empty());
    test.run(
        "INSERT INTO ticket_key_generation (generation, key_id, switch_at) VALUES \
         (2, decode('0807060504030201', 'hex'), '2031-01-01T00:00:00Z'), \
         (1, decode('0001020304050607', 'hex'), '2030-01-01T00:00:00Z')",
    )
    .await;
    let generations = test.db.ticket_key_generations().await.unwrap();
    let numbers: Vec<i64> = generations.iter().map(|g| g.generation).collect();
    assert_eq!(numbers, [1, 2]);
    assert_eq!(generations[0].key_id, [0, 1, 2, 3, 4, 5, 6, 7]);
    // 2030-01-01T00:00:00Z
    assert_eq!(generations[0].switch_at_micros, 1_893_456_000_000_000);
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires PostgreSQL (IMPLUVIUM_TEST_DATABASE_URL)"]
async fn the_initialisation_state_is_read() {
    let test = TestDb::migrated().await;
    assert_eq!(test.db.initialisation().await.unwrap(), None);
    test.run("INSERT INTO instance_initialisation (state) VALUES ('started')")
        .await;
    let started = test.db.initialisation().await.unwrap().expect("a row");
    assert_eq!(started.state, InitialisationState::Started);
    assert_eq!(started.completed_at_micros, None);
    test.run("UPDATE instance_initialisation SET state = 'completed', completed_at = now()")
        .await;
    let completed = test.db.initialisation().await.unwrap().expect("a row");
    assert_eq!(completed.state, InitialisationState::Completed);
    assert!(completed.completed_at_micros.unwrap() >= completed.started_at_micros);
}
