// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! The tokens of an account and of the server.
//!
//! The routes under `/api/v1/user/token` are for a signed-in browser session: a token cannot
//! create or remove tokens. The routes under `/api/v1/admin/token` need the right
//! `admin:manage`, like the other administration routes.
//!
//! `GET /api/v1/token` answers the rights of the credential that asks, and nothing for a caller
//! without one. A policy cannot say "answer nothing" yet, so the route is public and its handler
//! must resolve the credential itself.

use server_api_types::contracts::token::{
    AclDescriptions, ApiTokenList, GrantedAcls, NewApiToken, TokenCreate,
};
use server_domain::policy::Policy;

use super::admin_auth::admin;
use super::{JSON_BODY_LIMIT, limits, not_implemented, public, session_only};
use crate::registry::{BodyKind, Contract, Csrf, Method, ParamType, ResponseKind, Route};

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
    routes.extend(routes_of("/api/v1/admin/token", admin));
    routes.push(
        Route::new(
            Method::Get,
            "/api/v1/token",
            public(
                "lists the rights of the credential of the caller, or null when there is none; the answer is only about the caller itself",
            ),
            Contract::new(ResponseKind::Json, &[200], limits(0)).response_shape::<GrantedAcls>(),
            not_implemented,
        )
        .expect("the granted rights route is valid"),
    );
    routes
}
