// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Errors of the backup building block.
//!
//! Messages name the step and the reason. They never contain the database URL, a
//! password, SQL text, the output of a tool or a path.

use std::error::Error;
use std::fmt;
use std::io::ErrorKind;

use crate::DbError;

/// Why an external program did not do its job.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ToolFailure {
    /// The program was not found.
    Missing,
    /// The operating system could not start it.
    CannotStart(ErrorKind),
    /// It did not finish within the time limit and was killed.
    Timeout,
    /// It was ended by a signal.
    Signal,
    /// It exited with this non-zero status.
    Exit(i32),
    /// It finished but printed something this code does not understand.
    UnexpectedOutput,
}

impl fmt::Display for ToolFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing => f.write_str("was not found"),
            Self::CannotStart(kind) => write!(f, "could not be started: {kind}"),
            Self::Timeout => f.write_str("did not finish within the time limit and was stopped"),
            Self::Signal => f.write_str("was ended by a signal"),
            Self::Exit(code) => write!(f, "exited with status {code}"),
            Self::UnexpectedOutput => f.write_str("printed output that was not understood"),
        }
    }
}

/// Why a backup failed. Any error means that no complete backup exists from
/// this run; sets from earlier runs are untouched.
#[derive(Debug)]
pub enum BackupError {
    /// The database URL cannot be handed to the PostgreSQL tools. The text says
    /// what is wrong without repeating any part of the URL.
    Url(&'static str),
    /// The context is unusable. The text names the problem, never a value.
    InvalidInput(&'static str),
    /// Reading facts from the database failed.
    Database(DbError),
    /// `pg_dump` or `pg_restore` failed.
    Tool {
        tool: &'static str,
        reason: ToolFailure,
    },
    /// The installed `pg_dump` is older than the database server.
    ToolTooOld { tool_major: u32, server_major: u32 },
    /// `pg_dump` exited successfully but wrote nothing.
    EmptyDump,
    /// `pg_restore --list` could not read the table of contents of the dump.
    InvalidDump,
    /// Another backup holds the lock of the backups directory.
    AlreadyRunning,
    /// The operator's backup command failed.
    Operator(ToolFailure),
    /// A file operation failed. `step` says which one; the kind is the only
    /// detail of the operating system error that is kept.
    Io { step: &'static str, kind: ErrorKind },
}

impl fmt::Display for BackupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Url(reason) => {
                write!(f, "the database URL cannot be used for a backup: {reason}")
            }
            Self::InvalidInput(reason) => f.write_str(reason),
            Self::Database(error) => write!(f, "the backup could not read the database: {error}"),
            Self::Tool { tool, reason } => write!(f, "{tool} {reason}"),
            Self::ToolTooOld {
                tool_major,
                server_major,
            } => write!(
                f,
                "pg_dump {tool_major} is older than the database server {server_major}; \
                 install a PostgreSQL client of version {server_major} or newer"
            ),
            Self::EmptyDump => f.write_str("pg_dump produced an empty file"),
            Self::InvalidDump => {
                f.write_str("the dump could not be read back by pg_restore --list")
            }
            Self::AlreadyRunning => f.write_str("another backup is running"),
            Self::Operator(reason) => write!(f, "the operator backup command {reason}"),
            Self::Io { step, kind } => {
                write!(f, "the backup failed while trying to {step}: {kind}")
            }
        }
    }
}

impl Error for BackupError {}

impl From<DbError> for BackupError {
    fn from(error: DbError) -> Self {
        Self::Database(error)
    }
}

/// Builds the closure that maps an I/O error of one step to a [`BackupError`].
pub(crate) fn io(step: &'static str) -> impl FnOnce(std::io::Error) -> BackupError {
    move |error| BackupError::Io {
        step,
        kind: error.kind(),
    }
}
