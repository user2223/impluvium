// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! The embedded schema migrations.
//!
//! The files live in `server/migrations` and are compiled into the binary. A
//! migration that was applied once is never edited: the database keeps its
//! checksum, and a changed file makes [`migrate`] fail instead of running.

use sqlx::PgPool;
use sqlx::migrate::Migrator;

use crate::error::DbError;

/// All migrations of this build, in version order.
pub static MIGRATOR: Migrator = sqlx::migrate!("../../migrations");

/// Applies the migrations that are not applied yet. Running it again changes
/// nothing. It refuses a database whose applied history differs from this build.
pub async fn migrate(pool: &PgPool) -> Result<(), DbError> {
    MIGRATOR.run(pool).await.map_err(DbError::from)
}
