// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! `impluvium-server start`: the guarded start of the server.
//!
//! The order is fixed and nothing listens before the last step:
//!
//! 1. load and check the configuration;
//! 2. check the built web application directory, when one is configured;
//! 3. check the data directory and lock it;
//! 4. check the PostgreSQL version;
//! 5. run the database start guard (lock, identity checks, backup, migrations, first start);
//! 6. connect the product pool;
//! 7. build the router, with readiness tied to the database lock;
//! 8. bind the listener and serve until a signal arrives or the database lock is lost.
//!
//! Any failure ends the process with a message that holds no secret and a non-zero exit code. On
//! a normal stop the pool is closed and the database lock is released. When the lock is lost, the
//! readiness probe turns to `503` at once, the server stops accepting, in-flight requests get the
//! usual shutdown time, nothing more is released (the lock is gone) and the process exits with
//! [`Exit::OwnershipLost`].

use std::cell::Cell;
use std::future::{Future, poll_fn};
use std::io::Write;
use std::path::Path;
use std::pin::pin;
use std::process::ExitCode;
use std::sync::Arc;
use std::task::Poll;
use std::time::Duration;

use serde_json::Map;
use server_db::backup::BackupMode as DbBackupMode;
use server_db::instance::InstanceId;
use server_db::start_guard::{
    self, BackupReport, LossReason, OwnerGuard, StartKind, StartOptions, Started,
};
use server_db::{
    Database, DatabaseUrl, NEWEST_TESTED_MAJOR, OLDEST_SUPPORTED_MAJOR, PoolOptions,
    VersionVerdict, judge_server_version, read_server_version_num,
};
use server_domain::credential::NoCredentials;
use server_http::auth::{ExternalOrigin, OriginError, Scheme as OriginScheme, SecurityContext};
use server_http::emoji::EmojiFiles;
use server_http::observe::{RequestLogEntry, RequestLogSink};
use server_http::proxy::{InvalidNetwork, Network, TrustedProxies};
use server_http::registry;
use server_http::router::{AppOptions, build_with};
use server_http::serve::{BoundListener, serve};

use crate::config::{
    self, BackupMode as ConfiguredBackup, Cidr, Config, ExternalUrl, LogFormat, Scheme, Source,
};
use crate::data_dir::{self, DataDirectory};
use crate::emoji;
use crate::instance_keys::FileKeys;
use crate::logging::{self, JsonLogger, Level};
use crate::readiness::Readiness;
use crate::shutdown::{self, Cause, Outcome, ShutdownSignal};
use crate::spa::{self, SpaArtifact};

/// How the process ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Exit {
    /// Stopped by a signal; in-flight requests finished in time.
    Clean,
    /// The start was refused or the server failed.
    Failed,
    /// The database lock was lost while serving.
    OwnershipLost,
    /// Stopped by a signal, but a request was still running when the shutdown time ran out.
    ShutdownTimedOut,
}

impl Exit {
    /// The process exit code. The values are part of the administrator documentation.
    pub(crate) fn code(self) -> u8 {
        match self {
            Self::Clean => 0,
            Self::Failed => 1,
            Self::OwnershipLost => 3,
            Self::ShutdownTimedOut => 4,
        }
    }
}

/// What made the server stop serving.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StopCause {
    Signal(Cause),
    OwnershipLost(LossReason),
}

/// The exit of a run that began serving.
fn conclude(cause: Option<StopCause>, outcome: Outcome, server_failed: bool) -> Exit {
    match cause {
        Some(StopCause::OwnershipLost(_)) => Exit::OwnershipLost,
        _ if server_failed => Exit::Failed,
        Some(StopCause::Signal(_)) => match outcome {
            Outcome::Completed => Exit::Clean,
            Outcome::DeadlineExceeded => Exit::ShutdownTimedOut,
        },
        // The server ended without being asked to.
        None => Exit::Failed,
    }
}

/// Where messages go: JSON lines or plain lines on standard error.
struct Console {
    json: Option<Arc<JsonLogger>>,
}

impl Console {
    fn new(format: LogFormat) -> Self {
        match format {
            LogFormat::Json => {
                let logger = Arc::new(JsonLogger::new(Box::new(std::io::stderr())));
                logging::install_panic_hook(Arc::clone(&logger));
                Self { json: Some(logger) }
            }
            LogFormat::Text => Self { json: None },
        }
    }

    fn log(&self, level: Level, message: &str) {
        match &self.json {
            Some(logger) => logger.log(level, message, None, Map::new()),
            None => {
                let word = match level {
                    Level::Info => "info",
                    Level::Warn => "warn",
                    Level::Error => "error",
                };
                let _ = writeln!(std::io::stderr(), "{word} {}", logging::redact(message));
            }
        }
    }

    fn info(&self, message: &str) {
        self.log(Level::Info, message);
    }

    fn warn(&self, message: &str) {
        self.log(Level::Warn, message);
    }

    fn error(&self, message: &str) {
        self.log(Level::Error, message);
    }

    fn request_sink(&self) -> Arc<dyn RequestLogSink> {
        match &self.json {
            Some(logger) => Arc::clone(logger) as Arc<dyn RequestLogSink>,
            None => Arc::new(TextRequests),
        }
    }
}

/// Request lines in the plain format.
struct TextRequests;

impl RequestLogSink for TextRequests {
    fn request(&self, entry: &RequestLogEntry) {
        let method: String = entry
            .method
            .chars()
            .filter(char::is_ascii_uppercase)
            .collect();
        let _ = writeln!(
            std::io::stderr(),
            "info request {method} {} {} {}ms id={}",
            entry.path,
            entry.status,
            entry.duration.as_millis(),
            entry.request_id
        );
    }
}

/// What the checks before the database leave behind. Dropping it releases the data directory lock.
struct Prepared {
    spa: SpaArtifact,
    emoji: EmojiFiles,
    _data_dir: DataDirectory,
}

/// Steps 2 and 3: the web application directory, the emoji directory when one is configured, and
/// the data directory.
fn prepare(config: &Config) -> Result<Prepared, String> {
    let Some(dir) = &config.spa_dir else {
        return Err("IMPLUVIUM_SPA_DIR is required to start".to_owned());
    };
    let spa = spa::validate_artifact(Path::new(dir))
        .map_err(|error| format!("IMPLUVIUM_SPA_DIR: {error}"))?;
    let emoji = match &config.emoji_dir {
        Some(dir) => {
            emoji::load(Path::new(dir)).map_err(|error| format!("IMPLUVIUM_EMOJI_DIR: {error}"))?
        }
        None => EmojiFiles::empty(),
    };
    let data_dir =
        data_dir::open(&config.data_dir).map_err(|error| format!("IMPLUVIUM_DATA_DIR: {error}"))?;
    Ok(Prepared {
        spa,
        emoji,
        _data_dir: data_dir,
    })
}

/// The external origin the gate uses for cookies and the cross-site check.
fn external_origin(url: &ExternalUrl) -> Result<ExternalOrigin, OriginError> {
    let scheme = match url.scheme {
        Scheme::Http => OriginScheme::Http,
        Scheme::Https => OriginScheme::Https,
    };
    ExternalOrigin::new(scheme, &url.host, url.port)
}

fn trusted_proxies(networks: &[Cidr]) -> Result<TrustedProxies, InvalidNetwork> {
    let mut list = Vec::new();
    for network in networks {
        list.push(Network::new(network.addr, network.prefix)?);
    }
    Ok(TrustedProxies::new(list))
}

/// The backup mode of the start guard from the configured one.
fn backup_mode(config: &Config) -> DbBackupMode {
    match config.backup_mode {
        ConfiguredBackup::Auto => DbBackupMode::Auto,
        ConfiguredBackup::Command => DbBackupMode::Command(config.backup_command.clone()),
        ConfiguredBackup::Off => DbBackupMode::Off,
    }
}

/// Waits for a stop signal or the loss of the database lock; the loss wins when both are ready.
async fn wait_for_stop(signals: &mut ShutdownSignal, owner: &OwnerGuard) -> StopCause {
    let mut signal = pin!(signals.recv());
    let mut lost = pin!(owner.lost());
    poll_fn(|context| {
        if let Poll::Ready(reason) = lost.as_mut().poll(context) {
            return Poll::Ready(StopCause::OwnershipLost(reason));
        }
        if let Poll::Ready(cause) = signal.as_mut().poll(context) {
            return Poll::Ready(StopCause::Signal(cause));
        }
        Poll::Pending
    })
    .await
}

/// `impluvium-server start`.
pub(crate) fn run() -> ExitCode {
    ExitCode::from(execute(&Source::from_process()).code())
}

fn execute(source: &Source) -> Exit {
    let config = match config::load(source) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("configuration is invalid\n{error}");
            return Exit::Failed;
        }
    };
    let console = Console::new(config.log_format);
    let prepared = match prepare(&config) {
        Ok(prepared) => prepared,
        Err(message) => {
            console.error(&message);
            return Exit::Failed;
        }
    };
    console.info(&format!(
        "web application checked: {} files, {} inline script hashes",
        prepared.spa.files,
        prepared.spa.script_hashes.len()
    ));
    if config.emoji_dir.is_some() {
        console.info(&format!(
            "emoji pictures checked: {} files",
            prepared.emoji.len()
        ));
    }
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(_) => {
            console.error("the server could not start its runtime");
            return Exit::Failed;
        }
    };
    runtime.block_on(serve_product(
        &config,
        prepared.spa,
        Arc::new(prepared.emoji),
        &console,
    ))
}

/// Steps 4 to 8, and the stop.
async fn serve_product(
    config: &Config,
    spa: SpaArtifact,
    emoji: Arc<EmojiFiles>,
    console: &Console,
) -> Exit {
    let url = DatabaseUrl::new(config.database_url.expose());
    match read_server_version_num(&url).await {
        Err(error) => {
            console.error(&error.to_string());
            return Exit::Failed;
        }
        Ok(number) => match judge_server_version(number) {
            None => {
                console.error("the database server reports a version that cannot be read");
                return Exit::Failed;
            }
            Some(VersionVerdict::TooOld { major }) => {
                console.error(&format!(
                    "PostgreSQL {major} is not supported; use PostgreSQL \
                     {OLDEST_SUPPORTED_MAJOR} to {NEWEST_TESTED_MAJOR}"
                ));
                return Exit::Failed;
            }
            Some(VersionVerdict::Newer { major }) => console.warn(&format!(
                "PostgreSQL {major} is newer than the tested versions \
                 ({OLDEST_SUPPORTED_MAJOR} to {NEWEST_TESTED_MAJOR}); continuing"
            )),
            Some(VersionVerdict::Supported { .. }) => {}
        },
    }

    let keys = FileKeys::new(&config.data_dir);
    let mut options = StartOptions::new(&url, &config.data_dir, &keys);
    options.accept_new_database = config
        .accept_new_database
        .as_deref()
        .and_then(InstanceId::parse);
    options.backup_mode = backup_mode(config);
    options.backup_timeout = Duration::from_secs(u64::from(config.backup_timeout_secs));
    let started = match start_guard::start(&options).await {
        Ok(started) => started,
        Err(error) => {
            console.error(&error.to_string());
            return Exit::Failed;
        }
    };
    report_start(&started, console);
    run_owned(config, spa, emoji, console, &url, started).await
}

fn report_start(started: &Started, console: &Console) {
    let kind = match &started.report.kind {
        StartKind::Normal => "normal start".to_owned(),
        StartKind::FirstInitialisation => "first initialisation".to_owned(),
        StartKind::Resumed { from } => format!("resumed the first initialisation at {from}"),
        StartKind::Reattached => "empty database bound to the existing data directory".to_owned(),
    };
    console.info(&format!(
        "database ready: instance {}, {kind}, {} migrations applied",
        started.instance_id.as_str(),
        started.report.applied_migrations.len()
    ));
    match &started.report.backup {
        BackupReport::NotNeeded => {}
        BackupReport::Skipped => {
            console
                .warn("no backup was made before the migration: the operator turned backups off");
        }
        BackupReport::Operator => console.info("the operator backup program made the backup"),
        BackupReport::Created { name } => console.info(&format!("backup written: {name}")),
    }
    for consequence in &started.report.consequences {
        console.warn(consequence);
    }
}

/// Steps 6 to 8 while this process owns the database. Releases the lock on every exit except the
/// loss of the lock itself.
async fn run_owned(
    config: &Config,
    spa: SpaArtifact,
    emoji: Arc<EmojiFiles>,
    console: &Console,
    url: &DatabaseUrl,
    started: Started,
) -> Exit {
    let owner = started.owner;
    let pool_options = PoolOptions {
        max_connections: config.db_pool_max,
        min_connections: config.db_pool_min,
        acquire_timeout: Duration::from_secs(u64::from(config.db_timeout_secs)),
        ..PoolOptions::default()
    };
    let database = match Database::connect(url, &pool_options).await {
        Ok(database) => database,
        Err(error) => {
            console.error(&error.to_string());
            owner.release().await;
            return Exit::Failed;
        }
    };
    let exit = serve_owned(config, spa, emoji, console, &owner).await;
    // Do not wait long for connections that an abandoned request still holds.
    let _ = tokio::time::timeout(Duration::from_secs(5), database.close()).await;
    if exit == Exit::OwnershipLost {
        // The lock is gone; there is nothing left to release.
        drop(owner);
    } else {
        owner.release().await;
    }
    exit
}

async fn serve_owned(
    config: &Config,
    spa: SpaArtifact,
    emoji: Arc<EmojiFiles>,
    console: &Console,
    owner: &OwnerGuard,
) -> Exit {
    let (Ok(origin), Ok(proxies)) = (
        external_origin(&config.external_url),
        trusted_proxies(&config.trusted_proxies),
    ) else {
        console.error("IMPLUVIUM_EXTERNAL_URL or IMPLUVIUM_TRUSTED_PROXIES cannot be used");
        return Exit::Failed;
    };
    let readiness = Arc::new(Readiness::new(owner.subscribe()));
    let app_options = AppOptions {
        log_sink: console.request_sink(),
        script_hashes: spa.script_hashes,
        spa: Some(Arc::new(spa.served)),
    };
    let app = build_with(
        registry::application_with(
            Arc::clone(&readiness) as Arc<dyn server_http::health::ReadinessPort>,
            emoji,
        ),
        SecurityContext::new(origin, proxies, Arc::new(NoCredentials)),
        &app_options,
    );
    let listener = match BoundListener::bind(config.listen_addr).await {
        Ok(listener) => listener,
        Err(error) => {
            console.error(&format!(
                "cannot listen on the configured address: {}",
                error.kind()
            ));
            return Exit::Failed;
        }
    };
    let mut signals = match ShutdownSignal::install() {
        Ok(signals) => signals,
        Err(_) => {
            console.error("the stop signal handlers could not be installed");
            return Exit::Failed;
        }
    };
    readiness.open();
    match listener.local_addr() {
        Ok(address) => console.info(&format!("listening on {address}")),
        Err(_) => console.info("listening"),
    }

    let cause: Cell<Option<StopCause>> = Cell::new(None);
    let server_failed = Cell::new(false);
    let trigger = async {
        let reason = wait_for_stop(&mut signals, owner).await;
        // Readiness turns to 503 before the drain begins.
        readiness.close();
        cause.set(Some(reason));
    };
    let outcome = shutdown::serve_until(
        |graceful| async {
            if serve(listener, app, graceful).await.is_err() {
                server_failed.set(true);
            }
        },
        trigger,
        shutdown::DEADLINE,
    )
    .await;
    readiness.close();
    match cause.get() {
        Some(StopCause::Signal(signal)) => console.info(&format!("stopping on {signal:?}")),
        Some(StopCause::OwnershipLost(reason)) => console.error(&format!(
            "the database lock was lost ({reason:?}); stopping without releasing it"
        )),
        None => console.error("the server stopped unexpectedly"),
    }
    let exit = conclude(cause.get(), outcome, server_failed.get());
    if exit == Exit::ShutdownTimedOut {
        console.warn("requests were still running when the shutdown time ran out");
    }
    exit
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scratch::Scratch;

    const ID: &str = "0190a1b2-c3d4-4e5f-8a9b-0c1d2e3f4a5b";

    fn config(extra: &[(&str, &str)]) -> Config {
        let mut pairs = vec![
            ("IMPLUVIUM_EXTERNAL_URL", "https://impluvium.example"),
            ("IMPLUVIUM_DATA_DIR", "/nonexistent-impluvium-dir/data"),
            ("DATABASE_URL", "postgres://u:p@db.example.test/x"),
        ];
        pairs.retain(|(name, _)| !extra.iter().any(|(other, _)| other == name));
        pairs.extend_from_slice(extra);
        config::load(&Source::from_pairs(&pairs)).expect("a valid test configuration")
    }

    #[test]
    fn exit_codes_are_distinct_and_stable() {
        let codes = [
            Exit::Clean.code(),
            Exit::Failed.code(),
            Exit::OwnershipLost.code(),
            Exit::ShutdownTimedOut.code(),
        ];
        assert_eq!(codes, [0, 1, 3, 4]);
    }

    #[test]
    fn the_exit_follows_the_cause_of_the_stop() {
        let signal = Some(StopCause::Signal(Cause::Terminate));
        let lost = Some(StopCause::OwnershipLost(LossReason::LockNotHeld));
        assert_eq!(conclude(signal, Outcome::Completed, false), Exit::Clean);
        assert_eq!(
            conclude(signal, Outcome::DeadlineExceeded, false),
            Exit::ShutdownTimedOut
        );
        assert_eq!(conclude(signal, Outcome::Completed, true), Exit::Failed);
        assert_eq!(
            conclude(lost, Outcome::Completed, false),
            Exit::OwnershipLost
        );
        assert_eq!(
            conclude(lost, Outcome::DeadlineExceeded, true),
            Exit::OwnershipLost
        );
        assert_eq!(conclude(None, Outcome::Completed, false), Exit::Failed);
    }

    #[test]
    fn configuration_values_convert_to_server_types() {
        let configured = config(&[
            ("IMPLUVIUM_EXTERNAL_URL", "http://localhost:3000"),
            (
                "IMPLUVIUM_TRUSTED_PROXIES",
                "192.0.2.10/32, 2001:db8::10/128",
            ),
            ("IMPLUVIUM_BACKUP_MODE", "command"),
            (
                "IMPLUVIUM_BACKUP_COMMAND",
                r#"["/usr/local/bin/backup", "x"]"#,
            ),
        ]);
        let origin = external_origin(&configured.external_url).unwrap();
        assert_eq!(origin.as_str(), "http://localhost:3000");
        assert!(trusted_proxies(&configured.trusted_proxies).is_ok());
        assert_eq!(
            backup_mode(&configured),
            DbBackupMode::Command(vec!["/usr/local/bin/backup".to_owned(), "x".to_owned()])
        );
        assert_eq!(backup_mode(&config(&[])), DbBackupMode::Auto);
        let off = config(&[("IMPLUVIUM_BACKUP_MODE", "off")]);
        assert_eq!(backup_mode(&off), DbBackupMode::Off);
    }

    #[cfg(unix)]
    #[test]
    fn the_checks_before_the_database_refuse_with_clear_messages() {
        let scratch = Scratch::new("start-prepare");
        let data = scratch.path().to_string_lossy().into_owned();
        let web = Scratch::new("start-web");
        crate::spa::write_test_artifact(web.path());
        let web_dir = web.path().to_string_lossy().into_owned();
        let missing_spa = scratch.path().join("no-web").to_string_lossy().into_owned();
        let message = prepare(&config(&[
            ("IMPLUVIUM_DATA_DIR", data.as_str()),
            ("IMPLUVIUM_SPA_DIR", missing_spa.as_str()),
        ]))
        .err()
        .expect("a missing web application directory is refused");
        assert!(message.starts_with("IMPLUVIUM_SPA_DIR:"), "{message}");
        let missing_data = scratch
            .path()
            .join("no-data")
            .to_string_lossy()
            .into_owned();
        let message = prepare(&config(&[
            ("IMPLUVIUM_DATA_DIR", missing_data.as_str()),
            ("IMPLUVIUM_SPA_DIR", web_dir.as_str()),
        ]))
        .err()
        .expect("a missing data directory is refused");
        assert!(message.contains("does not exist"), "{message}");
        let both = [
            ("IMPLUVIUM_DATA_DIR", data.as_str()),
            ("IMPLUVIUM_SPA_DIR", web_dir.as_str()),
        ];
        let first = prepare(&config(&both)).unwrap();
        let message = prepare(&config(&both))
            .err()
            .expect("a second server on the same data directory is refused");
        assert!(
            message.contains("another server uses this data directory"),
            "{message}"
        );
        drop(first);
    }

    #[cfg(unix)]
    #[test]
    fn the_emoji_directory_is_checked_before_the_data_directory_is_locked() {
        let data = Scratch::new("start-emoji-data");
        let data_dir = data.path().to_string_lossy().into_owned();
        let web = Scratch::new("start-emoji-web");
        crate::spa::write_test_artifact(web.path());
        let web_dir = web.path().to_string_lossy().into_owned();
        let pictures = Scratch::new("start-emoji-files");
        std::fs::write(pictures.path().join("1f600.svg"), "<svg/>").unwrap();
        let pictures_dir = pictures.path().to_string_lossy().into_owned();
        let missing = data.path().join("no-emoji").to_string_lossy().into_owned();
        let message = prepare(&config(&[
            ("IMPLUVIUM_DATA_DIR", data_dir.as_str()),
            ("IMPLUVIUM_SPA_DIR", web_dir.as_str()),
            ("IMPLUVIUM_EMOJI_DIR", missing.as_str()),
        ]))
        .err()
        .expect("a missing emoji directory is refused");
        assert!(message.starts_with("IMPLUVIUM_EMOJI_DIR: "), "{message}");
        assert!(!data.path().join(".impluvium-lock").exists());
        let prepared = prepare(&config(&[
            ("IMPLUVIUM_DATA_DIR", data_dir.as_str()),
            ("IMPLUVIUM_SPA_DIR", web_dir.as_str()),
            ("IMPLUVIUM_EMOJI_DIR", pictures_dir.as_str()),
        ]))
        .unwrap();
        assert_eq!(prepared.emoji.len(), 1);
        drop(prepared);
        let without = prepare(&config(&[
            ("IMPLUVIUM_DATA_DIR", data_dir.as_str()),
            ("IMPLUVIUM_SPA_DIR", web_dir.as_str()),
        ]))
        .unwrap();
        assert!(without.emoji.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn a_start_without_a_valid_web_application_is_refused() {
        let scratch = Scratch::new("start-no-web");
        let data_path = scratch.path().join("data");
        std::fs::create_dir(&data_path).unwrap();
        let data = data_path.to_string_lossy().into_owned();
        let empty = scratch.path().join("web");
        std::fs::create_dir(&empty).unwrap();
        let empty = empty.to_string_lossy().into_owned();
        // The data directory is fine, so only the web application can be the cause.
        let without = config(&[("IMPLUVIUM_DATA_DIR", data.as_str())]);
        let message = prepare(&without)
            .err()
            .expect("no web application is refused");
        assert_eq!(message, "IMPLUVIUM_SPA_DIR is required to start");
        let invalid = config(&[
            ("IMPLUVIUM_DATA_DIR", data.as_str()),
            ("IMPLUVIUM_SPA_DIR", empty.as_str()),
        ]);
        let message = prepare(&invalid)
            .err()
            .expect("an invalid web application is refused");
        assert!(message.starts_with("IMPLUVIUM_SPA_DIR: "), "{message}");
        assert!(!data_path.join(".impluvium-lock").exists());
        // The whole start ends before it touches the database.
        let base = [
            ("IMPLUVIUM_EXTERNAL_URL", "https://impluvium.example"),
            ("IMPLUVIUM_DATA_DIR", data.as_str()),
            ("DATABASE_URL", "postgres://u:p@db.example.test/x"),
        ];
        assert_eq!(execute(&Source::from_pairs(&base)), Exit::Failed);
    }

    #[test]
    fn the_accept_setting_maps_to_an_instance_identifier() {
        let accepting = config(&[("IMPLUVIUM_ACCEPT_NEW_DATABASE", ID)]);
        let parsed = accepting
            .accept_new_database
            .as_deref()
            .and_then(InstanceId::parse);
        assert_eq!(parsed.as_ref().map(InstanceId::as_str), Some(ID));
        assert!(config(&[]).accept_new_database.is_none());
    }
}
