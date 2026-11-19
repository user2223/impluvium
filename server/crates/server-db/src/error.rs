// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Errors of the database layer.
//!
//! Messages never contain the connection URL, a password, a host name or the
//! text of a driver error, because those can echo connection details.

use std::error::Error;
use std::fmt;

/// Coarse reason a connection could not be established.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConnectFailure {
    /// The server refused the credentials or the user may not connect.
    Authentication,
    /// The server could not be reached or the connection broke.
    Unreachable,
    /// No connection was available within the acquire timeout.
    Timeout,
    /// Any other failure.
    Other,
}

/// Why a database operation failed.
#[derive(Debug)]
pub enum DbError {
    /// The connection URL is not a usable PostgreSQL URL.
    InvalidUrl,
    /// A pool option is out of range. The text names the option, never a value.
    InvalidOption(&'static str),
    /// The connection could not be established.
    Connect(ConnectFailure),
    /// A query failed or returned a value this version does not understand.
    Query,
    /// Applying the embedded migrations failed or the applied history does not
    /// match them (an applied migration was edited or is unknown to this build).
    Migrate(MigrateFailure),
}

/// Why migrating failed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MigrateFailure {
    /// An already applied migration differs from the embedded one.
    ChecksumMismatch,
    /// The database has applied a migration that this build does not contain.
    UnknownApplied,
    /// A migration was applied only partly earlier and must be repaired by hand.
    Dirty,
    /// Executing or recording a migration failed.
    Execution,
}

impl fmt::Display for DbError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidUrl => f.write_str("the database URL is not a valid PostgreSQL URL"),
            Self::InvalidOption(name) => write!(f, "invalid database pool option: {name}"),
            Self::Connect(ConnectFailure::Authentication) => {
                f.write_str("could not connect to the database: authentication failed")
            }
            Self::Connect(ConnectFailure::Unreachable) => {
                f.write_str("could not connect to the database: server unreachable")
            }
            Self::Connect(ConnectFailure::Timeout) => {
                f.write_str("could not connect to the database: timed out")
            }
            Self::Connect(ConnectFailure::Other) => {
                f.write_str("could not connect to the database")
            }
            Self::Query => f.write_str("a database query failed"),
            Self::Migrate(MigrateFailure::ChecksumMismatch) => {
                f.write_str("an applied migration was changed after it was applied")
            }
            Self::Migrate(MigrateFailure::UnknownApplied) => {
                f.write_str("the database has migrations that this version does not know")
            }
            Self::Migrate(MigrateFailure::Dirty) => {
                f.write_str("a migration was left half applied and needs repair")
            }
            Self::Migrate(MigrateFailure::Execution) => f.write_str("applying migrations failed"),
        }
    }
}

impl Error for DbError {}

/// Classifies a driver error that happened while connecting.
pub(crate) fn connect_failure(error: &sqlx::Error) -> ConnectFailure {
    match error {
        sqlx::Error::PoolTimedOut => ConnectFailure::Timeout,
        sqlx::Error::Io(_) | sqlx::Error::PoolClosed | sqlx::Error::WorkerCrashed => {
            ConnectFailure::Unreachable
        }
        sqlx::Error::Database(database) => match database.code().as_deref() {
            // invalid_authorization_specification, invalid_password
            Some("28000" | "28P01") => ConnectFailure::Authentication,
            _ => ConnectFailure::Other,
        },
        _ => ConnectFailure::Other,
    }
}

impl From<sqlx::migrate::MigrateError> for DbError {
    fn from(error: sqlx::migrate::MigrateError) -> Self {
        use sqlx::migrate::MigrateError as Source;
        Self::Migrate(match error {
            Source::VersionMismatch(_) => MigrateFailure::ChecksumMismatch,
            Source::VersionMissing(_) => MigrateFailure::UnknownApplied,
            Source::Dirty(_) => MigrateFailure::Dirty,
            _ => MigrateFailure::Execution,
        })
    }
}

impl DbError {
    /// Maps a driver error of a query. Connection problems that surface while a query runs (a lost
    /// connection, an exhausted pool) keep their connect classification; everything else is a query failure.
    pub(crate) fn from_query(error: &sqlx::Error) -> Self {
        match error {
            sqlx::Error::PoolTimedOut
            | sqlx::Error::Io(_)
            | sqlx::Error::PoolClosed
            | sqlx::Error::WorkerCrashed => Self::Connect(connect_failure(error)),
            _ => Self::Query,
        }
    }
}
