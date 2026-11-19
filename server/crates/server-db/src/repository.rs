// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Reads of the infrastructure schema: instance identity, the content key binding, ticket key
//! generations and the initialisation state.
//!
//! The queries are checked against the schema when the crate is built. A normal build uses the
//! metadata in the repository (`.sqlx` at the workspace root) and needs no database; after a query
//! or the schema changes, the metadata is regenerated with
//! `cargo xtask sqlx prepare --workspace -- --all-targets --locked -p server-db`.
//!
//! Points in time are returned as microseconds since the Unix epoch, converted by the database, so
//! no date and time library is needed here. Keys are the raw bytes stored in the tables.

use crate::error::DbError;
use crate::pool::Database;

/// The identity and layout version of this instance.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InstanceIdentity {
    /// The instance identifier in its textual form.
    pub instance_id: String,
    pub layout_version: i32,
    pub created_at_micros: i64,
}

/// The public content key this instance is bound to.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContentKeyBinding {
    /// 8 bytes.
    pub key_id: Vec<u8>,
    /// 32 bytes.
    pub public_key: Vec<u8>,
    pub algorithm: String,
    pub created_at_micros: i64,
}

/// One generation of the ticket signing key.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TicketKeyGeneration {
    /// From 1 to `u32::MAX`.
    pub generation: i64,
    /// 8 bytes.
    pub key_id: Vec<u8>,
    pub switch_at_micros: i64,
}

/// How far the first initialisation has come.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InitialisationState {
    Started,
    Completed,
}

/// The recorded state of the first initialisation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Initialisation {
    pub state: InitialisationState,
    pub started_at_micros: i64,
    pub completed_at_micros: Option<i64>,
}

struct InitialisationRow {
    state: String,
    started_at_micros: i64,
    completed_at_micros: Option<i64>,
}

impl Database {
    /// The identity of this instance, or `None` before it was created.
    pub async fn instance_identity(&self) -> Result<Option<InstanceIdentity>, DbError> {
        sqlx::query_as!(
            InstanceIdentity,
            r#"SELECT instance_id::text AS "instance_id!", layout_version AS "layout_version!", (extract(epoch FROM created_at) * 1000000)::bigint AS "created_at_micros!" FROM instance_identity"#
        )
        .fetch_optional(self.pool())
        .await
        .map_err(|error| DbError::from_query(&error))
    }

    /// The active content key binding, or `None` when no key is bound.
    pub async fn active_content_key_binding(&self) -> Result<Option<ContentKeyBinding>, DbError> {
        sqlx::query_as!(
            ContentKeyBinding,
            r#"SELECT key_id AS "key_id!", public_key AS "public_key!", algorithm AS "algorithm!", (extract(epoch FROM created_at) * 1000000)::bigint AS "created_at_micros!" FROM content_key_binding WHERE retired_at IS NULL"#
        )
        .fetch_optional(self.pool())
        .await
        .map_err(|error| DbError::from_query(&error))
    }

    /// All ticket key generations, lowest first.
    pub async fn ticket_key_generations(&self) -> Result<Vec<TicketKeyGeneration>, DbError> {
        sqlx::query_as!(
            TicketKeyGeneration,
            r#"SELECT generation AS "generation!", key_id AS "key_id!", (extract(epoch FROM switch_at) * 1000000)::bigint AS "switch_at_micros!" FROM ticket_key_generation ORDER BY generation"#
        )
        .fetch_all(self.pool())
        .await
        .map_err(|error| DbError::from_query(&error))
    }

    /// The state of the first initialisation, or `None` before it began.
    pub async fn initialisation(&self) -> Result<Option<Initialisation>, DbError> {
        let row = sqlx::query_as!(
            InitialisationRow,
            r#"SELECT state AS "state!", (extract(epoch FROM started_at) * 1000000)::bigint AS "started_at_micros!", (extract(epoch FROM completed_at) * 1000000)::bigint AS "completed_at_micros" FROM instance_initialisation"#
        )
        .fetch_optional(self.pool())
        .await
        .map_err(|error| DbError::from_query(&error))?;
        row.map(|row| {
            let state = match row.state.as_str() {
                "started" => InitialisationState::Started,
                "completed" => InitialisationState::Completed,
                _ => return Err(DbError::Query),
            };
            Ok(Initialisation {
                state,
                started_at_micros: row.started_at_micros,
                completed_at_micros: row.completed_at_micros,
            })
        })
        .transpose()
    }
}
