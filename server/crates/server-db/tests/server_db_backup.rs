// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Backups against a real PostgreSQL server with the real `pg_dump` and `pg_restore`.
//!
//! Every test needs a server and the PostgreSQL client tools of a version at least as new as the
//! server, and is ignored in a normal test run. Run them with
//!
//! ```text
//! IMPLUVIUM_TEST_DATABASE_URL=postgres://USER:PASSWORD@HOST:5432/postgres \
//!   cargo test -p server-db --test server_db_backup -- --ignored --test-threads=1
//! ```
//!
//! The user must be allowed to create and drop databases. Each test creates its own databases and
//! drops them at the end, also when the test fails.

use std::fs;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use server_db::backup::{
    BackupContext, BackupKind, BackupMode, BackupOutcome, ToolFailure, ToolPaths,
    backup_before_migration, backup_manual,
};
use server_db::{Database, DatabaseUrl, PoolOptions};
use sqlx::{AssertSqlSafe, Connection, PgConnection};

const URL_VARIABLE: &str = "IMPLUVIUM_TEST_DATABASE_URL";

/// Public synthetic known-answer values of the protocol vectors: the instance key 00 01 .. 1f and
/// the content public key and key identifier derived from it.
const CONTENT_PUBLIC_KEY: &str = "5f5603361a832deb6ae7edfbafa292396c2158650ae6d267fd10416f8eb36e8d";
const CONTENT_KEY_ID: &str = "e3176bf3bff6f8e7";
const INSTANCE_ID: &str = "0b3c1e9a-0000-4000-8000-000000000001";

fn admin_url() -> String {
    match std::env::var("IMPLUVIUM_TEST_DATABASE_URL") {
        Ok(url) if !url.is_empty() => url,
        _ => panic!(
            "{URL_VARIABLE} is not set: these tests need a PostgreSQL server \
             and must not pass without one"
        ),
    }
}

/// Replaces the database name at the end of a URL without a query.
fn with_database(url: &str, name: &str) -> String {
    let (head, _) = url
        .rsplit_once('/')
        .expect("the URL ends in a database name");
    format!("{head}/{name}")
}

/// libpq environment for `scheme://user:password@host:port/name`; the test URL has this plain form.
fn connection_environment(url: &str) -> Vec<(&'static str, String)> {
    let rest = url.split_once("://").expect("a URL with a scheme").1;
    let (authority, _) = rest.split_once('/').expect("a database name");
    let (userinfo, host_port) = authority.rsplit_once('@').expect("credentials");
    let (user, password) = userinfo.split_once(':').expect("a password");
    let (host, port) = host_port.rsplit_once(':').expect("a port");
    vec![
        ("PGUSER", user.to_owned()),
        ("PGPASSWORD", password.to_owned()),
        ("PGHOST", host.to_owned()),
        ("PGPORT", port.to_owned()),
    ]
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

/// Drops a database when it goes out of scope, also during a panic.
struct DropDatabase {
    admin_url: String,
    name: String,
}

impl Drop for DropDatabase {
    fn drop(&mut self) {
        let admin_url = self.admin_url.clone();
        let name = self.name.clone();
        let _ = std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("a runtime to drop the test database");
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

async fn create_database() -> (String, String, DropDatabase) {
    let admin_url = admin_url();
    let name = unique("impluvium_backup_test");
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
    (with_database(&admin_url, &name), name, guard)
}

async fn connect(url: &str) -> Database {
    let options = PoolOptions {
        max_connections: 2,
        ..PoolOptions::default()
    };
    Database::connect(&DatabaseUrl::new(url), &options)
        .await
        .expect("the pool connects")
}

/// A migrated database with the instance identity and the content key binding of the known-answer
/// key, and a data directory that holds the key and the marker.
struct Instance {
    url: String,
    data: PathBuf,
    key: [u8; 32],
    _drop: DropDatabase,
}

impl Instance {
    async fn new() -> Self {
        let (url, _name, guard) = create_database().await;
        let db = connect(&url).await;
        db.migrate().await.expect("the migrations apply");
        sqlx::query(AssertSqlSafe(format!(
            "INSERT INTO instance_identity (instance_id, layout_version) VALUES ('{INSTANCE_ID}', 1)"
        )))
        .execute(db.pool())
        .await
        .expect("the identity is inserted");
        sqlx::query(AssertSqlSafe(format!(
            "INSERT INTO content_key_binding (key_id, public_key, algorithm) \
             VALUES (decode('{CONTENT_KEY_ID}', 'hex'), decode('{CONTENT_PUBLIC_KEY}', 'hex'), 'ed25519')"
        )))
        .execute(db.pool())
        .await
        .expect("the binding is inserted");
        db.close().await;

        let data = std::env::temp_dir().join(unique("impluvium-backup-data"));
        fs::create_dir_all(&data).expect("a data directory");
        let key: [u8; 32] = std::array::from_fn(|index| index as u8);
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true).mode(0o600);
        std::io::Write::write_all(&mut options.open(data.join("instance.key")).unwrap(), &key)
            .unwrap();
        fs::write(data.join(".impluvium-instance"), INSTANCE_ID).unwrap();
        Self {
            url,
            data,
            key,
            _drop: guard,
        }
    }

    fn context(&self, kind: BackupKind, database_url: &DatabaseUrl) -> BackupContextOwner {
        BackupContextOwner {
            url: database_url.clone(),
            kind,
        }
    }
}

impl Drop for Instance {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.data);
    }
}

/// Holds what a [`BackupContext`] borrows from the URL.
struct BackupContextOwner {
    url: DatabaseUrl,
    kind: BackupKind,
}

impl BackupContextOwner {
    fn with<'a>(&'a self, instance: &'a Instance, tools: Option<ToolPaths>) -> BackupContext<'a> {
        BackupContext {
            database_url: &self.url,
            data_dir: &instance.data,
            instance_key: &instance.key,
            kind: self.kind.clone(),
            mode: BackupMode::Auto,
            timeout: Duration::from_secs(120),
            tools,
        }
    }
}

fn pre(from: &str, to: &str) -> BackupKind {
    BackupKind::PreMigration {
        from: from.to_owned(),
        to: to.to_owned(),
    }
}

fn entries(directory: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(directory)
        .expect("a readable directory")
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .filter(|name| name != ".lock")
        .collect();
    names.sort();
    names
}

fn mode(path: &Path) -> u32 {
    fs::metadata(path).unwrap().permissions().mode() & 0o777
}

/// A restore succeeded, or it exited with status 1 because a client of version 17 or newer sent
/// `SET transaction_timeout`, which servers older than 17 reject. That one error is harmless: the
/// data is restored, and the data assertions below check it. Any other error fails.
fn restore_outcome_is_acceptable(
    status: &std::process::ExitStatus,
    stderr: &str,
    server_major: u32,
) -> bool {
    if status.success() {
        return true;
    }
    if status.code() != Some(1) || server_major >= 17 {
        return false;
    }
    let mut saw_error = false;
    for line in stderr.lines() {
        let lower = line.to_lowercase();
        if lower.contains("errors ignored on restore") {
            if !line.trim_end().ends_with(": 1") {
                return false;
            }
        } else if lower.contains("error") {
            if !lower.contains("transaction_timeout") {
                return false;
            }
            saw_error = true;
        }
    }
    saw_error
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires PostgreSQL (IMPLUVIUM_TEST_DATABASE_URL) and the PostgreSQL client tools"]
async fn a_manual_backup_restores_into_a_new_database() {
    let instance = Instance::new().await;
    let owner = instance.context(BackupKind::Manual, &DatabaseUrl::new(&instance.url));
    let set = backup_manual(&owner.with(&instance, None))
        .await
        .expect("the backup succeeds");

    let backups = instance.data.join("backups");
    assert_eq!(entries(&backups), [set.name.as_str()]);
    assert_eq!(
        entries(&set.path),
        [
            ".impluvium-instance",
            "database.dump",
            "instance.key",
            "manifest.json"
        ]
    );
    assert_eq!(mode(&backups), 0o700);
    assert_eq!(mode(&set.path), 0o700);
    for file in entries(&set.path) {
        assert_eq!(mode(&set.path.join(&file)), 0o600, "{file}");
    }
    assert_eq!(
        fs::read(set.path.join("instance.key")).unwrap(),
        instance.key
    );
    let manifest = fs::read_to_string(set.path.join("manifest.json")).unwrap();
    for expected in [
        "\"kind\": \"manual\"".to_owned(),
        format!("\"instance_id\": \"{INSTANCE_ID}\""),
        "\"layout_version\": 1".to_owned(),
        "\"data_marker\": true".to_owned(),
        "\"name\": \"database.dump\"".to_owned(),
    ] {
        assert!(manifest.contains(&expected), "{expected} in {manifest}");
    }
    assert!(
        !manifest.contains("disposable"),
        "no password in the manifest"
    );

    // Restore into a new database; the connection settings travel in the environment only.
    let (restored_url, restored_name, _guard) = create_database().await;
    let mut restore = Command::new("pg_restore");
    restore
        .arg("--no-owner")
        .arg(format!("--dbname={restored_name}"))
        .arg(set.path.join("database.dump"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    for (name, value) in connection_environment(&admin_url()) {
        restore.env(name, value);
    }
    let output = restore.output().expect("pg_restore starts");

    let restored = connect(&restored_url).await;
    let version: String = sqlx::query_scalar("SHOW server_version_num")
        .fetch_one(restored.pool())
        .await
        .expect("the server version is readable");
    let server_major = version.trim().parse::<u32>().expect("a version number") / 10_000;
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        restore_outcome_is_acceptable(&output.status, &stderr, server_major),
        "pg_restore failed: {}\n{stderr}",
        output.status
    );
    let marker = fs::read_to_string(set.path.join(".impluvium-instance")).unwrap();
    let instance_id: String = sqlx::query_scalar("SELECT instance_id::text FROM instance_identity")
        .fetch_one(restored.pool())
        .await
        .expect("the identity was restored");
    assert_eq!(instance_id, marker);
    let public_key: String =
        sqlx::query_scalar("SELECT encode(public_key, 'hex') FROM content_key_binding")
            .fetch_one(restored.pool())
            .await
            .expect("the binding was restored");
    let key_id: String =
        sqlx::query_scalar("SELECT encode(key_id, 'hex') FROM content_key_binding")
            .fetch_one(restored.pool())
            .await
            .expect("the binding was restored");
    assert_eq!(public_key, CONTENT_PUBLIC_KEY);
    assert_eq!(key_id, CONTENT_KEY_ID);
    restored.close().await;
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires PostgreSQL (IMPLUVIUM_TEST_DATABASE_URL) and the PostgreSQL client tools"]
async fn four_pre_migration_backups_keep_the_three_newest_and_every_manual_set() {
    let instance = Instance::new().await;
    let url = DatabaseUrl::new(&instance.url);
    let manual = backup_manual(
        &instance
            .context(BackupKind::Manual, &url)
            .with(&instance, None),
    )
    .await
    .expect("the manual backup succeeds");
    let mut created = Vec::new();
    for number in 1..=4 {
        let owner = instance.context(
            pre(&format!("{number:04}"), &format!("{:04}", number + 1)),
            &url,
        );
        match backup_before_migration(&owner.with(&instance, None))
            .await
            .expect("the backup succeeds")
        {
            BackupOutcome::Created(set) => created.push(set),
            other => panic!("expected a set, got {other:?}"),
        }
    }
    assert_eq!(created[3].removed, [created[0].name.clone()]);
    let mut expected: Vec<String> = created[1..].iter().map(|set| set.name.clone()).collect();
    expected.push(manual.name);
    expected.sort();
    assert_eq!(entries(&instance.data.join("backups")), expected);
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires PostgreSQL (IMPLUVIUM_TEST_DATABASE_URL) and the PostgreSQL client tools"]
async fn a_missing_tool_fails_and_the_previous_set_survives() {
    let instance = Instance::new().await;
    let url = DatabaseUrl::new(&instance.url);
    let first = backup_manual(
        &instance
            .context(BackupKind::Manual, &url)
            .with(&instance, None),
    )
    .await
    .expect("the first backup succeeds");
    let before = entries(&first.path);

    let tools = ToolPaths {
        pg_dump: PathBuf::from("/nonexistent-impluvium-dir/pg_dump"),
        pg_restore: PathBuf::from("/nonexistent-impluvium-dir/pg_restore"),
    };
    let owner = instance.context(BackupKind::Manual, &url);
    let error = backup_manual(&owner.with(&instance, Some(tools)))
        .await
        .expect_err("a missing tool fails the backup");
    assert!(
        matches!(
            error,
            server_db::backup::BackupError::Tool {
                reason: ToolFailure::Missing,
                ..
            }
        ),
        "{error}"
    );
    assert_eq!(
        entries(&instance.data.join("backups")),
        [first.name.as_str()]
    );
    assert_eq!(entries(&first.path), before);
}
