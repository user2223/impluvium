// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! The PostgreSQL versions the server supports.
//!
//! The server is built and tested against the major versions 16, 17 and 18. Older majors are
//! refused before anything is changed. A newer major only produces a warning, because PostgreSQL
//! keeps its SQL interface stable between majors and the operator may have good reasons to run it.

use std::str::FromStr;
use std::time::Duration;

use sqlx::Connection;
use sqlx::postgres::{PgConnectOptions, PgConnection};

use crate::error::connect_failure;
use crate::{ConnectFailure, DatabaseUrl, DbError};

/// The oldest supported major version.
pub const OLDEST_SUPPORTED_MAJOR: u32 = 16;
/// The newest major version the server was tested with.
pub const NEWEST_TESTED_MAJOR: u32 = 18;

/// Longest wait for the connection of the version check.
const CONNECT_LIMIT: Duration = Duration::from_secs(30);

/// What the version of the database server means for the start.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VersionVerdict {
    /// A tested major version.
    Supported { major: u32 },
    /// A major version newer than the tested ones: start, but warn.
    Newer { major: u32 },
    /// A major version older than the supported ones: refuse.
    TooOld { major: u32 },
}

/// Judges the value of the `server_version_num` setting, which is the major version times
/// 10000 plus the minor version (for example 160004 or 180001). Returns `None` for a value that
/// cannot be a version.
pub fn judge_server_version(server_version_num: i64) -> Option<VersionVerdict> {
    if server_version_num <= 0 {
        return None;
    }
    let major = u32::try_from(server_version_num / 10_000).ok()?;
    Some(if major < OLDEST_SUPPORTED_MAJOR {
        VersionVerdict::TooOld { major }
    } else if major > NEWEST_TESTED_MAJOR {
        VersionVerdict::Newer { major }
    } else {
        VersionVerdict::Supported { major }
    })
}

/// Connects once and reads `server_version_num`. The connection is closed again.
pub async fn read_server_version_num(url: &DatabaseUrl) -> Result<i64, DbError> {
    let options = PgConnectOptions::from_str(url.expose())
        .map_err(|_| DbError::InvalidUrl)?
        .application_name("impluvium-server-version");
    let mut connection = within(CONNECT_LIMIT, async {
        PgConnection::connect_with(&options)
            .await
            .map_err(|error| DbError::Connect(connect_failure(&error)))
    })
    .await?;
    let value = within(CONNECT_LIMIT, async {
        sqlx::query_scalar::<_, i32>("SELECT current_setting('server_version_num')::int4")
            .fetch_one(&mut connection)
            .await
            .map_err(|error| DbError::from_query(&error))
    })
    .await;
    let _ = tokio::time::timeout(CONNECT_LIMIT, connection.close()).await;
    value.map(i64::from)
}

/// Runs a step with a time limit; a step that does not finish counts as a timed-out connection.
async fn within<T>(
    limit: Duration,
    step: impl std::future::Future<Output = Result<T, DbError>>,
) -> Result<T, DbError> {
    match tokio::time::timeout(limit, step).await {
        Ok(result) => result,
        Err(_) => Err(DbError::Connect(ConnectFailure::Timeout)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tested_majors_are_supported() {
        for (number, major) in [(160_000, 16), (160_004, 16), (170_002, 17), (180_001, 18)] {
            assert_eq!(
                judge_server_version(number),
                Some(VersionVerdict::Supported { major })
            );
        }
    }

    #[test]
    fn older_majors_are_too_old() {
        for (number, major) in [(150_012, 15), (140_000, 14), (100_001, 10), (90_624, 9)] {
            assert_eq!(
                judge_server_version(number),
                Some(VersionVerdict::TooOld { major })
            );
        }
    }

    #[test]
    fn newer_majors_only_warn() {
        assert_eq!(
            judge_server_version(190_000),
            Some(VersionVerdict::Newer { major: 19 })
        );
    }

    #[test]
    fn a_step_that_does_not_finish_in_time_is_a_timeout() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .unwrap();
        let result = runtime.block_on(within(
            Duration::from_millis(20),
            std::future::pending::<Result<(), DbError>>(),
        ));
        assert!(matches!(
            result,
            Err(DbError::Connect(ConnectFailure::Timeout))
        ));
        let done = runtime.block_on(within(Duration::from_secs(5), async { Ok(7) }));
        assert!(matches!(done, Ok(7)));
    }

    #[test]
    fn impossible_numbers_are_rejected() {
        assert_eq!(judge_server_version(-1), None);
        assert_eq!(judge_server_version(0), None);
    }
}
