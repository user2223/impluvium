// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: 0BSD

//! Generated Connect contracts and the protected server entry point.
//!
//! Servers mount `Server::into_axum_router` under `/api/v2`. Registering the
//! generated services directly with the framework bypasses this boundary.

pub use crate::connect_generated::*;

#[cfg(feature = "server")]
mod server;
#[cfg(feature = "server")]
pub use server::*;
