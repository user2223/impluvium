// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Database adapters implementing the application ports.
//!
//! This crate owns the one PostgreSQL connection pool of the server and the
//! embedded schema migrations. It is the only product crate that reaches sqlx; the development tool `server-xtask` uses it too.
//!
//! The pool connects without TLS. Reaching a database over an untrusted network
//! needs a later, explicit decision about TLS support.

mod error;
mod migrations;
mod pool;
mod repository;

pub use error::{ConnectFailure, DbError, MigrateFailure};
pub use migrations::{MIGRATOR, migrate};
pub use pool::{Database, DatabaseUrl, PoolOptions};
pub use repository::{
    ContentKeyBinding, Initialisation, InitialisationState, InstanceIdentity, TicketKeyGeneration,
};
pub use server_domain as domain;
