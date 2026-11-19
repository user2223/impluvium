// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! HTTP adapters composing application policies with public web contracts.
//!
//! The framework router stays private to this crate. Routes are declared in
//! the [`registry`], which also produces the machine-readable inventory; the
//! [`router`] assembles the framework router from the same definitions.

pub mod auth;
pub mod csrf;
pub mod proxy;
pub mod registry;
pub mod router;

pub use server_api_types as api_types;
pub use server_domain as domain;
