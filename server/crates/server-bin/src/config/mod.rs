// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Server configuration from environment variables.
//!
//! The table in `fields` describes every variable once. Loading builds a typed
//! `Config` from it and checks the result without touching a database, a
//! listener or any file other than the ones named by `_FILE` variables.
//! `impluvium-server config check` runs exactly this, and
//! `impluvium-server config reference` renders the administrator reference
//! from the same table.
//!
//! Error messages name variables and rules. They never repeat a value.

// Later parts of the server consume items that the commands below do not use.
#![allow(dead_code)]

use std::fmt;
use std::process::ExitCode;

use serde_json::{Map, Value};

mod env;
mod fields;
mod http_settings;
mod model;
mod reference;
mod secret;

pub(crate) use env::Source;
use fields::{FIELDS, Fallback, Field, Presence};
pub(crate) use model::{BackupMode, Cidr, Config, ExternalUrl, LogFormat, Scheme};

/// One problem with one variable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue {
    pub variable: String,
    pub message: String,
}

impl Issue {
    pub fn new(variable: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            variable: variable.into(),
            message: message.into(),
        }
    }
}

impl fmt::Display for Issue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.variable, self.message)
    }
}

/// Every problem found in one run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigError {
    issues: Vec<Issue>,
}

impl ConfigError {
    pub fn issues(&self) -> &[Issue] {
        &self.issues
    }
}

impl fmt::Display for ConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, issue) in self.issues.iter().enumerate() {
            if index > 0 {
                writeln!(formatter)?;
            }
            write!(formatter, "{issue}")?;
        }
        Ok(())
    }
}

impl std::error::Error for ConfigError {}

/// Loads and validates the configuration from the process environment.
pub fn load_from_process() -> Result<Config, ConfigError> {
    load(&Source::from_process())
}

/// Loads and validates the configuration from a source.
pub fn load(source: &Source) -> Result<Config, ConfigError> {
    let mut issues = env::unknown_variables(source.present());
    for name in source.not_unicode() {
        issues.push(Issue::new(name, "is not valid UTF-8"));
    }
    let mut object = Map::new();
    for field in FIELDS {
        match value_for(field, source) {
            Ok(Some(value)) => {
                object.insert(field.key.to_owned(), value);
            }
            Ok(None) => {}
            Err(issue) => issues.push(issue),
        }
    }
    if !issues.is_empty() {
        return Err(ConfigError { issues });
    }
    let config: Config = match serde_json::from_value(Value::Object(object)) {
        Ok(config) => config,
        Err(_) => {
            let issue = Issue::new("configuration", "has an unexpected shape");
            return Err(ConfigError {
                issues: vec![issue],
            });
        }
    };
    let issues = cross_check(&config);
    if issues.is_empty() {
        Ok(config)
    } else {
        Err(ConfigError { issues })
    }
}

/// The parsed value of one field, `None` when it stays unset.
fn value_for(field: &Field, source: &Source) -> Result<Option<Value>, Issue> {
    let (raw, origin) = match resolve(field, source)? {
        Some(found) => found,
        None => match (field.fallback, field.presence) {
            (Fallback::Value(text), _) => (text.to_owned(), field.variable),
            (_, Presence::Required) => {
                return Err(Issue::new(field.variable, missing_message(field)));
            }
            _ => return Ok(None),
        },
    };
    match model::parse(field.kind, &raw) {
        Ok(value) => Ok(Some(value)),
        Err(message) => Err(Issue::new(origin, message)),
    }
}

fn missing_message(field: &Field) -> String {
    match field.file_variable {
        Some(file) => format!("is required; set it or set {file}"),
        None => "is required".to_owned(),
    }
}

/// The text of a field and the name of the variable that supplied it.
fn resolve(field: &Field, source: &Source) -> Result<Option<(String, &'static str)>, Issue> {
    let direct = source.get(field.variable);
    let mut file = None;
    if let Some(name) = field.file_variable
        && let Some(path) = source.get(name)
    {
        file = Some((name, path));
    }
    match (direct, file) {
        (Some(_), Some((name, _))) => {
            let message = format!("is set together with {name}; set only one of them");
            Err(Issue::new(field.variable, message))
        }
        (Some(value), None) => Ok(Some((value.to_owned(), field.variable))),
        (None, Some((name, path))) => match secret::read_file(path) {
            Ok(text) => Ok(Some((text, name))),
            Err(message) => Err(Issue::new(name, message)),
        },
        (None, None) => Ok(None),
    }
}

/// Rules that involve more than one field.
fn cross_check(config: &Config) -> Vec<Issue> {
    let mut issues = Vec::new();
    if config.db_pool_min > config.db_pool_max {
        issues.push(Issue::new(
            "IMPLUVIUM_DB_POOL_MIN",
            "must not exceed IMPLUVIUM_DB_POOL_MAX",
        ));
    }
    let url = &config.external_url;
    if url.scheme == Scheme::Http && !url.is_loopback() && !config.insecure_lan {
        issues.push(Issue::new(
            "IMPLUVIUM_EXTERNAL_URL",
            "uses http:// for a host that is not this machine; \
             use https:// or set IMPLUVIUM_INSECURE_LAN to true",
        ));
    }
    match (config.backup_mode, config.backup_command.is_empty()) {
        (BackupMode::Command, true) => issues.push(Issue::new(
            "IMPLUVIUM_BACKUP_COMMAND",
            "is required when IMPLUVIUM_BACKUP_MODE is command",
        )),
        (BackupMode::Command, false) | (_, true) => {}
        (_, false) => issues.push(Issue::new(
            "IMPLUVIUM_BACKUP_COMMAND",
            "is only allowed when IMPLUVIUM_BACKUP_MODE is command",
        )),
    }
    issues
}

/// The result of `config check` for a source: a success text or an error text.
pub fn check_report(source: &Source) -> Result<String, String> {
    match load(source) {
        Ok(config) => {
            let json = serde_json::to_string_pretty(&config).unwrap_or_default();
            Ok(format!(
                "configuration is valid; secrets are not shown\n{json}"
            ))
        }
        Err(error) => Err(format!("configuration is invalid\n{error}")),
    }
}

/// `impluvium-server config check`: validates the environment and stops. It
/// starts no listener, opens no database connection and runs no migration.
pub fn run_check() -> ExitCode {
    match check_report(&Source::from_process()) {
        Ok(text) => {
            println!("{text}");
            ExitCode::SUCCESS
        }
        Err(text) => {
            eprintln!("{text}");
            ExitCode::FAILURE
        }
    }
}

/// `impluvium-server config reference`: prints the administrator reference.
pub fn run_reference() -> ExitCode {
    print!("{}", reference::render());
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::secret::TempDir;
    use super::*;

    const MARKER: &str = "zz-value-marker";
    const MINIMAL: [(&str, &str); 3] = [
        ("IMPLUVIUM_EXTERNAL_URL", "https://impluvium.example"),
        ("IMPLUVIUM_DATA_DIR", "/srv/impluvium/data"),
        (
            "DATABASE_URL",
            "postgres://impluvium:pw@db.example/impluvium",
        ),
    ];

    /// The minimal configuration without the variables in `without`, plus `extra`.
    fn source<'a>(without: &[&str], extra: &[(&'a str, &'a str)]) -> Source {
        let mut pairs: Vec<(&'a str, &'a str)> = Vec::new();
        for (name, value) in MINIMAL {
            let replaced = extra.iter().any(|(other, _)| *other == name);
            if !replaced && !without.contains(&name) {
                pairs.push((name, value));
            }
        }
        pairs.extend_from_slice(extra);
        Source::from_pairs(&pairs)
    }

    fn failing(source: &Source) -> ConfigError {
        load(source).expect_err("the configuration should be rejected")
    }

    fn names(error: &ConfigError) -> Vec<&str> {
        error.issues().iter().map(|i| i.variable.as_str()).collect()
    }

    #[test]
    fn minimal_configuration_loads_with_documented_defaults() {
        let config = load(&source(&[], &[])).unwrap();
        assert_eq!(config.external_url.origin(), "https://impluvium.example");
        assert_eq!(
            config.library_dir(),
            std::path::Path::new("/srv/impluvium/data/library")
        );
        assert!(config.trusted_proxies.is_empty());
        assert!(!config.insecure_lan);
        assert_eq!((config.db_pool_max, config.db_pool_min), (10, 0));
        assert_eq!(config.db_timeout_secs, 30);
        assert!(config.spa_dir.is_none());
        assert_eq!(config.log_filter, "info");
        assert_eq!(config.backup_mode, BackupMode::Auto);
        assert_eq!(config.backup_timeout_secs, 3600);
        assert!(config.backup_command.is_empty());
        assert_eq!(config.listen_addr.to_string(), "127.0.0.1:8080");
        assert!(config.accept_new_database.is_none());
        assert!(check_report(&source(&[], &[])).is_ok());
    }

    #[test]
    fn every_option_can_be_set() {
        let extra = [
            ("IMPLUVIUM_LIBRARY_DIR", "/srv/impluvium/games"),
            ("IMPLUVIUM_TRUSTED_PROXIES", "10.0.0.0/8, fd00::/8"),
            ("IMPLUVIUM_DB_POOL_MAX", "20"),
            ("IMPLUVIUM_DB_POOL_MIN", "2"),
            ("IMPLUVIUM_DB_TIMEOUT_SECS", "5"),
            ("IMPLUVIUM_SPA_DIR", "/srv/impluvium/web"),
            ("IMPLUVIUM_LOG", "warn,server_http=debug"),
            ("IMPLUVIUM_LOG_FORMAT", "json"),
            ("IMPLUVIUM_BACKUP_MODE", "command"),
            (
                "IMPLUVIUM_BACKUP_COMMAND",
                r#"["/usr/local/bin/backup", "--quiet"]"#,
            ),
            ("IMPLUVIUM_BACKUP_TIMEOUT_SECS", "7200"),
            ("IMPLUVIUM_LISTEN", "[::1]:9000"),
            (
                "IMPLUVIUM_ACCEPT_NEW_DATABASE",
                "0190a1b2-c3d4-4e5f-8a9b-0c1d2e3f4a5b",
            ),
        ];
        let config = load(&source(&[], &extra)).unwrap();
        assert_eq!(
            config.library_dir(),
            std::path::Path::new("/srv/impluvium/games")
        );
        assert_eq!(config.trusted_proxies.len(), 2);
        assert_eq!((config.db_pool_max, config.db_pool_min), (20, 2));
        assert_eq!(config.backup_command, ["/usr/local/bin/backup", "--quiet"]);
        assert_eq!(config.backup_timeout_secs, 7200);
        assert_eq!(config.listen_addr.to_string(), "[::1]:9000");
        assert_eq!(
            config.accept_new_database.as_deref(),
            Some("0190a1b2-c3d4-4e5f-8a9b-0c1d2e3f4a5b")
        );
    }

    #[test]
    fn listen_address_and_instance_identifier_are_checked() {
        for pair in [
            ("IMPLUVIUM_LISTEN", "8080"),
            ("IMPLUVIUM_LISTEN", "localhost:8080"),
            ("IMPLUVIUM_LISTEN", "127.0.0.1:0"),
            ("IMPLUVIUM_ACCEPT_NEW_DATABASE", "not-an-identifier"),
            (
                "IMPLUVIUM_ACCEPT_NEW_DATABASE",
                "0190A1B2-C3D4-4E5F-8A9B-0C1D2E3F4A5B",
            ),
        ] {
            assert_eq!(names(&failing(&source(&[], &[pair]))), [pair.0]);
        }
    }

    #[test]
    fn missing_required_values_are_all_reported() {
        let error = failing(&Source::from_pairs(&[]));
        assert_eq!(
            names(&error),
            [
                "IMPLUVIUM_EXTERNAL_URL",
                "IMPLUVIUM_DATA_DIR",
                "DATABASE_URL"
            ]
        );
        assert!(error.issues()[2].message.contains("DATABASE_URL_FILE"));
    }

    #[test]
    fn invalid_values_name_the_variable_and_never_repeat_the_value() {
        for field in FIELDS {
            let error = failing(&source(&[], &[(field.variable, MARKER)]));
            assert!(
                names(&error).contains(&field.variable),
                "{}",
                field.variable
            );
            assert!(
                !error.to_string().contains(MARKER),
                "{} leaked",
                field.variable
            );
        }
    }

    #[test]
    fn pool_limits_are_checked() {
        for pair in [
            ("IMPLUVIUM_DB_POOL_MAX", "0"),
            ("IMPLUVIUM_DB_POOL_MAX", "501"),
            ("IMPLUVIUM_DB_POOL_MIN", "-1"),
            ("IMPLUVIUM_DB_TIMEOUT_SECS", "0"),
            ("IMPLUVIUM_DB_TIMEOUT_SECS", "301"),
            ("IMPLUVIUM_BACKUP_TIMEOUT_SECS", "0"),
            ("IMPLUVIUM_BACKUP_TIMEOUT_SECS", "86401"),
        ] {
            assert_eq!(names(&failing(&source(&[], &[pair]))), [pair.0]);
        }
        let error = failing(&source(&[], &[("IMPLUVIUM_DB_POOL_MIN", "11")]));
        assert_eq!(names(&error), ["IMPLUVIUM_DB_POOL_MIN"]);
    }

    #[test]
    fn plain_http_needs_the_lan_switch_unless_the_host_is_local() {
        let http = ("IMPLUVIUM_EXTERNAL_URL", "http://impluvium.example");
        assert_eq!(names(&failing(&source(&[], &[http]))), [http.0]);
        let lan = [http, ("IMPLUVIUM_INSECURE_LAN", "true")];
        assert!(load(&source(&[], &lan)).is_ok());
        for local in [
            "http://localhost:3000",
            "http://127.0.0.1",
            "http://[::1]:8080",
        ] {
            let extra = [("IMPLUVIUM_EXTERNAL_URL", local)];
            assert!(load(&source(&[], &extra)).is_ok(), "{local}");
        }
        let proxied = [
            ("IMPLUVIUM_EXTERNAL_URL", "https://impluvium.example"),
            ("IMPLUVIUM_TRUSTED_PROXIES", "192.0.2.10/32"),
        ];
        assert!(load(&source(&[], &proxied)).is_ok());
    }

    #[test]
    fn backup_command_and_mode_must_agree() {
        let command = ("IMPLUVIUM_BACKUP_MODE", "command");
        assert_eq!(
            names(&failing(&source(&[], &[command]))),
            ["IMPLUVIUM_BACKUP_COMMAND"]
        );
        let stray = ("IMPLUVIUM_BACKUP_COMMAND", r#"["backup"]"#);
        assert_eq!(names(&failing(&source(&[], &[stray]))), [stray.0]);
        assert!(load(&source(&[], &[("IMPLUVIUM_BACKUP_MODE", "off")])).is_ok());
    }

    #[test]
    fn secrets_come_from_the_variable_or_from_a_file_but_not_both() {
        let directory = TempDir::new("both");
        let path = directory.write("url", b"postgres://u:file-secret@db.example/x\n");
        let from_file = source(&["DATABASE_URL"], &[("DATABASE_URL_FILE", path.as_str())]);
        let config = load(&from_file).unwrap();
        assert_eq!(
            config.database_url.expose(),
            "postgres://u:file-secret@db.example/x"
        );
        let both = source(&[], &[("DATABASE_URL_FILE", path.as_str())]);
        let error = failing(&both);
        assert_eq!(names(&error), ["DATABASE_URL"]);
        assert!(error.to_string().contains("only one"));
    }

    #[test]
    fn unusable_secret_files_are_rejected_without_showing_their_content() {
        let directory = TempDir::new("unusable");
        let bad_content = directory.write("bad", MARKER.as_bytes());
        let big = vec![b'x'; 70_000];
        let too_big = directory.write("big", &big);
        let missing = "/nonexistent-impluvium-dir/url";
        for path in [bad_content.as_str(), too_big.as_str(), missing] {
            let error = failing(&source(&["DATABASE_URL"], &[("DATABASE_URL_FILE", path)]));
            assert_eq!(names(&error), ["DATABASE_URL_FILE"]);
            let text = error.to_string();
            assert!(!text.contains(MARKER) && !text.contains("impluvium-config"));
        }
    }

    #[test]
    fn unknown_product_variables_are_errors_with_a_name_hint() {
        let typo = FIELDS[0].variable.replace("EXTERNAL", "EXTRNAL");
        let error = failing(&source(&[], &[(typo.as_str(), "x")]));
        assert_eq!(names(&error), [typo.as_str()]);
        assert!(error.to_string().contains(FIELDS[0].variable));
        // Names of planned features are unknown until the server reads them.
        let planned = [("IMPLUVIUM_OIDC_CLIENT_ID", "x")];
        assert_eq!(names(&failing(&source(&[], &planned))), [planned[0].0]);
        // Old names and build-only names are not product variables here.
        let other = [("EXTERNAL_URL", "x"), ("IMPLUVIUM_VERSION", "1")];
        assert!(load(&source(&[], &other)).is_ok());
    }

    #[test]
    fn the_check_report_shows_no_backup_arguments() {
        let command = r#"["/usr/local/bin/backup", "--password=arg-marker", "second"]"#;
        let extra = [
            ("IMPLUVIUM_BACKUP_MODE", "command"),
            ("IMPLUVIUM_BACKUP_COMMAND", command),
        ];
        let report = check_report(&source(&[], &extra)).unwrap();
        assert!(report.contains("/usr/local/bin/backup"));
        assert!(!report.contains("arg-marker") && !report.contains("second"));
        assert_eq!(report.matches("<redacted>").count(), 3);
    }

    #[test]
    fn mapped_ipv4_networks_are_refused_as_trusted_proxies() {
        let extra = [("IMPLUVIUM_TRUSTED_PROXIES", "::ffff:0:0/96")];
        let error = failing(&source(&[], &extra));
        assert_eq!(names(&error), ["IMPLUVIUM_TRUSTED_PROXIES"]);
        assert!(
            error.to_string().contains("IPv4 notation") || error.to_string().contains("as IPv4")
        );
    }

    #[test]
    fn the_check_report_hides_secrets_and_lists_every_problem() {
        let report = check_report(&source(&[], &[])).unwrap();
        assert!(report.contains("<redacted>"));
        assert!(!report.contains("postgres://") && !report.contains(":pw@"));
        let broken = source(
            &[],
            &[("IMPLUVIUM_LOG", MARKER), ("IMPLUVIUM_DB_POOL_MAX", MARKER)],
        );
        let text = check_report(&broken).unwrap_err();
        assert!(text.contains("IMPLUVIUM_LOG") && text.contains("IMPLUVIUM_DB_POOL_MAX"));
        assert!(!text.contains(MARKER));
    }

    #[test]
    fn every_variable_is_in_the_environment_register() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../../tools/env-register.json"
        );
        let text = std::fs::read_to_string(path).expect("read the environment register");
        let register: Value = serde_json::from_str(&text).expect("parse the register");
        let entries = register["entries"].as_array().expect("register entries");
        let entry = |name: &str| entries.iter().find(|e| e["name"] == name);
        for field in FIELDS {
            let item =
                entry(field.variable).unwrap_or_else(|| panic!("{} missing", field.variable));
            assert_eq!(item["status"], "active", "{}", field.variable);
            assert_eq!(item["evidence"], "read", "{}", field.variable);
            assert_eq!(item["secret"], field.is_secret(), "{}", field.variable);
            if let Some(file) = field.file_variable {
                let item = entry(file).unwrap_or_else(|| panic!("{file} missing"));
                assert_eq!(item["status"], "active", "{file}");
                assert_eq!(item["secret"], false, "{file}");
            }
        }
    }
}
