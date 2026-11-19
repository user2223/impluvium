// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! The routes of the server information, the settings, the first-run setup, sign-in and the
//! administration of sign-in methods, the signed-in account, tokens and the administration of
//! accounts.
//!
//! These routes carry their contracts (who may call, which body, which answer) but no behaviour
//! yet: every handler answers with the API error for a missing implementation and changes
//! nothing. The policies and the request and response shapes are the intended rules, except that
//! how the first-run setup reaches `POST /api/v1/setup`, `GET /api/v1/admin/auth` and
//! `POST /api/v1/admin/auth/invitation` is still open and is decided together with the setup
//! credential. The routes of the signed-in account, of tokens and of the account administration
//! follow the rules their handlers have today; where a rule cannot be written as a policy yet,
//! the module of the route says so.

mod admin_auth;
mod admin_users;
mod auth;
mod server;
mod token;
mod user;

use std::time::Duration;

use axum::response::{IntoResponse, Response};
use server_api_types::ErrorCode;
use server_domain::policy::{Policy, Requirement, SessionKind};

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

/// A signed-in browser session with a finished sign-in. Tokens, the store window and a session
/// that still owes its second factor do not pass; no right is named because no right can say
/// "a browser session".
fn session_only() -> Policy {
    Policy::protected(Requirement::authenticated().require_session(SessionKind::Interactive))
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
        .chain(user::routes())
        .chain(token::routes())
        .chain(admin_users::routes())
        .collect()
}

#[cfg(test)]
mod tests;
