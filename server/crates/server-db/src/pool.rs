// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! The one connection pool of the server.

use std::fmt;
use std::str::FromStr;
use std::time::Duration;

use sqlx::PgPool;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};

use crate::error::{DbError, connect_failure};

/// A PostgreSQL connection URL. It usually carries the password, so neither
/// `Debug` nor `Display` shows anything of it.
#[derive(Clone)]
pub struct DatabaseUrl(String);

impl DatabaseUrl {
    /// Wraps a URL. It is parsed when the pool connects.
    pub fn new(url: impl Into<String>) -> Self {
        Self(url.into())
    }
}

impl fmt::Debug for DatabaseUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("DatabaseUrl(<redacted>)")
    }
}

impl fmt::Display for DatabaseUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<redacted>")
    }
}

/// Explicit settings of the pool.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PoolOptions {
    /// Largest number of open connections. At least one.
    pub max_connections: u32,
    /// How long a caller waits for a free connection.
    pub acquire_timeout: Duration,
    /// Longest running time of one statement, applied to every connection.
    /// Whole milliseconds, at least one, at most `i32::MAX`.
    pub statement_timeout: Duration,
    /// Name shown in `pg_stat_activity`: printable ASCII, at most 63 bytes.
    pub application_name: String,
}

impl Default for PoolOptions {
    fn default() -> Self {
        Self {
            max_connections: 10,
            acquire_timeout: Duration::from_secs(5),
            statement_timeout: Duration::from_secs(30),
            application_name: "impluvium-server".to_owned(),
        }
    }
}

impl PoolOptions {
    fn validate(&self) -> Result<(), DbError> {
        if self.max_connections == 0 {
            return Err(DbError::InvalidOption("max_connections"));
        }
        if self.acquire_timeout.is_zero() {
            return Err(DbError::InvalidOption("acquire_timeout"));
        }
        let millis = self.statement_timeout.as_millis();
        if millis == 0 || millis > i32::MAX as u128 {
            return Err(DbError::InvalidOption("statement_timeout"));
        }
        let name = &self.application_name;
        if name.is_empty() || name.len() > 63 || !name.bytes().all(|b| (0x21..=0x7e).contains(&b)) {
            return Err(DbError::InvalidOption("application_name"));
        }
        Ok(())
    }
}

/// The server's database handle: one pool, cloned cheaply for every user.
#[derive(Clone)]
pub struct Database {
    pool: PgPool,
}

impl fmt::Debug for Database {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Database { .. }")
    }
}

impl Database {
    /// Opens the pool and checks that one connection can be made.
    ///
    /// Failures are reported as [`DbError`] values that name neither the URL nor
    /// the host nor the driver's own message.
    pub async fn connect(url: &DatabaseUrl, options: &PoolOptions) -> Result<Self, DbError> {
        options.validate()?;
        let connect = PgConnectOptions::from_str(&url.0)
            .map_err(|_| DbError::InvalidUrl)?
            .application_name(&options.application_name)
            .options([(
                "statement_timeout",
                options.statement_timeout.as_millis().to_string(),
            )]);
        let pool = PgPoolOptions::new()
            .max_connections(options.max_connections)
            .acquire_timeout(options.acquire_timeout)
            .connect_with(connect)
            .await
            .map_err(|error| DbError::Connect(connect_failure(&error)))?;
        Ok(Self { pool })
    }

    /// The pool, for the adapters in this crate and for tests.
    ///
    /// Do not log or print the returned pool's connect options (`connect_options()`): their
    /// `Debug` output includes the password.
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Applies the embedded migrations that are not applied yet.
    pub async fn migrate(&self) -> Result<(), DbError> {
        crate::migrate(&self.pool).await
    }

    /// Closes every connection and waits until they are gone.
    pub async fn close(&self) {
        self.pool.close().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MARKER: &str = "pw-marker-9f3a";

    fn secret_url() -> DatabaseUrl {
        DatabaseUrl::new(format!("postgres://user:{MARKER}@db.example.test:1/name"))
    }

    #[test]
    fn the_url_shows_nothing_of_itself() {
        let url = secret_url();
        for text in [format!("{url:?}"), format!("{url}"), format!("{url:#?}")] {
            assert!(
                !text.contains(MARKER) && !text.contains("db.example"),
                "{text}"
            );
        }
    }

    #[test]
    fn options_outside_their_range_are_rejected_by_name() {
        let bad = |change: fn(&mut PoolOptions)| {
            let mut options = PoolOptions::default();
            change(&mut options);
            match options.validate() {
                Err(DbError::InvalidOption(name)) => name,
                other => panic!("expected an invalid option, got {other:?}"),
            }
        };
        assert!(PoolOptions::default().validate().is_ok());
        assert_eq!(bad(|o| o.max_connections = 0), "max_connections");
        assert_eq!(
            bad(|o| o.acquire_timeout = Duration::ZERO),
            "acquire_timeout"
        );
        assert_eq!(
            bad(|o| o.statement_timeout = Duration::from_micros(10)),
            "statement_timeout"
        );
        assert_eq!(
            bad(|o| o.statement_timeout = Duration::from_millis(u64::from(u32::MAX))),
            "statement_timeout"
        );
        assert_eq!(
            bad(|o| o.application_name = String::new()),
            "application_name"
        );
        assert_eq!(
            bad(|o| o.application_name = "a b".to_owned()),
            "application_name"
        );
        assert_eq!(
            bad(|o| o.application_name = "x".repeat(64)),
            "application_name"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn an_unparsable_url_fails_without_echoing_it() {
        let url = DatabaseUrl::new(format!("not a url {MARKER}"));
        let error = Database::connect(&url, &PoolOptions::default())
            .await
            .unwrap_err();
        assert!(matches!(error, DbError::InvalidUrl));
        let text = format!("{error} {error:?}");
        assert!(!text.contains(MARKER), "{text}");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn an_unreachable_server_fails_without_url_host_or_password() {
        let url = DatabaseUrl::new(format!("postgres://user:{MARKER}@127.0.0.1:1/name"));
        let options = PoolOptions {
            acquire_timeout: Duration::from_millis(300),
            ..PoolOptions::default()
        };
        let error = Database::connect(&url, &options).await.unwrap_err();
        assert!(matches!(error, DbError::Connect(_)), "{error:?}");
        let text = format!("{error} {error:?}");
        assert!(
            !text.contains(MARKER) && !text.contains("127.0.0.1"),
            "{text}"
        );
    }
}
