// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Database backups: the standalone backup command and the backup that precedes a migration.
//!
//! # Sets
//!
//! A backup set is a directory in `<data>/backups/`, named `manual-<UTC time>` or
//! `pre-<from>-<to>-<UTC time>` (times as `yyyymmddThhmmssZ`; a second set in the same second
//! gets a `-<n>` suffix). It holds
//!
//! - `database.dump`: the `pg_dump --format=custom` archive,
//! - `instance.key`: a byte-for-byte copy of the instance key,
//! - `.impluvium-instance`: a copy of the data directory marker, when the data directory has one,
//! - `manifest.json`: kind, creation time, versions, file names and sizes and, when the database
//!   has one, the instance identity.
//!
//! Directories are private to the owner (mode 0700), files too (0600). A set is built in a hidden
//! `.partial-*` directory next to the final one. Every file is flushed, the manifest is written
//! last, and only then the directory is renamed. A directory without `manifest.json` and any
//! `.partial-*` directory is never a successful backup. Stale `.partial-*` directories are removed
//! at the start of the next backup. A failed run removes its own working directory and leaves every
//! earlier set untouched.
//!
//! One backup runs at a time: an exclusive lock on `backups/.lock` is held for the whole run,
//! including the cleanup of stale directories and retention. A second backup fails with
//! [`BackupError::AlreadyRunning`].
//!
//! After the final rename the directory of the backups is flushed. If that flush fails, the call
//! returns `Err` although the finished set exists: the migration must stay blocked, because the
//! backup could not be made durable.
//!
//! # Credentials
//!
//! The database URL is split into libpq environment variables (`PGHOST`, `PGUSER`, `PGPASSWORD`,
//! ...) that only the child process sees. The environment of the child is otherwise cleared except
//! for `PATH` and `LANG=C`. The password is never an argument, and no error or log text contains the
//! URL, a password or the output of a tool.
//!
//! # Contract with the migration guard
//!
//! [`backup_before_migration`] returns `Err` when no backup could be made in mode `auto` or
//! `command`. The startup guard must treat every `Err` as "do not migrate". [`BackupOutcome::Skipped`]
//! (mode `off`) means that the operator chose to migrate without a backup; the caller must log it
//! visibly. In mode `command` the operator program owns the backup: no set directory is written, and
//! a non-zero exit, a signal, a missing program or a timeout is an error.
//!
//! The work with child processes blocks the calling thread. Call the functions from a context that
//! may block (the startup path before the server accepts requests, or a dedicated runtime).

mod error;
mod files;
mod names;
mod process;
mod url;

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

pub use error::{BackupError, ToolFailure};
pub use names::BackupKind;

use crate::{Database, DatabaseUrl, DbError, PoolOptions};
use error::io as io_error;
use process::Deadline;

/// File name of the instance key, in the data directory and in a set.
pub const INSTANCE_KEY_FILE: &str = "instance.key";
/// File name of the data directory marker, in the data directory and in a set.
pub const DATA_MARKER_FILE: &str = ".impluvium-instance";
/// File name of the database dump in a set.
pub const DUMP_FILE: &str = "database.dump";
/// File name of the manifest in a set.
pub const MANIFEST_FILE: &str = files::MANIFEST;

/// The marker is opaque here and must stay small.
const MAX_MARKER_BYTES: u64 = 64 * 1024;
/// Longest accepted time limit.
const MAX_TIMEOUT: Duration = Duration::from_secs(24 * 3600);
/// Longest wait for the connection and the queries that read the server facts.
const PROBE_LIMIT: Duration = Duration::from_secs(30);

/// How a backup is made.
#[derive(Clone, Eq, PartialEq)]
pub enum BackupMode {
    /// The built-in `pg_dump` backup.
    Auto,
    /// The operator's program and arguments, started without a shell.
    Command(Vec<String>),
    /// No backup.
    Off,
}

impl std::fmt::Debug for BackupMode {
    /// Shows the number of arguments of an operator command, never the arguments: they may
    /// carry secrets.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Auto => f.write_str("Auto"),
            Self::Command(arguments) => write!(f, "Command(<{} arguments>)", arguments.len()),
            Self::Off => f.write_str("Off"),
        }
    }
}

/// Explicit locations of the PostgreSQL client tools. Without them they are looked up in `PATH`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolPaths {
    pub pg_dump: PathBuf,
    pub pg_restore: PathBuf,
}

/// Everything a backup needs. Fields are checked when the backup starts.
pub struct BackupContext<'a> {
    /// The database to dump. It carries the password and is never shown.
    pub database_url: &'a DatabaseUrl,
    /// The data directory; sets go to `<data_dir>/backups`.
    pub data_dir: &'a Path,
    /// The validated bytes of the instance key, to be copied into the set.
    pub instance_key: &'a [u8; 32],
    pub kind: BackupKind,
    pub mode: BackupMode,
    /// Time limit for the whole backup, from one second to 24 hours; programs still running at
    /// the end are killed.
    pub timeout: Duration,
    /// Where the tools are, or `None` to search `PATH`.
    pub tools: Option<ToolPaths>,
}

/// A finished set.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BackupSet {
    /// Directory name, for example `manual-20261119T120000Z`.
    pub name: String,
    /// Full path of the set.
    pub path: PathBuf,
    /// Older pre-migration sets that retention removed after this one succeeded.
    pub removed: Vec<String>,
    /// Removing an old set failed or the directory could not be read. The new set is complete anyway.
    pub retention_incomplete: bool,
}

/// What [`backup_before_migration`] did.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BackupOutcome {
    /// Mode `off`: nothing was backed up. The caller must log this.
    Skipped,
    /// Mode `command`: the operator program finished successfully.
    Operator,
    /// Mode `auto`: a set was written.
    Created(BackupSet),
}

impl BackupContext<'_> {
    fn validate(&self) -> Result<(), BackupError> {
        if self.timeout.is_zero() || self.timeout > MAX_TIMEOUT {
            return Err(BackupError::InvalidInput(
                "the backup time limit must be between one second and 24 hours",
            ));
        }
        if let BackupMode::Command(arguments) = &self.mode
            && (arguments.is_empty() || arguments.iter().any(|argument| argument.contains('\0')))
        {
            return Err(BackupError::InvalidInput(
                "the operator backup command must be a non-empty list of arguments",
            ));
        }
        Ok(())
    }
}

/// What is needed from the database server besides the dump.
struct ServerFacts {
    /// `server_version_num`, for example 180007.
    server_version_num: u32,
    /// Instance identifier and layout version, when the identity row exists.
    instance: Option<(String, i32)>,
}

/// Checked inputs of a dump run.
struct Prepared {
    environment: Vec<(&'static str, String)>,
    pg_dump: PathBuf,
    pg_restore: PathBuf,
}

fn tool_path(name: &'static str) -> Result<PathBuf, BackupError> {
    process::find_in_path(name).ok_or(BackupError::Tool {
        tool: name,
        reason: ToolFailure::Missing,
    })
}

/// Variables through which libpq (and sqlx) fill in connection parts that the URL leaves out:
/// the part of the URL and the process variables that would supply it.
const AMBIENT: [(&str, &[&str]); 4] = [
    ("PGHOST", &["PGHOST", "PGHOSTADDR"]),
    ("PGPORT", &["PGPORT"]),
    ("PGUSER", &["PGUSER"]),
    ("PGDATABASE", &["PGDATABASE"]),
];

/// The server connects with the process environment filling in what the URL omits, but `pg_dump`
/// runs with a cleared environment and would reach a different database or none. Refuse that
/// instead of guessing.
fn refuse_ambient(
    environment: &[(&'static str, String)],
    lookup: &dyn Fn(&str) -> Option<OsString>,
) -> Result<(), BackupError> {
    for (part, variables) in AMBIENT {
        let in_url = environment.iter().any(|(name, _)| *name == part);
        if !in_url && variables.iter().any(|name| lookup(name).is_some()) {
            return Err(BackupError::Url(
                "connection settings come from PG* environment variables; \
                 put host, port, user and database into the database URL",
            ));
        }
    }
    Ok(())
}

fn prepare(context: &BackupContext<'_>) -> Result<Prepared, BackupError> {
    prepare_with(context, &ambient_variable)
}

/// Reads one of the libpq variables of [`AMBIENT`] from the process environment.
fn ambient_variable(name: &str) -> Option<OsString> {
    match name {
        "PGHOST" => std::env::var_os("PGHOST"),
        "PGHOSTADDR" => std::env::var_os("PGHOSTADDR"),
        "PGPORT" => std::env::var_os("PGPORT"),
        "PGUSER" => std::env::var_os("PGUSER"),
        "PGDATABASE" => std::env::var_os("PGDATABASE"),
        _ => None,
    }
}

fn prepare_with(
    context: &BackupContext<'_>,
    lookup: &dyn Fn(&str) -> Option<OsString>,
) -> Result<Prepared, BackupError> {
    context.validate()?;
    let environment = url::libpq_environment(context.database_url.expose())?;
    refuse_ambient(&environment, lookup)?;
    let (pg_dump, pg_restore) = match &context.tools {
        Some(tools) => (tools.pg_dump.clone(), tools.pg_restore.clone()),
        None => (tool_path("pg_dump")?, tool_path("pg_restore")?),
    };
    Ok(Prepared {
        environment,
        pg_dump,
        pg_restore,
    })
}

/// Adds the connection settings to a command. The version probes get them without the password:
/// a client wrapper may pick the binary by host and port.
fn with_connection(
    mut command: std::process::Command,
    prepared: &Prepared,
    password: bool,
) -> std::process::Command {
    for (name, value) in &prepared.environment {
        if password || *name != "PGPASSWORD" {
            command.env(name, value);
        }
    }
    command
}

/// Makes a manual backup with the built-in `pg_dump`, whatever `context.mode` says. The kind must
/// be [`BackupKind::Manual`]. Blocks the calling thread while `pg_dump` runs.
pub async fn backup_manual(context: &BackupContext<'_>) -> Result<BackupSet, BackupError> {
    if context.kind != BackupKind::Manual {
        return Err(BackupError::InvalidInput(
            "a manual backup needs the manual kind",
        ));
    }
    run_auto(context).await
}

/// Makes the backup that must precede a migration, according to `context.mode`. The kind must be
/// [`BackupKind::PreMigration`]. See the module documentation for the contract: every `Err` must
/// stop the migration. Blocks the calling thread while a program runs.
pub async fn backup_before_migration(
    context: &BackupContext<'_>,
) -> Result<BackupOutcome, BackupError> {
    if !matches!(context.kind, BackupKind::PreMigration { .. }) {
        return Err(BackupError::InvalidInput(
            "a backup before a migration needs the pre-migration kind",
        ));
    }
    context.validate()?;
    match &context.mode {
        BackupMode::Off => Ok(BackupOutcome::Skipped),
        BackupMode::Command(arguments) => {
            run_operator(arguments, context.timeout)?;
            Ok(BackupOutcome::Operator)
        }
        BackupMode::Auto => run_auto(context).await.map(BackupOutcome::Created),
    }
}

async fn run_auto(context: &BackupContext<'_>) -> Result<BackupSet, BackupError> {
    let prepared = prepare(context)?;
    let facts = read_facts(context).await?;
    create_set(context, &prepared, &facts)
}

/// Reads the server version and the instance identity, then closes the connection again.
async fn read_facts(context: &BackupContext<'_>) -> Result<ServerFacts, BackupError> {
    let limit = context.timeout.min(PROBE_LIMIT);
    let options = PoolOptions {
        max_connections: 1,
        min_connections: 0,
        acquire_timeout: limit,
        statement_timeout: limit,
        application_name: "impluvium-backup".to_owned(),
    };
    let database = Database::connect(context.database_url, &options).await?;
    let facts = query_facts(&database).await;
    database.close().await;
    facts
}

async fn query_facts(database: &Database) -> Result<ServerFacts, BackupError> {
    let query_failed = |error: sqlx::Error| BackupError::Database(DbError::from_query(&error));
    let text = sqlx::query_scalar::<_, String>("SHOW server_version_num")
        .fetch_one(database.pool())
        .await
        .map_err(query_failed)?;
    let server_version_num = match text.trim().parse::<u32>() {
        Ok(number) if number >= 100_000 => number,
        _ => return Err(BackupError::Database(DbError::Query)),
    };
    let has_identity =
        sqlx::query_scalar::<_, bool>("SELECT to_regclass('public.instance_identity') IS NOT NULL")
            .fetch_one(database.pool())
            .await
            .map_err(query_failed)?;
    let instance = if has_identity {
        database
            .instance_identity()
            .await?
            .map(|identity| (identity.instance_id, identity.layout_version))
    } else {
        None
    };
    Ok(ServerFacts {
        server_version_num,
        instance,
    })
}

fn run_operator(arguments: &[String], timeout: Duration) -> Result<(), BackupError> {
    let Some((program, rest)) = arguments.split_first() else {
        return Err(BackupError::InvalidInput(
            "the operator backup command must be a non-empty list of arguments",
        ));
    };
    let mut command = std::process::Command::new(program);
    command.args(rest).stdin(std::process::Stdio::null());
    process::run(command, &Deadline::after(timeout)).map_err(BackupError::Operator)
}

fn tool_failure(tool: &'static str) -> impl FnOnce(ToolFailure) -> BackupError {
    move |reason| BackupError::Tool { tool, reason }
}

/// Builds, checks and publishes one set. Blocks while the tools run.
fn create_set(
    context: &BackupContext<'_>,
    prepared: &Prepared,
    facts: &ServerFacts,
) -> Result<BackupSet, BackupError> {
    let backups = files::prepare_backups_dir(context.data_dir)?;
    let _lock = files::acquire_lock(&backups)?;
    files::remove_stale_partials(&backups);
    let now = SystemTime::now();
    let stamp = names::compact_stamp(now);
    let pre_migration = matches!(context.kind, BackupKind::PreMigration { .. });
    let base = names::base_name(&context.kind, &stamp);
    let name = files::unique_name(&backups, &base, &stamp, pre_migration)?;
    let partial = files::create_partial(&backups, &name)?;
    let mut guard = files::PartialGuard::new(partial.clone());
    let deadline = Deadline::after(context.timeout);

    // The tool must be at least as new as the server, or the dump may be unusable.
    let mut probe = with_connection(process::tool_command(&prepared.pg_dump), prepared, false);
    probe.arg("--version");
    let version_text = process::run_capture(probe, &partial.join(".version"), &deadline)
        .map_err(tool_failure("pg_dump"))?;
    let tool_major = process::parse_major(&version_text).ok_or(BackupError::Tool {
        tool: "pg_dump",
        reason: ToolFailure::UnexpectedOutput,
    })?;
    let server_major = facts.server_version_num / 10_000;
    if tool_major < server_major {
        return Err(BackupError::ToolTooOld {
            tool_major,
            server_major,
        });
    }

    let dump_path = partial.join(DUMP_FILE);
    let mut dump = with_connection(process::tool_command(&prepared.pg_dump), prepared, true);
    dump.arg("--format=custom").arg("--no-password");
    let mut file_argument = OsString::from("--file=");
    file_argument.push(&dump_path);
    dump.arg(file_argument);
    process::run(dump, &deadline).map_err(tool_failure("pg_dump"))?;
    let dump_size = match std::fs::metadata(&dump_path) {
        Ok(meta) => meta.len(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => 0,
        Err(error) => return Err(io_error("inspect the dump")(error)),
    };
    if dump_size == 0 {
        return Err(BackupError::EmptyDump);
    }
    files::seal_file(&dump_path).map_err(io_error("flush the dump"))?;

    // `pg_restore --list` reads only the table of contents of the archive. It catches a dump that
    // is cut off in or before the table of contents or is not an archive at all, not damage in the
    // table data; `pg_dump` itself exiting with status 0 is what vouches for the data.
    let mut list = with_connection(process::tool_command(&prepared.pg_restore), prepared, false);
    list.arg("--list").arg(&dump_path);
    match process::run(list, &deadline) {
        Ok(()) => {}
        Err(ToolFailure::Exit(_)) => return Err(BackupError::InvalidDump),
        Err(reason) => {
            return Err(BackupError::Tool {
                tool: "pg_restore",
                reason,
            });
        }
    }

    let marker = files::read_optional_small_file(
        &context.data_dir.join(DATA_MARKER_FILE),
        MAX_MARKER_BYTES,
        "the data directory marker must be a small regular file",
    )?;
    files::write_new_file(&partial.join(INSTANCE_KEY_FILE), context.instance_key)
        .map_err(io_error("copy the instance key"))?;
    let mut listed: Vec<(&str, u64)> = vec![
        (DUMP_FILE, dump_size),
        (INSTANCE_KEY_FILE, context.instance_key.len() as u64),
    ];
    if let Some(bytes) = &marker {
        files::write_new_file(&partial.join(DATA_MARKER_FILE), bytes)
            .map_err(io_error("copy the data directory marker"))?;
        listed.push((DATA_MARKER_FILE, bytes.len() as u64));
    }

    let manifest = render_manifest(
        context,
        facts,
        &names::iso_stamp(now),
        &process::version_line(&version_text),
        &listed,
        marker.is_some(),
    );
    files::write_new_file(&partial.join(MANIFEST_FILE), manifest.as_bytes())
        .map_err(io_error("write the manifest"))?;
    files::sync_dir(&partial).map_err(io_error("flush the working directory"))?;

    let final_path = backups.join(&name);
    std::fs::rename(&partial, &final_path).map_err(io_error("publish the set"))?;
    guard.release();
    // The set exists from here on. A failing flush still returns an error, so that the migration
    // stays blocked.
    files::sync_dir(&backups).map_err(io_error("flush the backups directory"))?;

    let (removed, retention_incomplete) = if pre_migration {
        files::apply_retention(&backups, &name)
    } else {
        (Vec::new(), false)
    };
    Ok(BackupSet {
        name,
        path: final_path,
        removed,
        retention_incomplete,
    })
}

fn render_manifest(
    context: &BackupContext<'_>,
    facts: &ServerFacts,
    created: &str,
    dump_version: &str,
    listed: &[(&str, u64)],
    marker: bool,
) -> String {
    use names::json_string;
    let mut text = String::from("{\n  \"format\": 1,\n");
    match &context.kind {
        BackupKind::Manual => text.push_str("  \"kind\": \"manual\",\n"),
        BackupKind::PreMigration { from, to } => {
            text.push_str("  \"kind\": \"pre-migration\",\n");
            text.push_str(&format!("  \"from\": {},\n", json_string(from)));
            text.push_str(&format!("  \"to\": {},\n", json_string(to)));
        }
    }
    text.push_str(&format!("  \"created_utc\": {},\n", json_string(created)));
    text.push_str(&format!(
        "  \"postgres_server_version_num\": {},\n",
        facts.server_version_num
    ));
    text.push_str(&format!(
        "  \"pg_dump_version\": {},\n",
        json_string(dump_version)
    ));
    if let Some((instance_id, layout_version)) = &facts.instance {
        text.push_str(&format!(
            "  \"instance_id\": {},\n  \"layout_version\": {layout_version},\n",
            json_string(instance_id)
        ));
    }
    text.push_str(&format!("  \"data_marker\": {marker},\n  \"files\": [\n"));
    for (index, (name, size)) in listed.iter().enumerate() {
        let separator = if index + 1 == listed.len() { "" } else { "," };
        text.push_str(&format!(
            "    {{\"name\": {}, \"size\": {size}}}{separator}\n",
            json_string(name)
        ));
    }
    text.push_str("  ]\n}\n");
    text
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::fs;
    use std::io::{ErrorKind, Write};
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::time::Instant;

    const PASSWORD: &str = "pwmarker";
    const KEY: [u8; 32] = [7; 32];
    const INSTANCE_ID: &str = "0b3c1e9a-0000-4000-8000-000000000001";
    const DUMP_OK: &str = "printf 'PGDMP-data' > \"$out\"";

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    /// A scratch directory with a data directory and a directory for fake tools.
    /// Serialises the tests that start child processes. A child forked by one test
    /// thread briefly shares the open files of the others (until it executes its
    /// program), which can make another test's backup lock look held.
    static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

    struct Env {
        root: PathBuf,
        data: PathBuf,
        record: PathBuf,
        url: DatabaseUrl,
        _serial: std::sync::MutexGuard<'static, ()>,
    }

    impl Env {
        fn new() -> Self {
            let serial = SERIAL
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let number = COUNTER.fetch_add(1, Ordering::SeqCst);
            let nanos = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .map_or(0, |elapsed| elapsed.as_nanos());
            let root = std::env::temp_dir().join(format!(
                "impluvium-backup-test-{}-{nanos:x}-{number}",
                std::process::id()
            ));
            let data = root.join("data");
            fs::create_dir_all(&data).unwrap();
            fs::create_dir_all(root.join("tools")).unwrap();
            Self {
                record: root.join("record.txt"),
                url: DatabaseUrl::new(format!(
                    "postgres://backup%40user:{PASSWORD}@db.example.test:5433/appdb?sslmode=require"
                )),
                root,
                data,
                _serial: serial,
            }
        }

        fn backups(&self) -> PathBuf {
            self.data.join("backups")
        }

        /// Writes an executable script and returns its path.
        fn script(&self, body: &str) -> PathBuf {
            let number = COUNTER.fetch_add(1, Ordering::SeqCst);
            let path = self.root.join("tools").join(format!("tool-{number}"));
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o755)
                .open(&path)
                .unwrap();
            file.write_all(format!("#!/bin/sh\n{body}\n").as_bytes())
                .unwrap();
            file.sync_all().unwrap();
            path
        }

        /// A fake `pg_dump` that reports `version`, records its arguments and environment and then
        /// runs `action` with the target file in `$out`.
        fn tools(&self, version: &str, action: &str, restore_status: i32) -> ToolPaths {
            let dump = format!(
                "case \"$1\" in --version) env | sort > '{record}.version'; echo \"pg_dump (PostgreSQL) {version}\"; exit 0;; esac\n\
                 for a in \"$@\"; do case \"$a\" in --file=*) out=\"${{a#--file=}}\";; esac; done\n\
                 {{ for a in \"$@\"; do echo \"ARG:$a\"; done; env | sort; }} > '{record}'\n{action}",
                record = self.record.display()
            );
            ToolPaths {
                pg_dump: self.script(&dump),
                pg_restore: self.script(&format!(
                    "case \"$1\" in --list) exit {restore_status};; esac\nexit 2"
                )),
            }
        }

        fn context(&self, kind: BackupKind, tools: ToolPaths) -> BackupContext<'_> {
            BackupContext {
                database_url: &self.url,
                data_dir: &self.data,
                instance_key: &KEY,
                kind,
                mode: BackupMode::Auto,
                timeout: Duration::from_secs(20),
                tools: Some(tools),
            }
        }
    }

    impl Drop for Env {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn facts(version: u32) -> ServerFacts {
        ServerFacts {
            server_version_num: version,
            instance: Some((INSTANCE_ID.to_owned(), 1)),
        }
    }

    fn make(context: &BackupContext<'_>, facts: &ServerFacts) -> Result<BackupSet, BackupError> {
        let prepared = prepare_with(context, &|_| None)?;
        create_set(context, &prepared, facts)
    }

    fn entries(directory: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(directory)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .filter(|name| name != files::LOCK_FILE)
            .collect();
        names.sort();
        names
    }

    fn mode(path: &Path) -> u32 {
        fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    fn pre(from: &str, to: &str) -> BackupKind {
        BackupKind::PreMigration {
            from: from.to_owned(),
            to: to.to_owned(),
        }
    }

    #[test]
    fn a_manual_set_is_complete_private_and_free_of_secrets_in_arguments() {
        let env = Env::new();
        fs::write(env.data.join(DATA_MARKER_FILE), b"marker-bytes\n").unwrap();
        let tools = env.tools("18.7", DUMP_OK, 0);
        let context = env.context(BackupKind::Manual, tools);
        // pg_dump 18 against a server 16 is fine.
        let set = make(&context, &facts(160_004)).unwrap();
        assert!(set.name.starts_with("manual-") && set.name.ends_with('Z'));
        assert!(set.removed.is_empty() && !set.retention_incomplete);
        assert_eq!(set.path, env.backups().join(&set.name));
        assert_eq!(entries(&env.backups()), [set.name.as_str()]);
        assert_eq!(
            entries(&set.path),
            [
                ".impluvium-instance",
                "database.dump",
                "instance.key",
                "manifest.json"
            ]
        );
        assert_eq!(mode(&env.backups()), 0o700);
        assert_eq!(mode(&set.path), 0o700);
        for file in entries(&set.path) {
            assert_eq!(mode(&set.path.join(&file)), 0o600, "{file}");
        }
        assert_eq!(fs::read(set.path.join("instance.key")).unwrap(), KEY);
        assert_eq!(
            fs::read(set.path.join(".impluvium-instance")).unwrap(),
            b"marker-bytes\n"
        );
        assert_eq!(
            fs::read(set.path.join("database.dump")).unwrap(),
            b"PGDMP-data"
        );

        let manifest = fs::read_to_string(set.path.join("manifest.json")).unwrap();
        let identity = format!("\"instance_id\": \"{INSTANCE_ID}\"");
        for expected in [
            "\"kind\": \"manual\"",
            identity.as_str(),
            "\"layout_version\": 1",
            "\"postgres_server_version_num\": 160004",
            "\"pg_dump_version\": \"pg_dump (PostgreSQL) 18.7\"",
            "\"data_marker\": true",
            "{\"name\": \"database.dump\", \"size\": 10}",
            "{\"name\": \"instance.key\", \"size\": 32}",
            "{\"name\": \".impluvium-instance\", \"size\": 13}",
        ] {
            assert!(manifest.contains(expected), "{expected} in {manifest}");
        }
        assert!(!manifest.contains(PASSWORD));

        // The version probe sees the connection settings, but not the password.
        let probe = fs::read_to_string(format!("{}.version", env.record.display())).unwrap();
        assert!(probe.contains("PGHOST=db.example.test") && probe.contains("PGPORT=5433"));
        assert!(!probe.contains("PGPASSWORD") && !probe.contains(PASSWORD));

        let record = fs::read_to_string(&env.record).unwrap();
        let (arguments, environment): (Vec<&str>, Vec<&str>) =
            record.lines().partition(|line| line.starts_with("ARG:"));
        assert!(arguments.contains(&"ARG:--format=custom"));
        assert!(arguments.contains(&"ARG:--no-password"));
        assert!(arguments.iter().any(|line| line.starts_with("ARG:--file=")));
        let argument_text = arguments.join("\n");
        for secret in [
            PASSWORD,
            "backup@user",
            "backup%40user",
            "db.example",
            "appdb",
        ] {
            assert!(!argument_text.contains(secret), "{secret} in the arguments");
        }
        for expected in [
            "PGPASSWORD=pwmarker",
            "PGUSER=backup@user",
            "PGHOST=db.example.test",
            "PGPORT=5433",
            "PGDATABASE=appdb",
            "PGSSLMODE=require",
            "LANG=C",
        ] {
            assert!(environment.contains(&expected), "{expected}");
        }
        let allowed = [
            "PGPASSWORD",
            "PGUSER",
            "PGHOST",
            "PGPORT",
            "PGDATABASE",
            "PGSSLMODE",
            "LANG",
            "PATH",
            "PWD",
            "OLDPWD",
            "SHLVL",
            "_",
        ];
        for line in &environment {
            let name = line.split('=').next().unwrap();
            assert!(allowed.contains(&name), "unexpected variable {name}");
        }
    }

    #[test]
    fn without_identity_or_marker_the_manifest_says_so() {
        let env = Env::new();
        let tools = env.tools("18.7", DUMP_OK, 0);
        let context = env.context(pre("0001", "0002"), tools);
        let facts = ServerFacts {
            server_version_num: 180_007,
            instance: None,
        };
        let set = make(&context, &facts).unwrap();
        assert!(set.name.starts_with("pre-0001-0002-"));
        assert_eq!(
            entries(&set.path),
            ["database.dump", "instance.key", "manifest.json"]
        );
        let manifest = fs::read_to_string(set.path.join("manifest.json")).unwrap();
        assert!(manifest.contains("\"kind\": \"pre-migration\""));
        assert!(manifest.contains("\"from\": \"0001\"") && manifest.contains("\"to\": \"0002\""));
        assert!(manifest.contains("\"data_marker\": false"));
        assert!(!manifest.contains("instance_id"));
    }

    #[test]
    fn failures_leave_no_set_no_partial_and_earlier_sets_intact() {
        let env = Env::new();
        let good = env.tools("18.7", DUMP_OK, 0);
        let first = make(&env.context(BackupKind::Manual, good), &facts(180_007)).unwrap();
        let before = entries(&first.path);
        let cases: Vec<(&str, ToolPaths, &str)> = vec![
            (
                "dump exits 1",
                env.tools("18.7", "exit 1", 0),
                "exited with status 1",
            ),
            ("empty dump", env.tools("18.7", ": > \"$out\"", 0), "empty"),
            ("no file", env.tools("18.7", "exit 0", 0), "empty"),
            (
                "dump not readable",
                env.tools("18.7", DUMP_OK, 1),
                "pg_restore --list",
            ),
            (
                "garbled version",
                env.tools("x", DUMP_OK, 0),
                "not understood",
            ),
        ];
        for (label, tools, expected) in cases {
            let error =
                make(&env.context(BackupKind::Manual, tools), &facts(180_007)).expect_err(label);
            let text = error.to_string();
            assert!(text.contains(expected), "{label}: {text}");
            assert!(
                !text.contains(PASSWORD) && !text.contains("db.example"),
                "{label}"
            );
            assert_eq!(entries(&env.backups()), [first.name.as_str()], "{label}");
            assert_eq!(entries(&first.path), before, "{label}");
        }
    }

    #[test]
    fn a_missing_tool_fails() {
        let env = Env::new();
        let working = env.tools("18.7", DUMP_OK, 0);
        let missing = Path::new("/nonexistent-impluvium-dir/tool").to_path_buf();
        let no_dump = ToolPaths {
            pg_dump: missing.clone(),
            pg_restore: working.pg_restore.clone(),
        };
        let error = make(&env.context(BackupKind::Manual, no_dump), &facts(180_007)).unwrap_err();
        assert!(matches!(
            error,
            BackupError::Tool {
                tool: "pg_dump",
                reason: ToolFailure::Missing
            }
        ));
        let no_restore = ToolPaths {
            pg_dump: working.pg_dump,
            pg_restore: missing,
        };
        let error = make(
            &env.context(BackupKind::Manual, no_restore),
            &facts(180_007),
        )
        .unwrap_err();
        assert!(matches!(
            error,
            BackupError::Tool {
                tool: "pg_restore",
                reason: ToolFailure::Missing
            }
        ));
        assert!(entries(&env.backups()).is_empty());
    }

    #[test]
    fn a_tool_older_than_the_server_fails() {
        let env = Env::new();
        let tools = env.tools("16.3", DUMP_OK, 0);
        let error = make(&env.context(BackupKind::Manual, tools), &facts(180_007)).unwrap_err();
        assert!(matches!(
            error,
            BackupError::ToolTooOld {
                tool_major: 16,
                server_major: 18
            }
        ));
        assert!(entries(&env.backups()).is_empty());
    }

    #[test]
    fn a_slow_tool_is_killed_at_the_time_limit() {
        let env = Env::new();
        let tools = env.tools("18.7", "exec sleep 30", 0);
        let mut context = env.context(BackupKind::Manual, tools);
        context.timeout = Duration::from_secs(2);
        let started = Instant::now();
        let error = make(&context, &facts(180_007)).unwrap_err();
        assert!(matches!(
            error,
            BackupError::Tool {
                tool: "pg_dump",
                reason: ToolFailure::Timeout
            }
        ));
        assert!(started.elapsed() < Duration::from_secs(20));
        assert!(entries(&env.backups()).is_empty());
    }

    #[test]
    fn io_failures_clean_up_and_keep_earlier_sets() {
        let env = Env::new();
        fs::write(env.data.join(DATA_MARKER_FILE), b"m").unwrap();
        let good = env.tools("18.7", DUMP_OK, 0);
        let first = make(&env.context(BackupKind::Manual, good), &facts(180_007)).unwrap();
        let before = entries(&first.path);
        // Writes of a set: the key (1), the marker (2) and the manifest (3).
        for kind in [ErrorKind::StorageFull, ErrorKind::PermissionDenied] {
            for number in 1..=3 {
                let tools = env.tools("18.7", DUMP_OK, 0);
                files::test_hook::fail_write(number, kind);
                let result = make(&env.context(BackupKind::Manual, tools), &facts(180_007));
                files::test_hook::clear();
                match result {
                    Err(BackupError::Io { kind: found, step }) => {
                        assert_eq!(found, kind, "write {number}");
                        assert!(!step.contains('/'));
                    }
                    other => panic!("write {number}: expected an I/O error, got {other:?}"),
                }
                assert_eq!(
                    entries(&env.backups()),
                    [first.name.as_str()],
                    "write {number}"
                );
                assert_eq!(entries(&first.path), before);
            }
        }
    }

    #[test]
    fn retention_keeps_three_pre_migration_sets_and_every_manual_one() {
        let env = Env::new();
        let mut manual = Vec::new();
        for _ in 0..2 {
            let tools = env.tools("18.7", DUMP_OK, 0);
            manual.push(
                make(&env.context(BackupKind::Manual, tools), &facts(180_007))
                    .unwrap()
                    .name,
            );
        }
        let mut created: Vec<String> = Vec::new();
        for number in 0..5 {
            let tools = env.tools("18.7", DUMP_OK, 0);
            let kind = pre(&format!("{number:04}"), &format!("{:04}", number + 1));
            let set = make(&env.context(kind, tools), &facts(180_007)).unwrap();
            let expected_removed: Vec<String> = if number >= 3 {
                vec![created[number - 3].clone()]
            } else {
                Vec::new()
            };
            assert_eq!(set.removed, expected_removed, "run {number}");
            assert!(!set.retention_incomplete);
            created.push(set.name);
        }
        let mut expected: Vec<String> = created[2..].to_vec();
        expected.extend(manual);
        expected.sort();
        assert_eq!(entries(&env.backups()), expected);
    }

    #[test]
    fn stale_partials_are_removed_and_incomplete_sets_never_count() {
        let env = Env::new();
        let backups = env.backups();
        fs::create_dir_all(backups.join(".partial-old-1")).unwrap();
        fs::write(backups.join(".partial-old-1").join("database.dump"), b"x").unwrap();
        fs::create_dir_all(backups.join("pre-a-b-20200101T000000Z")).unwrap();
        for number in 0..4 {
            let tools = env.tools("18.7", DUMP_OK, 0);
            let kind = pre("a", &number.to_string());
            make(&env.context(kind, tools), &facts(180_007)).unwrap();
        }
        let found = entries(&backups);
        assert!(
            found.iter().all(|name| !name.starts_with(".partial-")),
            "{found:?}"
        );
        // The directory without a manifest is neither counted nor removed.
        assert!(found.contains(&"pre-a-b-20200101T000000Z".to_owned()));
        assert_eq!(found.len(), 4);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn command_mode_starts_the_program_without_a_shell() {
        let env = Env::new();
        let program = env.script(&format!(
            "printf '%s|%s' \"$1\" \"$2\" > '{}'",
            env.record.display()
        ));
        let literal = "$(touch impluvium-injection-x); `touch impluvium-injection-y`; a b";
        let tools = env.tools("18.7", DUMP_OK, 0);
        let mut context = env.context(pre("1", "2"), tools);
        context.mode = BackupMode::Command(vec![
            program.display().to_string(),
            literal.to_owned(),
            "second".to_owned(),
        ]);
        let outcome = backup_before_migration(&context).await.unwrap();
        assert_eq!(outcome, BackupOutcome::Operator);
        assert_eq!(
            fs::read_to_string(&env.record).unwrap(),
            format!("{literal}|second")
        );
        assert!(!Path::new("impluvium-injection-x").exists());
        assert!(!Path::new("impluvium-injection-y").exists());
        // The operator owns the backup: nothing is written to the data directory.
        assert!(!env.backups().exists());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn command_mode_reports_every_kind_of_failure() {
        let env = Env::new();
        let tools = env.tools("18.7", DUMP_OK, 0);
        let mut context = env.context(pre("1", "2"), tools);
        context.timeout = Duration::from_secs(1);
        let cases = [
            (env.script("exit 3"), ToolFailure::Exit(3)),
            (env.script("kill -9 $$"), ToolFailure::Signal),
            (env.script("exec sleep 30"), ToolFailure::Timeout),
            (
                PathBuf::from("/nonexistent-impluvium-dir/backup"),
                ToolFailure::Missing,
            ),
        ];
        for (program, expected) in cases {
            context.mode = BackupMode::Command(vec![program.display().to_string()]);
            match backup_before_migration(&context).await {
                Err(BackupError::Operator(found)) => assert_eq!(found, expected),
                other => panic!("expected {expected:?}, got {other:?}"),
            }
        }
        assert!(!env.backups().exists());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn mode_off_skips_without_touching_anything() {
        let env = Env::new();
        let tools = env.tools("18.7", DUMP_OK, 0);
        let mut context = env.context(pre("1", "2"), tools);
        context.mode = BackupMode::Off;
        assert_eq!(
            backup_before_migration(&context).await.unwrap(),
            BackupOutcome::Skipped
        );
        assert!(!env.backups().exists());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn unusable_contexts_are_rejected() {
        let env = Env::new();
        let tools = env.tools("18.7", DUMP_OK, 0);
        let mut context = env.context(pre("1", "2"), tools.clone());
        context.timeout = Duration::ZERO;
        assert!(matches!(
            backup_before_migration(&context).await,
            Err(BackupError::InvalidInput(_))
        ));
        context.timeout = Duration::from_secs(5);
        context.mode = BackupMode::Command(Vec::new());
        assert!(matches!(
            backup_before_migration(&context).await,
            Err(BackupError::InvalidInput(_))
        ));
        // The kind must fit the entry point.
        context.mode = BackupMode::Off;
        assert!(matches!(
            backup_manual(&context).await,
            Err(BackupError::InvalidInput(_))
        ));
        let manual = env.context(BackupKind::Manual, tools.clone());
        assert!(matches!(
            backup_before_migration(&manual).await,
            Err(BackupError::InvalidInput(_))
        ));
        // A data directory that is missing fails before anything is started.
        let missing = PathBuf::from("/nonexistent-impluvium-dir/data");
        let mut context = env.context(BackupKind::Manual, tools);
        context.data_dir = &missing;
        assert!(matches!(
            make(&context, &facts(180_007)),
            Err(BackupError::Io { .. })
        ));
    }

    #[test]
    fn retention_never_removes_the_new_set_even_when_the_clock_is_behind() {
        let env = Env::new();
        let backups = env.backups();
        for day in 1..=3 {
            let old = backups.join(format!("pre-a-b-2099010{day}T000000Z"));
            fs::create_dir_all(&old).unwrap();
            fs::write(old.join("manifest.json"), b"{}").unwrap();
        }
        let tools = env.tools("18.7", DUMP_OK, 0);
        let set = make(&env.context(pre("c", "d"), tools), &facts(180_007)).unwrap();
        assert_eq!(set.removed, ["pre-a-b-20990101T000000Z"]);
        assert!(!set.removed.contains(&set.name));
        let mut expected = vec![
            "pre-a-b-20990102T000000Z".to_owned(),
            "pre-a-b-20990103T000000Z".to_owned(),
            set.name.clone(),
        ];
        expected.sort();
        assert_eq!(entries(&backups), expected);
    }

    #[test]
    fn connection_settings_from_the_environment_are_refused() {
        let only =
            |set: &'static str| move |name: &str| (name == set).then(|| OsString::from("value"));
        let cases = [
            ("postgres://u:p@:5432/d", "PGHOST"),
            ("postgres://u:p@:5432/d", "PGHOSTADDR"),
            ("postgres://u:p@h/d", "PGPORT"),
            ("postgres://h:5432/d", "PGUSER"),
            ("postgres://u:p@h:5432", "PGDATABASE"),
        ];
        for (url, variable) in cases {
            let environment = url::libpq_environment(url).unwrap();
            let lookup = only(variable);
            assert!(
                matches!(
                    refuse_ambient(&environment, &lookup),
                    Err(BackupError::Url(_))
                ),
                "{variable}"
            );
            // Nothing set: accepted.
            assert!(
                refuse_ambient(&environment, &|_| None).is_ok(),
                "{variable}"
            );
        }
        // A URL with every part is accepted whatever the environment holds.
        let environment = url::libpq_environment("postgres://u:p@h:5432/d").unwrap();
        let everything = |_: &str| Some(OsString::from("value"));
        assert!(refuse_ambient(&environment, &everything).is_ok());
    }

    #[test]
    fn a_second_backup_is_refused_while_the_lock_is_held() {
        let env = Env::new();
        let tools = env.tools("18.7", DUMP_OK, 0);
        let first = make(&env.context(BackupKind::Manual, tools), &facts(180_007)).unwrap();
        let backups = env.backups();
        fs::create_dir_all(backups.join(".partial-old-1")).unwrap();
        let lock = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(backups.join(".lock"))
            .unwrap();
        lock.try_lock().unwrap();
        let before = entries(&backups);
        let tools = env.tools("18.7", DUMP_OK, 0);
        let error = make(&env.context(BackupKind::Manual, tools), &facts(180_007)).unwrap_err();
        assert!(matches!(error, BackupError::AlreadyRunning), "{error}");
        assert_eq!(entries(&backups), before);
        assert!(backups.join(".lock").exists());
        drop(lock);
        let tools = env.tools("18.7", DUMP_OK, 0);
        make(&env.context(BackupKind::Manual, tools), &facts(180_007)).unwrap();
        let found = entries(&backups);
        assert!(found.contains(&first.name) && found.len() == 2, "{found:?}");
    }

    #[test]
    fn the_mode_debug_output_hides_operator_arguments() {
        let mode = BackupMode::Command(vec!["/bin/backup".to_owned(), "secret-arg".to_owned()]);
        assert_eq!(format!("{mode:?}"), "Command(<2 arguments>)");
        assert_eq!(format!("{:?}", BackupMode::Off), "Off");
    }
}
