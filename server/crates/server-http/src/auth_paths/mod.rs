// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! The server routes under `/auth`.
//!
//! `GET /auth/callback/oidc` is the address that identity providers were
//! configured with before the API moved; it forwards the browser to the
//! callback of the API and keeps the query exactly as it arrived.
//! `GET /auth/oidc` and `GET /auth/signout` are registered so that they never
//! fall through to the page shell, but they have no behaviour yet: they answer
//! with the API error for a missing implementation and change no state.

use std::time::Duration;

use axum::extract::RawQuery;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use server_api_types::ErrorCode;
use server_domain::policy::Policy;

use crate::error::HttpError;
use crate::registry::{Contract, Limits, Method, ResponseKind, Route};

/// Where the old callback address leads.
const CALLBACK_TARGET: &str = "/api/v1/auth/oidc/callback";

fn limits() -> Limits {
    Limits::new(0, Duration::from_secs(5))
}

/// The redirect of the old callback address. The query is copied byte for
/// byte; an empty query is dropped.
fn callback_redirect(query: Option<&str>) -> Response {
    let mut location = String::from(CALLBACK_TARGET);
    if let Some(query) = query.filter(|query| !query.is_empty()) {
        location.push('?');
        location.push_str(query);
    }
    // The request target parser already refused control characters, space and quotes; a header
    // value additionally refuses DEL. Anything that still fails here is answered with 400.
    let Ok(location) = HeaderValue::from_str(&location) else {
        return HttpError::new(ErrorCode::BadRequest).into_response();
    };
    (
        StatusCode::TEMPORARY_REDIRECT,
        [
            (header::LOCATION, location),
            (header::CACHE_CONTROL, HeaderValue::from_static("no-store")),
        ],
    )
        .into_response()
}

async fn not_implemented() -> Response {
    HttpError::new(ErrorCode::NotImplemented).into_response()
}

/// The three routes under `/auth`.
///
/// # Panics
///
/// Never in practice: paths, reasons and contracts are fixed and valid, which
/// the tests of this module check.
pub fn routes() -> Vec<Route> {
    let public = |reason: &'static str| Policy::public(reason).expect("the reason is not empty");
    let callback = Route::new(
        Method::Get,
        "/auth/callback/oidc",
        public("the old callback address of identity providers; it only redirects"),
        Contract::new(ResponseKind::Redirect, &[307], limits()),
        |RawQuery(query): RawQuery| async move { callback_redirect(query.as_deref()) },
    )
    .expect("the callback route is valid");
    let sign_in = Route::new(
        Method::Get,
        "/auth/oidc",
        public("the start of the single sign-on flow; not implemented yet"),
        Contract::new(ResponseKind::Json, &[200], limits()),
        not_implemented,
    )
    .expect("the sign-in route is valid");
    let sign_out = Route::new(
        Method::Get,
        "/auth/signout",
        public("sign-out entry; not implemented yet and changes nothing"),
        Contract::new(ResponseKind::Json, &[200], limits()),
        not_implemented,
    )
    .expect("the sign-out route is valid");
    vec![callback, sign_in, sign_out]
}

#[cfg(test)]
mod tests;
