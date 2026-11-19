// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! The central table of configuration fields.
//!
//! Loading, validation and the generated reference all read this table, so a
//! variable cannot be documented without being read or the other way round.

/// How the text of a variable is interpreted and checked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// An origin URL: `http` or `https`, a host and an optional port.
    Origin,
    /// An absolute path.
    Directory,
    /// A comma-separated list of CIDR networks.
    Cidrs,
    /// `true` or `false`.
    Flag,
    /// A whole number within inclusive bounds.
    Number { min: u32, max: u32 },
    /// A `postgres://` or `postgresql://` URL.
    DatabaseUrl,
    /// A comma-separated list of log directives.
    LogFilter,
    /// One of a fixed list of lowercase words.
    Choice(&'static [&'static str]),
    /// A JSON array of strings.
    Arguments,
}

impl Kind {
    /// Short description of the accepted values for the generated reference.
    pub fn describe(&self) -> String {
        match self {
            Self::Origin => "origin URL".to_owned(),
            Self::Directory => "absolute path".to_owned(),
            Self::Cidrs => "comma-separated CIDR networks".to_owned(),
            Self::Flag => "`true` or `false`".to_owned(),
            Self::Number { min, max } => format!("integer from {min} to {max}"),
            Self::DatabaseUrl => "PostgreSQL URL".to_owned(),
            Self::LogFilter => "log filter".to_owned(),
            Self::Choice(words) => {
                let mut quoted = Vec::new();
                for word in words.iter() {
                    quoted.push(format!("`{word}`"));
                }
                format!("one of {}", quoted.join(", "))
            }
            Self::Arguments => "JSON array of strings".to_owned(),
        }
    }
}

/// Whether a field must be configured.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Presence {
    Required,
    Optional,
}

/// What applies when a field is not configured.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fallback {
    /// Nothing is set.
    None,
    /// This text is parsed as if the operator had written it.
    Value(&'static str),
    /// A behaviour that depends on other fields, described in words.
    Described(&'static str),
}

/// One configuration field.
#[derive(Debug, Clone, Copy)]
pub struct Field {
    /// Name of the field in the typed configuration.
    pub key: &'static str,
    /// Name of the environment variable.
    pub variable: &'static str,
    /// Name of the variable that holds a path to a file with the value. Only
    /// secrets have one.
    pub file_variable: Option<&'static str>,
    pub kind: Kind,
    pub presence: Presence,
    pub fallback: Fallback,
    /// One line for the generated reference. It must not contain a pipe.
    pub summary: &'static str,
}

impl Field {
    pub fn is_secret(&self) -> bool {
        self.file_variable.is_some()
    }
}

/// Words accepted for the log format.
pub const LOG_FORMATS: &[&str] = &["text", "json"];
/// Words accepted for the backup mode.
pub const BACKUP_MODES: &[&str] = &["auto", "command", "off"];

pub const FIELDS: &[Field] = &[
    Field {
        key: "external_url",
        variable: "IMPLUVIUM_EXTERNAL_URL",
        file_variable: None,
        kind: Kind::Origin,
        presence: Presence::Required,
        fallback: Fallback::None,
        summary: "Public origin of the server, for example https://impluvium.example.",
    },
    Field {
        key: "data_dir",
        variable: "IMPLUVIUM_DATA_DIR",
        file_variable: None,
        kind: Kind::Directory,
        presence: Presence::Required,
        fallback: Fallback::None,
        summary: "Directory for server data.",
    },
    Field {
        key: "library_dir",
        variable: "IMPLUVIUM_LIBRARY_DIR",
        file_variable: None,
        kind: Kind::Directory,
        presence: Presence::Optional,
        fallback: Fallback::Described("`library` inside the data directory"),
        summary: "Directory that holds the game library.",
    },
    Field {
        key: "trusted_proxies",
        variable: "IMPLUVIUM_TRUSTED_PROXIES",
        file_variable: None,
        kind: Kind::Cidrs,
        presence: Presence::Optional,
        fallback: Fallback::Described("empty; no proxy is trusted"),
        summary: "Networks of reverse proxies whose forwarding headers are trusted.",
    },
    Field {
        key: "insecure_lan",
        variable: "IMPLUVIUM_INSECURE_LAN",
        file_variable: None,
        kind: Kind::Flag,
        presence: Presence::Optional,
        fallback: Fallback::Value("false"),
        summary: "Allows a plain http external origin on a local network.",
    },
    Field {
        key: "database_url",
        variable: "DATABASE_URL",
        file_variable: Some("DATABASE_URL_FILE"),
        kind: Kind::DatabaseUrl,
        presence: Presence::Required,
        fallback: Fallback::None,
        summary: "PostgreSQL connection URL, including the password.",
    },
    Field {
        key: "db_pool_max",
        variable: "IMPLUVIUM_DB_POOL_MAX",
        file_variable: None,
        kind: Kind::Number { min: 1, max: 500 },
        presence: Presence::Optional,
        fallback: Fallback::Value("10"),
        summary: "Largest number of database connections in the pool.",
    },
    Field {
        key: "db_pool_min",
        variable: "IMPLUVIUM_DB_POOL_MIN",
        file_variable: None,
        kind: Kind::Number { min: 0, max: 500 },
        presence: Presence::Optional,
        fallback: Fallback::Value("0"),
        summary: "Number of database connections kept open; at most the maximum.",
    },
    Field {
        key: "db_timeout_secs",
        variable: "IMPLUVIUM_DB_TIMEOUT_SECS",
        file_variable: None,
        kind: Kind::Number { min: 1, max: 300 },
        presence: Presence::Optional,
        fallback: Fallback::Value("30"),
        summary: "Seconds to wait for a free database connection.",
    },
    Field {
        key: "spa_dir",
        variable: "IMPLUVIUM_SPA_DIR",
        file_variable: None,
        kind: Kind::Directory,
        presence: Presence::Optional,
        fallback: Fallback::None,
        summary: "Directory with the built web application.",
    },
    Field {
        key: "log_filter",
        variable: "IMPLUVIUM_LOG",
        file_variable: None,
        kind: Kind::LogFilter,
        presence: Presence::Optional,
        fallback: Fallback::Value("info"),
        summary: "Log filter: a level, or target=level directives separated by commas.",
    },
    Field {
        key: "log_format",
        variable: "IMPLUVIUM_LOG_FORMAT",
        file_variable: None,
        kind: Kind::Choice(LOG_FORMATS),
        presence: Presence::Optional,
        fallback: Fallback::Value("text"),
        summary: "Format of log lines.",
    },
    Field {
        key: "backup_mode",
        variable: "IMPLUVIUM_BACKUP_MODE",
        file_variable: None,
        kind: Kind::Choice(BACKUP_MODES),
        presence: Presence::Optional,
        fallback: Fallback::Value("auto"),
        summary: "How the backup before pending migrations is made.",
    },
    Field {
        key: "backup_command",
        variable: "IMPLUVIUM_BACKUP_COMMAND",
        file_variable: None,
        kind: Kind::Arguments,
        presence: Presence::Optional,
        fallback: Fallback::Described("none; required when the backup mode is `command`"),
        summary: "Operator backup program and arguments, started without a shell.",
    },
    Field {
        key: "backup_timeout_secs",
        variable: "IMPLUVIUM_BACKUP_TIMEOUT_SECS",
        file_variable: None,
        kind: Kind::Number { min: 1, max: 86400 },
        presence: Presence::Optional,
        fallback: Fallback::Value("3600"),
        summary: "Seconds after which a running backup is stopped and counted as failed.",
    },
];
