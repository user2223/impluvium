// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! The routes of the server information, the settings, the first-run setup, sign-in and the
//! administration of sign-in methods.
//!
//! These routes carry their contracts (who may call, which body, which answer) but no behaviour
//! yet: every handler answers with the API error for a missing implementation and changes
//! nothing. The policies and the request and response shapes are the intended rules, except that
//! how the first-run setup reaches `POST /api/v1/setup`, `GET /api/v1/admin/auth` and
//! `POST /api/v1/admin/auth/invitation` is still open and is decided together with the setup
//! credential.

mod admin_auth;
mod auth;
mod server;

use std::time::Duration;

use axum::response::{IntoResponse, Response};
use server_api_types::ErrorCode;
use server_domain::policy::Policy;

use crate::error::HttpError;
use crate::registry::{Limits, Route};

/// Largest JSON body of the routes here, in bytes; passkey answers are the biggest.
const JSON_BODY_LIMIT: u64 = 64 * 1024;

fn limits(max_body_bytes: u64) -> Limits {
    Limits::new(max_body_bytes, Duration::from_secs(10))
}

fn public(reason: &'static str) -> Policy {
    Policy::public(reason).expect("the reason is not empty")
}

async fn not_implemented() -> Response {
    HttpError::new(ErrorCode::NotImplemented).into_response()
}

/// All routes of this module.
///
/// # Panics
///
/// Never in practice: paths, policies and contracts are fixed and valid, which the tests of this
/// module check.
pub fn routes() -> Vec<Route> {
    server::routes()
        .into_iter()
        .chain(auth::routes())
        .chain(admin_auth::routes())
        .collect()
}

#[cfg(test)]
mod tests;
