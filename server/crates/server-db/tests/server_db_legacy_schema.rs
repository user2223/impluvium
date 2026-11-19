// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Schema parity between the inherited Prisma migration chain and the plain-SQL
//! fixture `tests/fixtures/legacy-baseline.sql`.
//!
//! The fixture is test material only. The server never starts with this schema
//! and the product migrations do not contain it. These tests prove, per
//! PostgreSQL major version, that the 121 inherited migrations and the fixture
//! create the same schema, including the historical constraint names.
//!
//! Every test needs a server and is ignored in a normal test run. Run them with
//!
//! ```text
//! IMPLUVIUM_TEST_DATABASE_URL=postgres://USER:PASSWORD@HOST:5432/postgres \
//!   cargo test -p server-db --test server_db_legacy_schema -- --ignored --test-threads=1
//! ```
//!
//! The user must be allowed to create databases and the extension `pg_trgm`.
//! Each test creates its own databases and drops them at the end, also when the
//! test fails.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use server_db::MIGRATOR;
use sqlx::{AssertSqlSafe, Connection, PgConnection};

const URL_VARIABLE: &str = "IMPLUVIUM_TEST_DATABASE_URL";
const FIXTURE: &str = include_str!("fixtures/legacy-baseline.sql");
const INHERITED_MIGRATIONS: usize = 121;

/// The URL of the maintenance database. A missing variable fails the test.
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

static COUNTER: AtomicU32 = AtomicU32::new(0);

/// A fresh, empty database with an open connection. The database is dropped
/// when this value goes out of scope, also during a panic.
struct TestDatabase {
    connection: PgConnection,
    admin_url: String,
    name: String,
}

impl TestDatabase {
    async fn new() -> Self {
        let admin_url = admin_url();
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("a clock after 1970")
            .as_nanos();
        let name = format!(
            "impluvium_legacy_{}_{nanos:x}_{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::SeqCst)
        );
        let mut admin = PgConnection::connect(&admin_url)
            .await
            .expect("the maintenance database accepts the test connection");
        sqlx::query(AssertSqlSafe(format!("CREATE DATABASE \"{name}\"")))
            .execute(&mut admin)
            .await
            .expect("the test user may create databases");
        // Registered before the connect below, so a failure there still drops the database.
        let mut test = Self {
            // Replaced right below; the maintenance connection serves as a placeholder.
            connection: admin,
            admin_url: admin_url.clone(),
            name: name.clone(),
        };
        test.connection = PgConnection::connect(&with_database(&admin_url, &name))
            .await
            .expect("the new database accepts the test connection");
        test
    }

    /// Runs a script of several statements in one round trip.
    async fn script(&mut self, sql: String, what: &str) {
        sqlx::raw_sql(AssertSqlSafe(sql))
            .execute(&mut self.connection)
            .await
            .unwrap_or_else(|error| panic!("{what} failed: {error}"));
    }

    async fn text_column(&mut self, sql: &'static str) -> Vec<String> {
        sqlx::query_scalar::<_, String>(sql)
            .fetch_all(&mut self.connection)
            .await
            .expect("the catalog query runs")
    }

    async fn server_version_num(&mut self) -> i64 {
        let version: String = sqlx::query_scalar("SHOW server_version_num")
            .fetch_one(&mut self.connection)
            .await
            .expect("the server reports its version");
        version.parse().expect("a numeric server_version_num")
    }

    async fn snapshot(&mut self) -> Vec<String> {
        self.text_column(SNAPSHOT_SQL).await
    }

    /// Applies the inherited chain in directory order and returns how many ran.
    async fn apply_chain(&mut self) -> usize {
        let files = chain_files();
        for file in &files {
            let sql = std::fs::read_to_string(file)
                .unwrap_or_else(|error| panic!("{} is readable: {error}", file.display()));
            self.script(sql, &format!("inherited migration {}", file.display()))
                .await;
        }
        files.len()
    }

    async fn apply_fixture(&mut self) {
        self.script(FIXTURE.to_owned(), "the legacy fixture").await;
    }
}

impl Drop for TestDatabase {
    fn drop(&mut self) {
        let admin_url = self.admin_url.clone();
        let name = self.name.clone();
        // A destructor cannot await, so a short-lived runtime on its own thread does it.
        // The open connection is closed by the server when the drop below forces it.
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

fn server_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The `migration.sql` files of the inherited chain, in directory order.
fn chain_files() -> Vec<PathBuf> {
    let root = server_dir().join("prisma/migrations");
    let mut directories: Vec<PathBuf> = std::fs::read_dir(&root)
        .unwrap_or_else(|error| panic!("{} is readable: {error}", root.display()))
        .map(|entry| entry.expect("a directory entry").path())
        .filter(|path| path.is_dir())
        .collect();
    directories.sort();
    directories
        .into_iter()
        .map(|directory| directory.join("migration.sql"))
        .collect()
}

/// A deterministic description of the schema `public`, one text per catalog
/// entry, sorted bytewise. Objects owned by an extension, the two migration
/// history tables and anything outside `public` are left out.
///
/// Columns are listed with their rank among the live columns of the table
/// rather than their attribute number: dropped columns leave gaps in the chain
/// that a fresh table does not have, but the order of the live columns is part
/// of the schema. PostgreSQL 18 stores NOT NULL as named constraints (contype
/// `n`); older servers have none, and the constraint list then simply lacks them.
const SNAPSHOT_SQL: &str = r#"
SELECT entry FROM (
    SELECT concat('extension ', e.extname, ' in ', n.nspname) AS entry
    FROM pg_extension e
    JOIN pg_namespace n ON n.oid = e.extnamespace
    WHERE e.extname <> 'plpgsql'

    UNION ALL
    SELECT concat('enum ', t.typname, ' (',
                  string_agg(v.enumlabel, ', ' ORDER BY v.enumsortorder), ')')
    FROM pg_type t
    JOIN pg_namespace n ON n.oid = t.typnamespace AND n.nspname = 'public'
    JOIN pg_enum v ON v.enumtypid = t.oid
    GROUP BY t.oid, t.typname

    UNION ALL
    SELECT concat('relation ', c.relkind::text, ' ', c.relname,
                  ' persistence=', c.relpersistence::text,
                  ' rls=', c.relrowsecurity::text,
                  ' options=', coalesce(c.reloptions::text, ''))
    FROM pg_class c
    JOIN pg_namespace n ON n.oid = c.relnamespace AND n.nspname = 'public'
    WHERE c.relkind IN ('r', 'p', 'v', 'm', 'S', 'f')
      AND c.relname NOT IN ('_prisma_migrations', '_sqlx_migrations')
      AND NOT EXISTS (SELECT 1 FROM pg_depend d
                      WHERE d.classid = 'pg_class'::regclass AND d.objid = c.oid
                        AND d.deptype = 'e')

    UNION ALL
    SELECT concat('column ', c.relname, '.', a.attname,
                  ' rank=', row_number() OVER (PARTITION BY c.oid ORDER BY a.attnum),
                  ' type=', format_type(a.atttypid, a.atttypmod),
                  ' notnull=', a.attnotnull::text,
                  ' default=', coalesce(pg_get_expr(d.adbin, d.adrelid), ''),
                  ' identity=', a.attidentity::text,
                  ' generated=', a.attgenerated::text)
    FROM pg_attribute a
    JOIN pg_class c ON c.oid = a.attrelid
    JOIN pg_namespace n ON n.oid = c.relnamespace AND n.nspname = 'public'
    LEFT JOIN pg_attrdef d ON d.adrelid = a.attrelid AND d.adnum = a.attnum
    WHERE a.attnum > 0 AND NOT a.attisdropped
      AND c.relkind IN ('r', 'p', 'v', 'm', 'f')
      AND c.relname NOT IN ('_prisma_migrations', '_sqlx_migrations')

    UNION ALL
    SELECT concat('constraint ', c.relname, '.', k.conname, ' type=', k.contype::text,
                  ' validated=', k.convalidated::text,
                  ' ', pg_get_constraintdef(k.oid))
    FROM pg_constraint k
    JOIN pg_class c ON c.oid = k.conrelid
    JOIN pg_namespace n ON n.oid = c.relnamespace AND n.nspname = 'public'
    WHERE c.relname NOT IN ('_prisma_migrations', '_sqlx_migrations')

    UNION ALL
    SELECT concat('index ', t.relname, ' ', pg_get_indexdef(i.indexrelid))
    FROM pg_index i
    JOIN pg_class t ON t.oid = i.indrelid
    JOIN pg_namespace n ON n.oid = t.relnamespace AND n.nspname = 'public'
    WHERE t.relname NOT IN ('_prisma_migrations', '_sqlx_migrations')

    UNION ALL
    SELECT concat('sequence ', c.relname, ' type=', format_type(s.seqtypid, NULL),
                  ' start=', s.seqstart, ' increment=', s.seqincrement,
                  ' min=', s.seqmin, ' max=', s.seqmax, ' cycle=', s.seqcycle::text)
    FROM pg_sequence s
    JOIN pg_class c ON c.oid = s.seqrelid
    JOIN pg_namespace n ON n.oid = c.relnamespace AND n.nspname = 'public'

    UNION ALL
    SELECT concat('trigger ', pg_get_triggerdef(g.oid))
    FROM pg_trigger g
    JOIN pg_class c ON c.oid = g.tgrelid
    JOIN pg_namespace n ON n.oid = c.relnamespace AND n.nspname = 'public'
    WHERE NOT g.tgisinternal

    UNION ALL
    SELECT concat('function ', pg_get_functiondef(p.oid))
    FROM pg_proc p
    JOIN pg_namespace n ON n.oid = p.pronamespace AND n.nspname = 'public'
    WHERE p.prokind IN ('f', 'p')
      AND NOT EXISTS (SELECT 1 FROM pg_depend d
                      WHERE d.classid = 'pg_proc'::regclass AND d.objid = p.oid
                        AND d.deptype = 'e')
) snapshot
ORDER BY entry COLLATE "C"
"#;

/// Only the differing entries, one per line: `-` only in the first schema,
/// `+` only in the second. Empty when the snapshots are equal.
fn difference(first: &[String], second: &[String]) -> String {
    let first_set: BTreeSet<&String> = first.iter().collect();
    let second_set: BTreeSet<&String> = second.iter().collect();
    let mut lines = Vec::new();
    for entry in first_set.difference(&second_set) {
        lines.push(format!("- {entry}"));
    }
    for entry in second_set.difference(&first_set) {
        lines.push(format!("+ {entry}"));
    }
    lines.join("\n")
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires PostgreSQL (IMPLUVIUM_TEST_DATABASE_URL)"]
async fn the_inherited_chain_and_the_fixture_create_the_same_schema() {
    let mut chain = TestDatabase::new().await;
    let applied = chain.apply_chain().await;
    assert_eq!(applied, INHERITED_MIGRATIONS, "the inherited chain changed");
    let mut fixture = TestDatabase::new().await;
    fixture.apply_fixture().await;

    let from_chain = chain.snapshot().await;
    let from_fixture = fixture.snapshot().await;

    // A comparison of two empty or trivial snapshots would prove nothing.
    assert!(
        from_chain
            .iter()
            .any(|e| e.starts_with("extension pg_trgm ")),
        "the snapshot lacks the extension"
    );
    assert!(
        from_chain.iter().filter(|e| e.starts_with("enum ")).count() >= 5,
        "the snapshot lacks the enum types"
    );
    assert!(
        from_chain
            .iter()
            .filter(|e| e.starts_with("column "))
            .count()
            > 100,
        "the snapshot lacks columns"
    );
    assert!(
        from_chain
            .iter()
            .any(|e| e.starts_with("constraint Game.Game_pkey ")),
        "the snapshot lacks constraints"
    );
    assert!(
        from_chain.iter().any(|e| e.starts_with("index ")),
        "the snapshot lacks indexes"
    );
    // The historical NOT NULL names exist as catalog entries on PostgreSQL 18 only.
    if fixture.server_version_num().await >= 180_000 {
        assert!(
            from_chain
                .iter()
                .any(|e| e.starts_with("constraint Game.Game_mIconId_not_null type=n ")),
            "PostgreSQL 18 keeps the historical NOT NULL name after the rename"
        );
    }

    let diff = difference(&from_chain, &from_fixture);
    assert!(
        diff.is_empty(),
        "the inherited chain (-) and the fixture (+) differ:\n{diff}"
    );
    assert_eq!(from_chain, from_fixture);
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires PostgreSQL (IMPLUVIUM_TEST_DATABASE_URL)"]
async fn the_snapshot_notices_changes_of_the_schema() {
    let mut test = TestDatabase::new().await;
    test.apply_fixture().await;
    let mut previous = test.snapshot().await;
    let changes = [
        r#"ALTER TABLE "Game" RENAME CONSTRAINT "Game_pkey" TO "Game_pkey_renamed""#,
        r#"ALTER TABLE "Game" ADD COLUMN "extra" integer"#,
        r#"ALTER TABLE "Game" ALTER COLUMN "libraryPath" DROP NOT NULL"#,
        r#"ALTER TABLE "Game" ALTER COLUMN "extra" SET DEFAULT 7"#,
        r#"CREATE INDEX "Game_extra_idx" ON "Game" ("extra")"#,
        r#"ALTER TYPE "Platform" ADD VALUE 'extra_platform'"#,
    ];
    for change in changes {
        test.script(change.to_owned(), change).await;
        let current = test.snapshot().await;
        assert!(
            !difference(&previous, &current).is_empty(),
            "the snapshot did not notice: {change}"
        );
        previous = current;
    }
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires PostgreSQL (IMPLUVIUM_TEST_DATABASE_URL)"]
async fn the_fixture_is_not_a_product_migration() {
    // The fixture is read at build time by this test only and holds the inherited schema.
    assert!(FIXTURE.contains("\"serverName\""));
    assert!(
        !FIXTURE.lines().any(|line| line.starts_with('\\')),
        "the fixture must run through raw_sql, without psql meta-commands"
    );

    // The product migrator embeds the files of server/migrations only.
    let versions: Vec<i64> = MIGRATOR.migrations.iter().map(|m| m.version).collect();
    let on_disk: BTreeSet<String> = std::fs::read_dir(server_dir().join("migrations"))
        .expect("the product migrations are readable")
        .map(|entry| {
            entry
                .expect("a directory entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    assert_eq!(versions, [1]);
    assert_eq!(
        on_disk,
        BTreeSet::from(["0001_baseline.sql".to_owned()]),
        "the product migrations are exactly the files the migrator embeds"
    );

    // No product migration carries the content of the fixture.
    for name in &on_disk {
        let path = server_dir().join("migrations").join(name);
        let sql = std::fs::read_to_string(&path).expect("a readable product migration");
        for marker in ["\"serverName\"", "\"_prisma_migrations\"", "pg_trgm"] {
            assert!(
                !sql.contains(marker),
                "{} contains {marker} from the legacy schema",
                path.display()
            );
        }
    }
}
