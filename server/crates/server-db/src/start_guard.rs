// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! The database start guard: one server per database, the identity of database, data directory and
//! instance key, the first initialisation, the backup before a migration and the migration.
//!
//! [`start`] covers the database part of the startup. The caller reads the configuration before
//! and serves requests after it, and ends the process when ownership is lost.
//!
//! # Order
//!
//! 1. **Lock.** A dedicated connection takes the session-level advisory lock
//!    [`INSTANCE_LOCK_KEY`] with `pg_try_advisory_lock`. A second server is refused at once
//!    ([`StartError::AnotherServerOwns`]); it never waits. Every later step runs under the lock,
//!    and the lock is checked in `pg_locks` once more before [`start`] returns.
//!    The connection goes to the [`OwnerGuard`], which keeps it open and watches it.
//! 2. **Read.** Marker, instance key, the migration history (`_sqlx_migrations`) and the instance
//!    tables are read. A history this build cannot continue (an unknown, a changed or a failed
//!    migration) is refused here, before anything is changed.
//! 3. **Decide.** Marker, key and database facts are compared (see [`IdentityRefusal`]). Every
//!    refusal happens before the first write. The result is one of: normal start, first
//!    initialisation, resuming an interrupted one, or reattaching an empty database.
//! 4. **Backup and migrate.** The instance tables are part of the schema, so an empty database is
//!    migrated *first* and then initialised or reattached; no backup is made for it, there is
//!    nothing to lose. For a completed instance with pending migrations the backup is made *before*
//!    the migration and any error blocks the start. An instance whose first initialisation never
//!    finished holds no data and is migrated without a backup. Migrations run on their own
//!    connection (not the lock connection); after a migration error that connection is closed and
//!    nothing is retried on it, because a migration lock may stay with the session until it ends.
//! 5. **Initialise or reattach.** See below.
//! 6. **Confirm.** Instance row, layout, key binding, marker and key are read again and must
//!    amount to a normal start. Only then [`start`] returns [`Started`].
//!
//! Reading before deciding keeps every refusal free of side effects, and doing the backup after the
//! decision means no backup is made for a start that will be refused.
//!
//! # First initialisation
//!
//! Each step is durable and the sequence resumes where it stopped:
//!
//! 1. one transaction inserts the initialisation row (`started`) and the instance row;
//! 2. the data directory marker is written with that identifier;
//! 3. the instance key is created;
//! 4. one transaction inserts the key binding and ticket key generation 1 and marks the
//!    initialisation `completed`.
//!
//! A start that finds `started` resumes only what that attempt provably began: no marker and no key
//! continue at step 2, the marker of this instance without a key at step 3, the marker with a key at
//! step 4. Any other combination is refused, never repaired.
//!
//! # Lost ownership
//!
//! The owner guard checks its connection on an interval. A broken connection, a terminated backend
//! or a check that times out, or a session that no longer holds the lock in `pg_locks`, means the
//! lock is gone: [`OwnerGuard::lost`] returns and the caller
//! must end the process. Another server may then take over; a new [`start`] in the same process is
//! possible but the old server must not keep serving.
//!
//! The connections do not use TLS, like the pool.

mod decision;

use std::fmt;
use std::path::Path;
use std::str::FromStr;
use std::time::Duration;

use sqlx::postgres::{PgConnectOptions, PgConnection};
use sqlx::{Connection, Postgres, Transaction};
use tokio::sync::watch;
use tokio::task::JoinHandle;

use crate::backup::{
    BackupContext, BackupError, BackupKind, BackupMode, BackupOutcome, ToolPaths,
    backup_before_migration,
};
use crate::error::connect_failure;
use crate::instance::{
    DataMarker, InstanceId, LAYOUT_VERSION, MarkerError, read_marker, write_marker,
};
use crate::{DatabaseUrl, DbError, InitialisationState, MIGRATOR};
use decision::{AppliedMigration, DbFacts, KnownMigration, Plan, classify_migrations, decide};

pub use decision::{IdentityRefusal, InitStep, MigrationRefusal};

/// The advisory lock that makes a server the owner of a database: the ASCII text `impluv` and the
/// number 1. Locks are per database, so servers on different databases never meet.
pub const INSTANCE_LOCK_KEY: i64 = 0x696d_706c_7576_0001;

/// Longest wait for a connection.
const CONNECT_LIMIT: Duration = Duration::from_secs(30);

/// The public side of an instance key.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PublicKeys {
    /// The public content key (Ed25519).
    pub content_public_key: [u8; 32],
    /// Its key identifier.
    pub content_key_id: [u8; 8],
    /// The key identifier of ticket key generation 1.
    pub ticket_key_id: [u8; 8],
}

/// An existing instance key, loaded and validated.
pub struct LoadedKeys {
    pub public: PublicKeys,
    /// The raw bytes of the instance key file, for the backup copy. Never shown.
    pub instance_key: [u8; 32],
}

impl fmt::Debug for LoadedKeys {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LoadedKeys")
            .field("public", &self.public)
            .field("instance_key", &"<redacted>")
            .finish()
    }
}

/// Why the instance key could not be loaded or created. The text must not contain key material or
/// paths.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KeyError(String);

impl KeyError {
    pub fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for KeyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for KeyError {}

/// The instance key file, seen from the database layer. The product binary implements it; this
/// crate never derives a key.
pub trait InstanceKeys: Sync {
    /// Loads and validates the instance key, or `None` when there is no key file.
    fn existing(&self) -> Result<Option<LoadedKeys>, KeyError>;

    /// Creates the key file exclusively and returns its public side. It must return an error when
    /// the file exists already; it must never replace or reuse an existing key.
    fn create(&self) -> Result<PublicKeys, KeyError>;
}

/// Settings of [`start`].
pub struct StartOptions<'a> {
    pub database_url: &'a DatabaseUrl,
    pub data_dir: &'a Path,
    pub keys: &'a dyn InstanceKeys,
    /// Explicit consent to bind an *empty* database to the existing data directory with this
    /// instance identifier. Without it, or with another identifier, such a start is refused. It is
    /// ignored in every other situation.
    pub accept_new_database: Option<InstanceId>,
    pub backup_mode: BackupMode,
    /// Time limit of the backup before a migration.
    pub backup_timeout: Duration,
    pub backup_tools: Option<ToolPaths>,
    /// How often the owner guard checks its connection.
    pub monitor_interval: Duration,
    /// How long one check may take before ownership counts as lost.
    pub monitor_timeout: Duration,
    /// Test seam: stop with [`StartError::InjectedStop`] after this initialisation step, as if the
    /// process had ended there. Leave it `None`.
    #[doc(hidden)]
    pub stop_after: Option<InitStep>,
}

impl<'a> StartOptions<'a> {
    /// The usual settings: built-in backup with a one hour limit, a check every five seconds.
    pub fn new(
        database_url: &'a DatabaseUrl,
        data_dir: &'a Path,
        keys: &'a dyn InstanceKeys,
    ) -> Self {
        Self {
            database_url,
            data_dir,
            keys,
            accept_new_database: None,
            backup_mode: BackupMode::Auto,
            backup_timeout: Duration::from_secs(3600),
            backup_tools: None,
            monitor_interval: Duration::from_secs(5),
            monitor_timeout: Duration::from_secs(5),
            stop_after: None,
        }
    }

    fn validate(&self) -> Result<(), StartError> {
        if self.monitor_interval.is_zero() {
            return Err(StartError::InvalidOption("monitor_interval"));
        }
        if self.monitor_timeout.is_zero() {
            return Err(StartError::InvalidOption("monitor_timeout"));
        }
        Ok(())
    }
}

/// Why the start was refused or failed. No variant carries the URL, a password or a path.
#[derive(Debug)]
pub enum StartError {
    /// An option is out of range.
    InvalidOption(&'static str),
    /// Another server holds the lock of this database.
    AnotherServerOwns,
    /// The database could not be reached or a query failed.
    Db(DbError),
    /// The marker of the data directory could not be read or written.
    Marker(MarkerError),
    /// The instance key could not be loaded or created.
    Keys(KeyError),
    /// Marker, key and database do not fit together.
    Identity(IdentityRefusal),
    /// The migration history does not allow a start.
    Migration(MigrationRefusal),
    /// The backup before the migration failed; nothing was migrated.
    Backup(BackupError),
    /// Applying the migrations failed; the migration connection was closed.
    Migrate(DbError),
    /// The lock was not held any more when the start finished.
    OwnershipLost,
    /// Test seam: the start stopped after this step on purpose.
    InjectedStop(InitStep),
}

impl fmt::Display for StartError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidOption(name) => write!(f, "invalid start option: {name}"),
            Self::AnotherServerOwns => f.write_str("another server owns this database"),
            Self::Db(error) => write!(f, "{error}"),
            Self::Marker(error) => write!(f, "{error}"),
            Self::Keys(error) => write!(f, "instance key: {error}"),
            Self::Identity(refusal) => write!(f, "{refusal}"),
            Self::Migration(refusal) => write!(f, "{refusal}"),
            Self::Backup(error) => write!(f, "backup before the migration failed: {error}"),
            Self::Migrate(error) => write!(f, "migration failed: {error}"),
            Self::OwnershipLost => f.write_str("the database lock was lost during the start"),
            Self::InjectedStop(step) => write!(f, "stopped after {step} (test seam)"),
        }
    }
}

impl std::error::Error for StartError {}

impl From<MarkerError> for StartError {
    fn from(error: MarkerError) -> Self {
        Self::Marker(error)
    }
}

impl From<KeyError> for StartError {
    fn from(error: KeyError) -> Self {
        Self::Keys(error)
    }
}

impl From<IdentityRefusal> for StartError {
    fn from(refusal: IdentityRefusal) -> Self {
        Self::Identity(refusal)
    }
}

impl From<MigrationRefusal> for StartError {
    fn from(refusal: MigrationRefusal) -> Self {
        Self::Migration(refusal)
    }
}

fn query_failed(error: sqlx::Error) -> StartError {
    StartError::Db(DbError::from_query(&error))
}

/// How the start went.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StartKind {
    /// Everything existed and matched.
    Normal,
    /// The first initialisation ran.
    FirstInitialisation,
    /// An interrupted first initialisation was continued from this step.
    Resumed { from: InitStep },
    /// An empty database was bound to the existing data directory.
    Reattached,
}

/// What happened about the backup.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BackupReport {
    /// No backup was due.
    NotNeeded,
    /// The operator chose not to back up. The caller must log this visibly.
    Skipped,
    /// The operator's program made the backup.
    Operator,
    /// A set was written, with this directory name.
    Created { name: String },
}

/// What the caller must tell the operator.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StartReport {
    pub kind: StartKind,
    /// Versions of the migrations applied by this start, ascending.
    pub applied_migrations: Vec<i64>,
    pub backup: BackupReport,
    /// Consequences of the start that the operator must know about; empty for a normal start.
    pub consequences: Vec<&'static str>,
}

/// A successful start.
#[derive(Debug)]
pub struct Started {
    /// Holds the lock. Keep it for as long as the server runs.
    pub owner: OwnerGuard,
    pub instance_id: InstanceId,
    pub layout_version: i32,
    pub report: StartReport,
}

/// Whether this process still owns the database.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Ownership {
    Held,
    /// The guard was released or dropped; the lock is given up.
    Released,
    Lost(LossReason),
}

/// Why ownership is gone.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LossReason {
    /// The connection broke or the backend was terminated.
    ConnectionError,
    /// The check did not answer in time.
    Timeout,
    /// The connection answers but no longer holds the lock.
    LockNotHeld,
    /// The guard was released or dropped. Only [`OwnerGuard::lost`] reports this.
    Released,
}

/// Holds the connection that holds the lock and watches it.
#[derive(Debug)]
pub struct OwnerGuard {
    state: watch::Receiver<Ownership>,
    stop: Option<watch::Sender<()>>,
    task: Option<JoinHandle<()>>,
}

impl OwnerGuard {
    fn spawn(connection: PgConnection, interval: Duration, timeout: Duration) -> Self {
        let (state_tx, state) = watch::channel(Ownership::Held);
        let (stop, stop_rx) = watch::channel(());
        let task = tokio::spawn(monitor(connection, interval, timeout, state_tx, stop_rx));
        Self {
            state,
            stop: Some(stop),
            task: Some(task),
        }
    }

    /// The current state.
    pub fn ownership(&self) -> Ownership {
        *self.state.borrow()
    }

    /// A receiver for the state, to wait on in the caller's own select loop.
    pub fn subscribe(&self) -> watch::Receiver<Ownership> {
        self.state.clone()
    }

    /// Returns when ownership is lost. The caller must then end the server process.
    pub async fn lost(&self) -> LossReason {
        let mut state = self.state.clone();
        let reached = state
            .wait_for(|ownership| *ownership != Ownership::Held)
            .await
            .map(|ownership| *ownership);
        match reached {
            Ok(Ownership::Lost(reason)) => reason,
            Ok(Ownership::Released) => LossReason::Released,
            _ => LossReason::ConnectionError,
        }
    }

    /// Gives the lock up and waits until the connection is closed.
    pub async fn release(mut self) {
        self.stop.take();
        if let Some(task) = self.task.take() {
            let _ = task.await;
        }
    }
}

async fn monitor(
    mut connection: PgConnection,
    interval: Duration,
    timeout: Duration,
    state: watch::Sender<Ownership>,
    mut stop: watch::Receiver<()>,
) {
    let mut outcome = Ownership::Released;
    loop {
        let check = tokio::time::timeout(timeout, holds_lock(&mut connection)).await;
        let reason = match check {
            Ok(Ok(true)) => None,
            Ok(Ok(false)) => Some(LossReason::LockNotHeld),
            Ok(Err(_)) => Some(LossReason::ConnectionError),
            Err(_) => Some(LossReason::Timeout),
        };
        if let Some(reason) = reason {
            outcome = Ownership::Lost(reason);
            break;
        }
        // The stop channel changes only by being closed: the guard was released or dropped.
        if tokio::time::timeout(interval, stop.changed()).await.is_ok() {
            break;
        }
    }
    // Publish before the slow part, so nobody keeps seeing `Held` while the lock is being given up.
    state.send_replace(outcome);
    unlock_and_close(connection).await;
}

/// Whether this session holds [`INSTANCE_LOCK_KEY`]. PostgreSQL stores a bigint advisory key with
/// the high 32 bits in `classid`, the low 32 bits in `objid` and `objsubid` 1.
async fn holds_lock(connection: &mut PgConnection) -> Result<bool, sqlx::Error> {
    let high = i64::from((INSTANCE_LOCK_KEY >> 32) as u32);
    let low = i64::from(INSTANCE_LOCK_KEY as u32);
    sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (SELECT 1 FROM pg_locks WHERE pid = pg_backend_pid() \
         AND locktype = 'advisory' AND granted AND objsubid = 1 \
         AND classid::bigint = $1 AND objid::bigint = $2)",
    )
    .bind(high)
    .bind(low)
    .fetch_one(connection)
    .await
}

/// Gives the lock up explicitly and closes the connection, so that a start right afterwards does
/// not meet a lock that is only released when the server notices the closed socket. Failures are
/// ignored: a dead connection has lost the lock anyway.
async fn unlock_and_close(mut connection: PgConnection) {
    let _ = tokio::time::timeout(
        Duration::from_secs(5),
        sqlx::query_scalar::<_, bool>("SELECT pg_advisory_unlock($1)")
            .bind(INSTANCE_LOCK_KEY)
            .fetch_one(&mut connection),
    )
    .await;
    let _ = tokio::time::timeout(CONNECT_LIMIT, connection.close()).await;
}

async fn connect(
    url: &DatabaseUrl,
    application_name: &str,
    statement_timeout: Option<Duration>,
) -> Result<PgConnection, StartError> {
    let mut options = PgConnectOptions::from_str(url.expose())
        .map_err(|_| StartError::Db(DbError::InvalidUrl))?
        .application_name(application_name);
    if let Some(limit) = statement_timeout {
        options = options.options([("statement_timeout", limit.as_millis().to_string())]);
    }
    match tokio::time::timeout(CONNECT_LIMIT, PgConnection::connect_with(&options)).await {
        Ok(Ok(connection)) => Ok(connection),
        Ok(Err(error)) => Err(StartError::Db(DbError::Connect(connect_failure(&error)))),
        Err(_) => Err(StartError::Db(DbError::Connect(
            crate::ConnectFailure::Timeout,
        ))),
    }
}

/// Starts the database side of the server. See the module documentation for the order.
///
/// On `Err` nothing is held: the lock connection and the working connections are closed.
pub async fn start(options: &StartOptions<'_>) -> Result<Started, StartError> {
    options.validate()?;
    let mut lock = connect(
        options.database_url,
        "impluvium-server-owner",
        Some(Duration::from_secs(10)),
    )
    .await?;
    match sqlx::query_scalar::<_, bool>("SELECT pg_try_advisory_lock($1)")
        .bind(INSTANCE_LOCK_KEY)
        .fetch_one(&mut lock)
        .await
    {
        Ok(true) => {}
        Ok(false) => {
            let _ = lock.close().await;
            return Err(StartError::AnotherServerOwns);
        }
        Err(error) => {
            unlock_and_close(lock).await;
            return Err(query_failed(error));
        }
    }
    match run(options).await {
        Ok((instance_id, layout_version, report)) => {
            // The lock must still be held when the start is declared done.
            let held = tokio::time::timeout(CONNECT_LIMIT, holds_lock(&mut lock)).await;
            if !matches!(held, Ok(Ok(true))) {
                unlock_and_close(lock).await;
                return Err(StartError::OwnershipLost);
            }
            Ok(Started {
                owner: OwnerGuard::spawn(lock, options.monitor_interval, options.monitor_timeout),
                instance_id,
                layout_version,
                report,
            })
        }
        Err(error) => {
            unlock_and_close(lock).await;
            Err(error)
        }
    }
}

async fn run(options: &StartOptions<'_>) -> Result<(InstanceId, i32, StartReport), StartError> {
    // No statement timeout: a migration may take long.
    let mut work = connect(options.database_url, "impluvium-server-start", None).await?;
    let result = run_on(options, &mut work).await;
    let _ = work.close().await;
    result
}

fn known_migrations() -> Vec<KnownMigration> {
    MIGRATOR
        .migrations
        .iter()
        .filter(|migration| !migration.migration_type.is_down_migration())
        .map(|migration| KnownMigration {
            version: migration.version,
            checksum: migration.checksum.to_vec(),
        })
        .collect()
}

async fn run_on(
    options: &StartOptions<'_>,
    conn: &mut PgConnection,
) -> Result<(InstanceId, i32, StartReport), StartError> {
    let marker = read_marker(options.data_dir)?;
    let loaded = options.keys.existing()?;
    let known = known_migrations();
    let state = classify_migrations(&read_applied_migrations(conn).await?, &known)?;
    let facts = read_facts(conn).await?;
    let public = loaded.as_ref().map(|loaded| &loaded.public);
    let plan = decide(
        &facts,
        marker.as_ref(),
        public,
        options.accept_new_database.as_ref(),
    )?;

    let mut report = StartReport {
        kind: StartKind::Normal,
        applied_migrations: Vec::new(),
        backup: BackupReport::NotNeeded,
        consequences: Vec::new(),
    };
    if !state.pending.is_empty() {
        let completed = facts.initialisation == Some(InitialisationState::Completed);
        if completed && let Some(loaded) = &loaded {
            let context = BackupContext {
                database_url: options.database_url,
                data_dir: options.data_dir,
                instance_key: &loaded.instance_key,
                kind: BackupKind::PreMigration {
                    from: state.applied_max.unwrap_or(0).to_string(),
                    to: known
                        .iter()
                        .map(|known| known.version)
                        .max()
                        .unwrap_or(0)
                        .to_string(),
                },
                mode: options.backup_mode.clone(),
                timeout: options.backup_timeout,
                tools: options.backup_tools.clone(),
            };
            report.backup = match backup_before_migration(&context)
                .await
                .map_err(StartError::Backup)?
            {
                BackupOutcome::Skipped => BackupReport::Skipped,
                BackupOutcome::Operator => BackupReport::Operator,
                BackupOutcome::Created(set) => BackupReport::Created { name: set.name },
            };
        }
        // The connection is closed by the caller on every error; nothing is retried on it.
        MIGRATOR
            .run(&mut *conn)
            .await
            .map_err(|error| StartError::Migrate(DbError::from(error)))?;
        report.applied_migrations = state.pending.clone();
    }

    let stop_after = options.stop_after;
    match plan {
        Plan::Normal => {}
        Plan::FirstInit => {
            report.kind = StartKind::FirstInitialisation;
            let id = insert_instance_rows(conn).await?;
            injected(stop_after, InitStep::Rows)?;
            continue_init(options, conn, &id, InitStep::Marker, None).await?;
        }
        Plan::Resume(from) => {
            report.kind = StartKind::Resumed { from };
            let (id, _) = facts
                .instance
                .clone()
                .ok_or(IdentityRefusal::Inconsistent)?;
            continue_init(options, conn, &id, from, loaded.as_ref().map(|l| &l.public)).await?;
        }
        Plan::Reattach => {
            let marker = marker.as_ref().ok_or(IdentityRefusal::Inconsistent)?;
            let public = public.ok_or(IdentityRefusal::MarkerWithoutKey)?;
            reattach(conn, marker, public).await?;
            report.kind = StartKind::Reattached;
            report.consequences = vec![
                "The database was empty. It now belongs to the existing data directory and \
                 starts without any data of the old database.",
                "Existing devices keep trusting the same content key.",
                "The ticket key generations start again at generation 1.",
            ];
        }
    }

    // Confirm with fresh reads.
    let marker = read_marker(options.data_dir)?;
    let loaded = options.keys.existing()?;
    let facts = read_facts(conn).await?;
    let plan = decide(
        &facts,
        marker.as_ref(),
        loaded.as_ref().map(|loaded| &loaded.public),
        None,
    )?;
    if plan != Plan::Normal {
        return Err(IdentityRefusal::Unconfirmed.into());
    }
    let (id, layout) = facts.instance.ok_or(IdentityRefusal::Unconfirmed)?;
    Ok((id, layout, report))
}

fn injected(stop_after: Option<InitStep>, step: InitStep) -> Result<(), StartError> {
    if stop_after == Some(step) {
        Err(StartError::InjectedStop(step))
    } else {
        Ok(())
    }
}

/// Runs the steps of the first initialisation from `from` on. `existing` holds the public keys of
/// a key that exists already.
async fn continue_init(
    options: &StartOptions<'_>,
    conn: &mut PgConnection,
    id: &InstanceId,
    from: InitStep,
    existing: Option<&PublicKeys>,
) -> Result<(), StartError> {
    let mut public = existing.cloned();
    if from <= InitStep::Marker {
        let marker = DataMarker {
            instance_id: id.clone(),
            layout_version: LAYOUT_VERSION,
        };
        write_marker(options.data_dir, &marker)?;
        injected(options.stop_after, InitStep::Marker)?;
    }
    if from <= InitStep::Key {
        public = Some(options.keys.create()?);
        injected(options.stop_after, InitStep::Key)?;
    }
    let public = public.ok_or(IdentityRefusal::MarkerWithoutKey)?;
    bind_keys(conn, &public).await?;
    injected(options.stop_after, InitStep::Bind)
}

async fn insert_instance_rows(conn: &mut PgConnection) -> Result<InstanceId, StartError> {
    let mut tx = conn.begin().await.map_err(query_failed)?;
    sqlx::query("INSERT INTO instance_initialisation (state) VALUES ('started')")
        .execute(&mut *tx)
        .await
        .map_err(query_failed)?;
    let id: String = sqlx::query_scalar(
        "INSERT INTO instance_identity (layout_version) VALUES ($1) RETURNING instance_id::text",
    )
    .bind(LAYOUT_VERSION)
    .fetch_one(&mut *tx)
    .await
    .map_err(query_failed)?;
    tx.commit().await.map_err(query_failed)?;
    InstanceId::parse(&id).ok_or(StartError::Db(DbError::Query))
}

async fn insert_key_rows(
    tx: &mut Transaction<'_, Postgres>,
    public: &PublicKeys,
) -> Result<(), StartError> {
    sqlx::query(
        "INSERT INTO content_key_binding (key_id, public_key, algorithm) VALUES ($1, $2, 'ed25519')",
    )
    .bind(&public.content_key_id[..])
    .bind(&public.content_public_key[..])
    .execute(&mut **tx)
    .await
    .map_err(query_failed)?;
    sqlx::query(
        "INSERT INTO ticket_key_generation (generation, key_id, switch_at) VALUES (1, $1, now())",
    )
    .bind(&public.ticket_key_id[..])
    .execute(&mut **tx)
    .await
    .map_err(query_failed)?;
    Ok(())
}

async fn bind_keys(conn: &mut PgConnection, public: &PublicKeys) -> Result<(), StartError> {
    let mut tx = conn.begin().await.map_err(query_failed)?;
    insert_key_rows(&mut tx, public).await?;
    let updated = sqlx::query(
        "UPDATE instance_initialisation SET state = 'completed', completed_at = now() \
         WHERE state = 'started'",
    )
    .execute(&mut *tx)
    .await
    .map_err(query_failed)?;
    if updated.rows_affected() != 1 {
        return Err(IdentityRefusal::Inconsistent.into());
    }
    tx.commit().await.map_err(query_failed)
}

async fn reattach(
    conn: &mut PgConnection,
    marker: &DataMarker,
    public: &PublicKeys,
) -> Result<(), StartError> {
    let mut tx = conn.begin().await.map_err(query_failed)?;
    sqlx::query(
        "INSERT INTO instance_identity (instance_id, layout_version) VALUES ($1::text::uuid, $2)",
    )
    .bind(marker.instance_id.as_str())
    .bind(marker.layout_version)
    .execute(&mut *tx)
    .await
    .map_err(query_failed)?;
    sqlx::query(
        "INSERT INTO instance_initialisation (state, completed_at) VALUES ('completed', now())",
    )
    .execute(&mut *tx)
    .await
    .map_err(query_failed)?;
    insert_key_rows(&mut tx, public).await?;
    tx.commit().await.map_err(query_failed)
}

async fn read_applied_migrations(
    conn: &mut PgConnection,
) -> Result<Vec<AppliedMigration>, StartError> {
    let exists: bool = sqlx::query_scalar("SELECT to_regclass('_sqlx_migrations') IS NOT NULL")
        .fetch_one(&mut *conn)
        .await
        .map_err(query_failed)?;
    if !exists {
        return Ok(Vec::new());
    }
    let rows = sqlx::query_as::<_, (i64, Vec<u8>, bool)>(
        "SELECT version, checksum, success FROM _sqlx_migrations ORDER BY version",
    )
    .fetch_all(&mut *conn)
    .await
    .map_err(query_failed)?;
    Ok(rows
        .into_iter()
        .map(|(version, checksum, success)| AppliedMigration {
            version,
            checksum,
            success,
        })
        .collect())
}

async fn read_facts(conn: &mut PgConnection) -> Result<DbFacts, StartError> {
    let (identity, binding, ticket, initialisation) =
        sqlx::query_as::<_, (bool, bool, bool, bool)>(
            "SELECT to_regclass('instance_identity') IS NOT NULL, \
                to_regclass('content_key_binding') IS NOT NULL, \
                to_regclass('ticket_key_generation') IS NOT NULL, \
                to_regclass('instance_initialisation') IS NOT NULL",
        )
        .fetch_one(&mut *conn)
        .await
        .map_err(query_failed)?;
    match (identity, binding, ticket, initialisation) {
        (false, false, false, false) => {
            // No instance tables. Anything else in the database is not ours to build on,
            // except relations that belong to an installed extension. The empty
            // `_sqlx_migrations` exception is safe only because the migration history was
            // classified before this read (see `run_on`).
            let foreign = sqlx::query_scalar::<_, bool>(
                "SELECT EXISTS (SELECT 1 FROM pg_class c \
                 JOIN pg_namespace n ON n.oid = c.relnamespace \
                 WHERE c.relkind IN ('r', 'p', 'v', 'm', 'f', 'S') \
                 AND n.nspname <> 'information_schema' AND n.nspname !~ '^pg_' \
                 AND NOT EXISTS (SELECT 1 FROM pg_depend d \
                                 WHERE d.classid = 'pg_class'::regclass \
                                 AND d.objid = c.oid AND d.deptype = 'e') \
                 AND NOT (c.relname = '_sqlx_migrations' \
                          AND COALESCE(n.nspname = current_schema(), false)))",
            )
            .fetch_one(&mut *conn)
            .await
            .map_err(query_failed)?;
            return Ok(DbFacts {
                foreign_relations: foreign,
                ..DbFacts::default()
            });
        }
        (true, true, true, true) => {}
        _ => return Err(IdentityRefusal::Inconsistent.into()),
    }
    let instance = sqlx::query_as::<_, (String, i32)>(
        "SELECT instance_id::text, layout_version FROM instance_identity",
    )
    .fetch_optional(&mut *conn)
    .await
    .map_err(query_failed)?
    .map(|(id, layout)| {
        InstanceId::parse(&id)
            .map(|id| (id, layout))
            .ok_or(StartError::Db(DbError::Query))
    })
    .transpose()?;
    let state = sqlx::query_scalar::<_, String>("SELECT state FROM instance_initialisation")
        .fetch_optional(&mut *conn)
        .await
        .map_err(query_failed)?;
    let initialisation = match state.as_deref() {
        None => None,
        Some("started") => Some(InitialisationState::Started),
        Some("completed") => Some(InitialisationState::Completed),
        Some(_) => return Err(StartError::Db(DbError::Query)),
    };
    let binding_rows = sqlx::query_scalar::<_, i64>("SELECT count(*) FROM content_key_binding")
        .fetch_one(&mut *conn)
        .await
        .map_err(query_failed)?;
    let active_binding = sqlx::query_as::<_, (Vec<u8>, Vec<u8>)>(
        "SELECT key_id, public_key FROM content_key_binding WHERE retired_at IS NULL",
    )
    .fetch_optional(&mut *conn)
    .await
    .map_err(query_failed)?;
    let ticket_rows = sqlx::query_scalar::<_, i64>("SELECT count(*) FROM ticket_key_generation")
        .fetch_one(&mut *conn)
        .await
        .map_err(query_failed)?;
    let first_ticket_key_id = sqlx::query_scalar::<_, Vec<u8>>(
        "SELECT key_id FROM ticket_key_generation WHERE generation = 1",
    )
    .fetch_optional(&mut *conn)
    .await
    .map_err(query_failed)?;
    Ok(DbFacts {
        instance,
        initialisation,
        binding_rows,
        active_binding,
        ticket_rows,
        first_ticket_key_id,
        foreign_relations: false,
    })
}
