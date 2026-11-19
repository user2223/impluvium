// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! The start guard against a real PostgreSQL server: one owner per database, the identity check,
//! the first initialisation and its resumption, the migration history and the loss of ownership.
//!
//! Every test needs a server and is ignored in a normal test run. Run them with
//!
//! ```text
//! IMPLUVIUM_TEST_DATABASE_URL=postgres://USER:PASSWORD@HOST:5432/postgres \
//!   cargo test -p server-db --test server_db_start_guard -- --ignored --test-threads=1
//! ```
//!
//! The user must be allowed to create and drop databases and to terminate its own sessions. Each
//! test creates its own databases and data directories and removes them at the end, also when the
//! test fails. The instance key is replaced by fixed public values: no key is derived here.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use server_db::DatabaseUrl;
use server_db::backup::BackupMode;
use server_db::instance::{DataMarker, InstanceId, LAYOUT_VERSION, read_marker};
use server_db::start_guard::{
    BackupReport, IdentityRefusal, InitStep, InstanceKeys, KeyError, LoadedKeys, LossReason,
    MigrationRefusal, Ownership, PublicKeys, StartError, StartKind, StartOptions, Started, start,
};
use sqlx::{AssertSqlSafe, Connection, PgConnection};

const URL_VARIABLE: &str = "IMPLUVIUM_TEST_DATABASE_URL";

/// Public synthetic known-answer values of the protocol vectors: the content public key and key
/// identifier and the first ticket key identifier derived from the instance key 00 01 .. 1f.
const CONTENT_PUBLIC_KEY: &str = "5f5603361a832deb6ae7edfbafa292396c2158650ae6d267fd10416f8eb36e8d";
const CONTENT_KEY_ID: &str = "e3176bf3bff6f8e7";
const TICKET_KEY_ID: &str = "417b006e05e6fbee";

/// The schema of the inherited application: a database that is not ours and has no migration table.
const LEGACY_SCHEMA: &str = include_str!("fixtures/legacy-baseline.sql");

fn unhex<const N: usize>(text: &str) -> [u8; N] {
    let mut out = [0u8; N];
    assert_eq!(text.len(), N * 2);
    for (index, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[index * 2..index * 2 + 2], 16).expect("hex");
    }
    out
}

type Material = (PublicKeys, [u8; 32]);

/// The known-answer material of the protocol vectors.
fn material_a() -> Material {
    (
        PublicKeys {
            content_public_key: unhex(CONTENT_PUBLIC_KEY),
            content_key_id: unhex(CONTENT_KEY_ID),
            ticket_key_id: unhex(TICKET_KEY_ID),
        },
        std::array::from_fn(|index| index as u8),
    )
}

/// Other fixed values, standing for another instance.
fn material_b() -> Material {
    (
        PublicKeys {
            content_public_key: [7; 32],
            content_key_id: [8; 8],
            ticket_key_id: [9; 8],
        },
        [4; 32],
    )
}

/// An instance key file in memory: absent until created.
struct FakeKeys {
    slot: Mutex<Option<Material>>,
    material: Material,
    creations: AtomicU32,
}

impl FakeKeys {
    fn absent(material: Material) -> Self {
        Self {
            slot: Mutex::new(None),
            material,
            creations: AtomicU32::new(0),
        }
    }

    fn present(material: Material) -> Self {
        let keys = Self::absent(material.clone());
        *keys.slot.lock().unwrap() = Some(material);
        keys
    }

    fn exists(&self) -> bool {
        self.slot.lock().unwrap().is_some()
    }

    fn creations(&self) -> u32 {
        self.creations.load(Ordering::SeqCst)
    }
}

impl InstanceKeys for FakeKeys {
    fn existing(&self) -> Result<Option<LoadedKeys>, KeyError> {
        Ok(self
            .slot
            .lock()
            .unwrap()
            .clone()
            .map(|(public, instance_key)| LoadedKeys {
                public,
                instance_key,
            }))
    }

    fn create(&self) -> Result<PublicKeys, KeyError> {
        let mut slot = self.slot.lock().unwrap();
        if slot.is_some() {
            return Err(KeyError::new("the key exists already"));
        }
        *slot = Some(self.material.clone());
        self.creations.fetch_add(1, Ordering::SeqCst);
        Ok(self.material.0.clone())
    }
}

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
    match query {
        Some(query) => format!("{head}/{name}?{query}"),
        None => format!("{head}/{name}"),
    }
}

static COUNTER: AtomicU32 = AtomicU32::new(0);

fn unique(prefix: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("a clock after 1970")
        .as_nanos();
    format!(
        "{prefix}_{}_{nanos:x}_{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::SeqCst)
    )
}

/// A database created for one test and dropped at the end, also during a panic.
struct TestDb {
    admin_url: String,
    name: String,
    url: DatabaseUrl,
    url_text: String,
}

impl TestDb {
    async fn new() -> Self {
        let admin_url = admin_url();
        let name = unique("impluvium_test_guard");
        let mut connection = PgConnection::connect(&admin_url)
            .await
            .expect("the maintenance database accepts the test connection");
        sqlx::query(AssertSqlSafe(format!("CREATE DATABASE \"{name}\"")))
            .execute(&mut connection)
            .await
            .expect("the test user may create databases");
        let _ = connection.close().await;
        let url_text = with_database(&admin_url, &name);
        Self {
            admin_url,
            name,
            url: DatabaseUrl::new(url_text.clone()),
            url_text,
        }
    }

    async fn connection(&self) -> PgConnection {
        PgConnection::connect(&self.url_text)
            .await
            .expect("the test database accepts a connection")
    }

    async fn run(&self, sql: &str) -> u64 {
        let mut connection = self.connection().await;
        // Test statements are written in this file; none carries outside input.
        let result = sqlx::query(AssertSqlSafe(sql.to_owned()))
            .execute(&mut connection)
            .await
            .unwrap_or_else(|error| panic!("statement failed: {sql}: {error}"));
        let _ = connection.close().await;
        result.rows_affected()
    }

    /// Runs a script of several statements.
    async fn run_script(&self, sql: &str) {
        let mut connection = self.connection().await;
        sqlx::raw_sql(AssertSqlSafe(sql.to_owned()))
            .execute(&mut connection)
            .await
            .expect("the script runs");
        let _ = connection.close().await;
    }

    async fn number(&self, sql: &'static str) -> i64 {
        let mut connection = self.connection().await;
        let value = sqlx::query_scalar::<_, i64>(sql)
            .fetch_one(&mut connection)
            .await
            .expect("the count query runs");
        let _ = connection.close().await;
        value
    }

    async fn text(&self, sql: &'static str) -> String {
        let mut connection = self.connection().await;
        let value = sqlx::query_scalar::<_, String>(sql)
            .fetch_one(&mut connection)
            .await
            .expect("the text query runs");
        let _ = connection.close().await;
        value
    }

    /// Ends the backend that holds the instance lock, from another session.
    async fn terminate_owner(&self) {
        let mut admin = PgConnection::connect(&self.admin_url)
            .await
            .expect("the maintenance database accepts the test connection");
        let ended = sqlx::query_scalar::<_, bool>(
            "SELECT pg_terminate_backend(pid) FROM pg_stat_activity \
             WHERE application_name = 'impluvium-server-owner' AND datname = $1",
        )
        .bind(&self.name)
        .fetch_all(&mut admin)
        .await
        .expect("the owner session can be ended");
        let _ = admin.close().await;
        assert_eq!(ended, [true], "exactly one owner session exists");
    }
}

impl Drop for TestDb {
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

/// A data directory removed at the end.
struct DataDir(PathBuf);

impl DataDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(unique("impluvium_guard_data"));
        fs::create_dir_all(&path).expect("a data directory");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn marker(&self) -> Option<DataMarker> {
        read_marker(&self.0).expect("the marker is readable")
    }
}

impl Drop for DataDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn options<'a>(db: &'a TestDb, data: &'a DataDir, keys: &'a FakeKeys) -> StartOptions<'a> {
    let mut options = StartOptions::new(&db.url, data.path(), keys);
    options.backup_mode = BackupMode::Off;
    options.monitor_interval = Duration::from_millis(200);
    options.monitor_timeout = Duration::from_secs(5);
    options
}

async fn started(db: &TestDb, data: &DataDir, keys: &FakeKeys) -> Started {
    start(&options(db, data, keys))
        .await
        .unwrap_or_else(|error| panic!("the start was refused: {error}"))
}

/// Starts and gives the lock up again.
async fn initialise(db: &TestDb, data: &DataDir, keys: &FakeKeys) -> InstanceId {
    let started = started(db, data, keys).await;
    let id = started.instance_id.clone();
    started.owner.release().await;
    id
}

async fn refused(db: &TestDb, data: &DataDir, keys: &FakeKeys) -> StartError {
    match start(&options(db, data, keys)).await {
        Ok(_) => panic!("the start was accepted"),
        Err(error) => error,
    }
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires PostgreSQL (IMPLUVIUM_TEST_DATABASE_URL)"]
async fn eight_concurrent_guards_yield_exactly_one_owner() {
    let db = TestDb::new().await;
    let data = DataDir::new();
    let keys = FakeKeys::absent(material_a());
    let attempt = || async { start(&options(&db, &data, &keys)).await };
    // Eight starts in one task: they interleave at every await point, so they race on the lock.
    let (r0, r1, r2, r3, r4, r5, r6, r7) = tokio::join!(
        attempt(),
        attempt(),
        attempt(),
        attempt(),
        attempt(),
        attempt(),
        attempt(),
        attempt()
    );
    let results = vec![r0, r1, r2, r3, r4, r5, r6, r7];
    let (owners, refusals): (Vec<_>, Vec<_>) = results.into_iter().partition(Result::is_ok);
    assert_eq!(owners.len(), 1);
    assert_eq!(refusals.len(), 7);
    for refusal in refusals {
        let error = refusal.expect_err("a refusal");
        assert!(matches!(error, StartError::AnotherServerOwns), "{error}");
        assert_eq!(error.to_string(), "another server owns this database");
    }
    let owner = owners.into_iter().next().expect("one owner").expect("ok");
    assert_eq!(owner.report.kind, StartKind::FirstInitialisation);
    assert_eq!(owner.owner.ownership(), Ownership::Held);
    owner.owner.release().await;
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires PostgreSQL (IMPLUVIUM_TEST_DATABASE_URL)"]
async fn a_fresh_start_initialises_and_the_second_start_is_normal() {
    let db = TestDb::new().await;
    let data = DataDir::new();
    let keys = FakeKeys::absent(material_a());

    let first = started(&db, &data, &keys).await;
    assert_eq!(first.report.kind, StartKind::FirstInitialisation);
    assert_eq!(first.report.applied_migrations, [1]);
    assert_eq!(first.report.backup, BackupReport::NotNeeded);
    assert!(first.report.consequences.is_empty());
    assert_eq!(first.layout_version, LAYOUT_VERSION);
    assert_eq!(
        data.marker(),
        Some(DataMarker {
            instance_id: first.instance_id.clone(),
            layout_version: LAYOUT_VERSION,
        })
    );
    assert_eq!(keys.creations(), 1);
    assert_eq!(
        db.text("SELECT instance_id::text FROM instance_identity")
            .await,
        first.instance_id.as_str()
    );
    assert_eq!(
        db.text("SELECT state FROM instance_initialisation").await,
        "completed"
    );
    assert_eq!(
        db.number("SELECT count(*) FROM content_key_binding").await,
        1
    );
    assert_eq!(
        db.text("SELECT encode(public_key, 'hex') FROM content_key_binding")
            .await,
        CONTENT_PUBLIC_KEY
    );
    assert_eq!(
        db.text("SELECT encode(key_id, 'hex') FROM ticket_key_generation WHERE generation = 1")
            .await,
        TICKET_KEY_ID
    );
    // While the first owner is alive, a second start is refused at once.
    assert!(matches!(
        refused(&db, &data, &keys).await,
        StartError::AnotherServerOwns
    ));
    let id = first.instance_id.clone();
    first.owner.release().await;

    let second = started(&db, &data, &keys).await;
    assert_eq!(second.report.kind, StartKind::Normal);
    assert!(second.report.applied_migrations.is_empty());
    assert_eq!(second.instance_id, id);
    assert_eq!(keys.creations(), 1);
    second.owner.release().await;
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires PostgreSQL (IMPLUVIUM_TEST_DATABASE_URL)"]
async fn a_foreign_database_or_data_directory_is_refused() {
    let db_one = TestDb::new().await;
    let db_two = TestDb::new().await;
    let data_one = DataDir::new();
    let data_two = DataDir::new();
    let keys_one = FakeKeys::absent(material_a());
    let keys_two = FakeKeys::absent(material_b());
    let id_one = initialise(&db_one, &data_one, &keys_one).await;
    let id_two = initialise(&db_two, &data_two, &keys_two).await;
    assert_ne!(id_one, id_two);

    // Swapped data directories (each with its own key).
    for (db, data, keys) in [
        (&db_one, &data_two, &keys_two),
        (&db_two, &data_one, &keys_one),
    ] {
        match refused(db, data, keys).await {
            StartError::Identity(IdentityRefusal::ForeignDatabase) => {}
            other => panic!("unexpected: {other}"),
        }
    }
    // A database that an instance owns, with a data directory that has no marker.
    let bare = DataDir::new();
    match refused(&db_one, &bare, &keys_one).await {
        StartError::Identity(IdentityRefusal::DatabaseWithoutMarker) => {}
        other => panic!("unexpected: {other}"),
    }
    // The marker without its key.
    let no_key = FakeKeys::absent(material_a());
    match refused(&db_one, &data_one, &no_key).await {
        StartError::Identity(IdentityRefusal::MarkerWithoutKey) => {}
        other => panic!("unexpected: {other}"),
    }
    // Nothing was changed by the refusals.
    assert_eq!(
        data_one.marker().map(|marker| marker.instance_id),
        Some(id_one)
    );
    assert_eq!(bare.marker(), None);
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires PostgreSQL (IMPLUVIUM_TEST_DATABASE_URL)"]
async fn a_swapped_instance_key_is_refused() {
    let db = TestDb::new().await;
    let data = DataDir::new();
    initialise(&db, &data, &FakeKeys::absent(material_a())).await;
    match refused(&db, &data, &FakeKeys::present(material_b())).await {
        StartError::Identity(IdentityRefusal::KeyMismatch) => {}
        other => panic!("unexpected: {other}"),
    }
    // The right key still starts.
    let started = started(&db, &data, &FakeKeys::present(material_a())).await;
    assert_eq!(started.report.kind, StartKind::Normal);
    started.owner.release().await;
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires PostgreSQL (IMPLUVIUM_TEST_DATABASE_URL)"]
async fn an_empty_database_with_a_data_directory_needs_the_explicit_reattach() {
    let old_db = TestDb::new().await;
    let new_db = TestDb::new().await;
    let data = DataDir::new();
    let keys = FakeKeys::absent(material_a());
    let id = initialise(&old_db, &data, &keys).await;

    match refused(&new_db, &data, &keys).await {
        StartError::Identity(IdentityRefusal::EmptyDatabaseWithData) => {}
        other => panic!("unexpected: {other}"),
    }
    // Refusing leaves the empty database empty: not even migrated.
    assert_eq!(
        new_db
            .number("SELECT count(*) FROM pg_tables WHERE schemaname = 'public'")
            .await,
        0
    );
    let wrong = InstanceId::parse("0b3c1e9a-0000-4000-8000-0000000000ff").expect("an id");
    let mut with_wrong = options(&new_db, &data, &keys);
    with_wrong.accept_new_database = Some(wrong);
    match start(&with_wrong).await {
        Err(StartError::Identity(IdentityRefusal::EmptyDatabaseWithData)) => {}
        Err(other) => panic!("unexpected: {other}"),
        Ok(_) => panic!("the start was accepted"),
    }

    let mut with_right = options(&new_db, &data, &keys);
    with_right.accept_new_database = Some(id.clone());
    let attached = start(&with_right).await.expect("the reattach is accepted");
    assert_eq!(attached.report.kind, StartKind::Reattached);
    assert_eq!(attached.report.consequences.len(), 3);
    assert_eq!(attached.instance_id, id);
    assert_eq!(attached.report.applied_migrations, [1]);
    assert_eq!(
        new_db
            .text("SELECT instance_id::text FROM instance_identity")
            .await,
        id.as_str()
    );
    assert_eq!(keys.creations(), 1, "the existing key is reused");
    attached.owner.release().await;

    let again = started(&new_db, &data, &keys).await;
    assert_eq!(again.report.kind, StartKind::Normal);
    again.owner.release().await;
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires PostgreSQL (IMPLUVIUM_TEST_DATABASE_URL)"]
async fn a_migration_history_this_build_cannot_continue_is_refused() {
    let db = TestDb::new().await;
    let data = DataDir::new();
    let keys = FakeKeys::absent(material_a());
    initialise(&db, &data, &keys).await;

    let migration_error = |error: StartError| match error {
        StartError::Migration(refusal) => refusal,
        other => panic!("unexpected: {other}"),
    };

    db.run(
        "INSERT INTO _sqlx_migrations (version, description, success, checksum, execution_time) \
         VALUES (999, 'newer', true, '\\x00', 0)",
    )
    .await;
    assert_eq!(
        migration_error(refused(&db, &data, &keys).await),
        MigrationRefusal::Downgrade { version: 999 }
    );
    assert!(
        refused(&db, &data, &keys)
            .await
            .to_string()
            .contains("downgrade refused")
    );
    db.run("DELETE FROM _sqlx_migrations WHERE version = 999")
        .await;

    let mut connection = db.connection().await;
    let checksum =
        sqlx::query_scalar::<_, Vec<u8>>("SELECT checksum FROM _sqlx_migrations WHERE version = 1")
            .fetch_one(&mut connection)
            .await
            .expect("the checksum is readable");
    db.run("UPDATE _sqlx_migrations SET checksum = '\\x00' WHERE version = 1")
        .await;
    assert_eq!(
        migration_error(refused(&db, &data, &keys).await),
        MigrationRefusal::Changed { version: 1 }
    );
    sqlx::query("UPDATE _sqlx_migrations SET checksum = $1 WHERE version = 1")
        .bind(checksum)
        .execute(&mut connection)
        .await
        .expect("the checksum is restored");
    let _ = connection.close().await;

    db.run("UPDATE _sqlx_migrations SET success = false WHERE version = 1")
        .await;
    assert_eq!(
        migration_error(refused(&db, &data, &keys).await),
        MigrationRefusal::Failed { version: 1 }
    );
    db.run("UPDATE _sqlx_migrations SET success = true WHERE version = 1")
        .await;

    let started = started(&db, &data, &keys).await;
    assert_eq!(started.report.kind, StartKind::Normal);
    started.owner.release().await;
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires PostgreSQL (IMPLUVIUM_TEST_DATABASE_URL)"]
async fn a_failed_backup_blocks_the_migration_and_a_failed_migration_is_reported() {
    let db = TestDb::new().await;
    let data = DataDir::new();
    let keys = FakeKeys::absent(material_a());
    initialise(&db, &data, &keys).await;

    // Make the migration look pending: forget that it was applied.
    db.run("DELETE FROM _sqlx_migrations").await;

    let mut failing_backup = options(&db, &data, &keys);
    failing_backup.backup_mode = BackupMode::Command(vec!["false".to_owned()]);
    match start(&failing_backup).await {
        Err(StartError::Backup(_)) => {}
        Err(other) => panic!("unexpected: {other}"),
        Ok(_) => panic!("the start was accepted"),
    }
    assert_eq!(
        db.number("SELECT count(*) FROM _sqlx_migrations").await,
        0,
        "nothing was migrated after a failed backup"
    );

    // With a backup that works, the migration runs, fails (the tables exist already) and the
    // start reports that.
    let mut working_backup = options(&db, &data, &keys);
    working_backup.backup_mode = BackupMode::Command(vec!["true".to_owned()]);
    match start(&working_backup).await {
        Err(StartError::Migrate(_)) => {}
        Err(other) => panic!("unexpected: {other}"),
        Ok(_) => panic!("the start was accepted"),
    }
    assert_eq!(db.number("SELECT count(*) FROM _sqlx_migrations").await, 0);

    // The lock is free again: a start without pending migrations is possible once the history is
    // restored by hand.
    db.run(
        "INSERT INTO _sqlx_migrations (version, description, success, checksum, execution_time) \
         VALUES (1, 'x', true, '\\x00', 0)",
    )
    .await;
    assert!(matches!(
        refused(&db, &data, &keys).await,
        StartError::Migration(MigrationRefusal::Changed { version: 1 })
    ));
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires PostgreSQL (IMPLUVIUM_TEST_DATABASE_URL)"]
async fn a_crash_after_each_initialisation_step_resumes_or_completes() {
    for (step, resumed) in [
        (InitStep::Rows, Some(InitStep::Marker)),
        (InitStep::Marker, Some(InitStep::Key)),
        (InitStep::Key, Some(InitStep::Bind)),
        (InitStep::Bind, None),
    ] {
        let db = TestDb::new().await;
        let data = DataDir::new();
        let keys = FakeKeys::absent(material_a());
        let mut crashing = options(&db, &data, &keys);
        crashing.stop_after = Some(step);
        match start(&crashing).await {
            Err(StartError::InjectedStop(stopped)) => assert_eq!(stopped, step),
            Err(other) => panic!("unexpected: {other}"),
            Ok(_) => panic!("the start was accepted"),
        }
        let state = db.text("SELECT state FROM instance_initialisation").await;
        assert_eq!(
            state,
            if step == InitStep::Bind {
                "completed"
            } else {
                "started"
            }
        );
        assert_eq!(
            data.marker().is_some(),
            step >= InitStep::Marker,
            "{step:?}"
        );
        assert_eq!(keys.exists(), step >= InitStep::Key, "{step:?}");

        let next = started(&db, &data, &keys).await;
        match resumed {
            Some(from) => assert_eq!(next.report.kind, StartKind::Resumed { from }, "{step:?}"),
            None => assert_eq!(next.report.kind, StartKind::Normal),
        }
        assert_eq!(
            keys.creations(),
            1,
            "{step:?}: the key is created exactly once"
        );
        assert_eq!(
            data.marker().map(|marker| marker.instance_id),
            Some(next.instance_id.clone())
        );
        next.owner.release().await;
        let normal = started(&db, &data, &keys).await;
        assert_eq!(normal.report.kind, StartKind::Normal);
        normal.owner.release().await;
    }
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires PostgreSQL (IMPLUVIUM_TEST_DATABASE_URL)"]
async fn an_interrupted_start_next_to_foreign_files_is_refused() {
    let db = TestDb::new().await;
    let data = DataDir::new();
    let keys = FakeKeys::absent(material_a());
    let mut crashing = options(&db, &data, &keys);
    crashing.stop_after = Some(InitStep::Rows);
    assert!(matches!(
        start(&crashing).await,
        Err(StartError::InjectedStop(InitStep::Rows))
    ));

    // A key without a marker.
    let stray_key = FakeKeys::present(material_a());
    assert!(matches!(
        refused(&db, &data, &stray_key).await,
        StartError::Identity(IdentityRefusal::InterruptedInitialisation)
    ));
    // A marker of another instance.
    let foreign = DataDir::new();
    let foreign_marker = DataMarker {
        instance_id: InstanceId::parse("0b3c1e9a-0000-4000-8000-0000000000aa").expect("an id"),
        layout_version: LAYOUT_VERSION,
    };
    server_db::instance::write_marker(foreign.path(), &foreign_marker).expect("a marker");
    assert!(matches!(
        refused(&db, &foreign, &keys).await,
        StartError::Identity(IdentityRefusal::InterruptedInitialisation)
    ));
    // The refusals changed nothing: the interrupted start can still be resumed with its own files.
    assert_eq!(keys.creations(), 0);
    let resumed = started(&db, &data, &keys).await;
    assert_eq!(
        resumed.report.kind,
        StartKind::Resumed {
            from: InitStep::Marker
        }
    );
    resumed.owner.release().await;
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires PostgreSQL (IMPLUVIUM_TEST_DATABASE_URL)"]
async fn a_terminated_lock_connection_is_reported_as_lost() {
    let db = TestDb::new().await;
    let data = DataDir::new();
    let keys = FakeKeys::absent(material_a());
    let started = started(&db, &data, &keys).await;
    assert_eq!(started.owner.ownership(), Ownership::Held);
    // Held ownership stays held while nothing happens.
    tokio::time::sleep(Duration::from_millis(700)).await;
    assert_eq!(started.owner.ownership(), Ownership::Held);

    db.terminate_owner().await;
    let reason = tokio::time::timeout(Duration::from_secs(10), started.owner.lost())
        .await
        .expect("the loss is reported within seconds");
    assert_eq!(reason, LossReason::ConnectionError);
    assert_eq!(started.owner.ownership(), Ownership::Lost(reason));
    started.owner.release().await;
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires PostgreSQL (IMPLUVIUM_TEST_DATABASE_URL)"]
async fn after_the_owner_is_lost_a_new_guard_can_take_ownership() {
    let db = TestDb::new().await;
    let data = DataDir::new();
    let keys = FakeKeys::absent(material_a());
    let first = started(&db, &data, &keys).await;
    let id = first.instance_id.clone();

    db.terminate_owner().await;
    tokio::time::timeout(Duration::from_secs(10), first.owner.lost())
        .await
        .expect("the loss is reported within seconds");

    // The caller ends the old process; a new server then takes over.
    let second = started(&db, &data, &keys).await;
    assert_eq!(second.report.kind, StartKind::Normal);
    assert_eq!(second.instance_id, id);
    assert_eq!(second.owner.ownership(), Ownership::Held);
    // And the new owner keeps the old one out.
    assert!(matches!(
        refused(&db, &data, &keys).await,
        StartError::AnotherServerOwns
    ));
    first.owner.release().await;
    second.owner.release().await;
}

/// A refused start in a database of someone else leaves everything as it was.
async fn assert_nothing_adopted(db: &TestDb, data: &DataDir, keys: &FakeKeys) {
    match refused(db, data, keys).await {
        StartError::Identity(IdentityRefusal::NotEmpty) => {}
        other => panic!("unexpected: {other}"),
    }
    assert_eq!(
        db.number("SELECT count(*) FROM pg_tables WHERE tablename = '_sqlx_migrations'")
            .await,
        0,
        "nothing was migrated"
    );
    assert_eq!(
        db.number("SELECT count(*) FROM pg_tables WHERE tablename = 'instance_identity'")
            .await,
        0
    );
    assert_eq!(data.marker(), None);
    assert!(!keys.exists());
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires PostgreSQL (IMPLUVIUM_TEST_DATABASE_URL)"]
async fn a_database_with_other_tables_is_never_adopted() {
    let db = TestDb::new().await;
    let data = DataDir::new();
    let keys = FakeKeys::absent(material_a());
    db.run("CREATE TABLE somebody_elses_table ()").await;
    assert_nothing_adopted(&db, &data, &keys).await;
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires PostgreSQL (IMPLUVIUM_TEST_DATABASE_URL)"]
async fn the_inherited_legacy_database_is_never_adopted() {
    let db = TestDb::new().await;
    let data = DataDir::new();
    let keys = FakeKeys::absent(material_a());
    db.run_script(LEGACY_SCHEMA).await;
    assert_nothing_adopted(&db, &data, &keys).await;
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires PostgreSQL (IMPLUVIUM_TEST_DATABASE_URL)"]
async fn a_released_owner_publishes_the_release_and_frees_the_lock_at_once() {
    let db = TestDb::new().await;
    let data = DataDir::new();
    let keys = FakeKeys::absent(material_a());
    let first = started(&db, &data, &keys).await;
    let mut watcher = first.owner.subscribe();
    assert_eq!(*watcher.borrow(), Ownership::Held);
    first.owner.release().await;
    assert_eq!(*watcher.borrow_and_update(), Ownership::Released);
    // No waiting for the server to notice a closed socket: the next start gets the lock.
    for _ in 0..3 {
        let next = started(&db, &data, &keys).await;
        next.owner.release().await;
    }
}
