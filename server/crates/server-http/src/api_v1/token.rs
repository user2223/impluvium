// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! The tokens of an account and of the server.
//!
//! The routes under `/api/v1/user/token` are for a signed-in browser session: a token cannot
//! create or remove tokens. The routes under `/api/v1/admin/token` need the right
//! `admin:manage` and a signed-in browser session too, so a token that holds `admin:manage`
//! cannot hand out or remove tokens. The answers that carry a token (`POST` on both prefixes) and
//! the answer about the caller (`GET /api/v1/token`) are marked `no_store`.
//!
//! `GET /api/v1/token` answers the rights of the credential that asks. It answers `null` to an
//! anonymous caller and to a session that still owes its second factor; a valid browser
//! session, the store window or a valid API token gets its own rights. A policy cannot say
//! "answer nothing" yet, so the route is public and its handler must resolve the credential
//! itself and apply these rules.
//!
//! Rules for the handlers that come later:
//!
//! - `DELETE /api/v1/user/token/{id}` stays within the tokens of the caller's account and
//!   `DELETE /api/v1/admin/token/{id}` within the tokens the administration lists. Both refuse
//!   an unknown id, and neither deletes a fallback element when the id matches nothing.
//! - The routes of the account's tokens need an account behind the caller; a service token has
//!   none and is rejected.
//! - A personal token may never be granted `admin:manage`, whatever the owner may do. Rights
//!   stored under old names (`read`, ...) need a mapping in the token resolver that replaces the
//!   test table.

use server_api_types::contracts::token::{
    AclDescriptions, ApiTokenList, GrantedAcls, NewApiToken, TokenCreate,
};
use server_domain::policy::{Policy, Requirement, Right, SessionKind};

use super::{JSON_BODY_LIMIT, limits, not_implemented, public, session_only};
use crate::registry::{BodyKind, Contract, Csrf, Method, ParamType, ResponseKind, Route};

/// The right `admin:manage` held by a signed-in browser session; a token that holds the right
/// does not pass.
fn admin_session() -> Policy {
    let right = Right::new("admin:manage").expect("the right name is valid");
    Policy::protected(
        Requirement::authenticated()
            .require_rights(&[right])
            .expect("the list of rights is not empty")
            .require_session(SessionKind::Interactive),
    )
}

fn list() -> Contract {
    Contract::new(ResponseKind::Json, &[200], limits(0)).response_shape::<ApiTokenList>()
}

fn acls() -> Contract {
    Contract::new(ResponseKind::Json, &[200], limits(0)).response_shape::<AclDescriptions>()
}

fn create() -> Contract {
    Contract::new(ResponseKind::Json, &[200], limits(JSON_BODY_LIMIT))
        .request_body(BodyKind::Json)
        .request_shape::<TokenCreate>()
        .response_shape::<NewApiToken>()
        .csrf(Csrf::Required)
        .no_store()
}

fn remove() -> Contract {
    Contract::new(ResponseKind::Empty, &[204], limits(0))
        .path_param("id", ParamType::Uuid)
        .csrf(Csrf::Required)
}

fn routes_of(prefix: &str, policy: fn() -> Policy) -> Vec<Route> {
    let route = |method, path: String, contract| {
        Route::new(method, &path, policy(), contract, not_implemented)
            .expect("the token route is valid")
    };
    vec![
        route(Method::Get, prefix.to_owned(), list()),
        route(Method::Post, prefix.to_owned(), create()),
        route(Method::Delete, format!("{prefix}/{{id}}"), remove()),
        route(Method::Get, format!("{prefix}/acls"), acls()),
    ]
}

pub(super) fn routes() -> Vec<Route> {
    let mut routes = routes_of("/api/v1/user/token", session_only);
    routes.extend(routes_of("/api/v1/admin/token", admin_session));
    routes.push(
        Route::new(
            Method::Get,
            "/api/v1/token",
            public(
                "lists the rights of the credential of the caller, or null when there is none; the answer is only about the caller itself",
            ),
            Contract::new(ResponseKind::Json, &[200], limits(0))
                .response_shape::<GrantedAcls>()
                .no_store(),
            not_implemented,
        )
        .expect("the granted rights route is valid"),
    );
    routes
}
