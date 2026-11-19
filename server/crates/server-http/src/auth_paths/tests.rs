// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

use std::net::SocketAddr;
use std::sync::Arc;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{HeaderMap, Request, StatusCode, header};
use server_domain::credential::NoCredentials;
use tower::ServiceExt;

use super::routes;
use crate::auth::{ExternalOrigin, PeerAddress, Scheme, SecurityContext};
use crate::proxy::TrustedProxies;
use crate::registry::Registry;
use crate::router::{AppOptions, build_with};
use crate::spa::{SpaEntry, SpaFiles};

struct Reply {
    status: StatusCode,
    headers: HeaderMap,
    body: Vec<u8>,
}

fn router() -> Router {
    let mut registry = Registry::new();
    for route in routes() {
        registry.register(route).unwrap();
    }
    let context = SecurityContext::new(
        ExternalOrigin::new(Scheme::Https, "app.example.test", None).unwrap(),
        TrustedProxies::default(),
        Arc::new(NoCredentials),
    );
    let shell = SpaFiles::new(vec![SpaEntry {
        name: "index.html".to_owned(),
        bytes: b"<!doctype html><title>app</title>".to_vec(),
        sha256: "AAAAindex=".to_owned(),
    }])
    .unwrap();
    let options = AppOptions {
        spa: Some(Arc::new(shell)),
        ..AppOptions::default()
    };
    build_with(registry, context, &options).into_router()
}

fn send(method: &str, target: &str) -> Reply {
    let router = router();
    let request = Request::builder()
        .method(method)
        .uri(target)
        .extension(PeerAddress(SocketAddr::from(([192, 0, 2, 1], 4000))))
        .body(Body::empty())
        .unwrap();
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(async move {
            let response = router.oneshot(request).await.unwrap();
            let (parts, body) = response.into_parts();
            let bytes = to_bytes(body, usize::MAX).await.unwrap();
            Reply {
                status: parts.status,
                headers: parts.headers,
                body: bytes.to_vec(),
            }
        })
}

fn header_of<'a>(reply: &'a Reply, name: &str) -> &'a str {
    reply
        .headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
}

fn code(reply: &Reply) -> String {
    let value: serde_json::Value = serde_json::from_slice(&reply.body).unwrap();
    value["code"].as_str().unwrap().to_owned()
}

fn location(query: &str) -> String {
    let reply = send("GET", &format!("/auth/callback/oidc{query}"));
    assert_eq!(reply.status, StatusCode::TEMPORARY_REDIRECT, "{query}");
    header_of(&reply, "location").to_owned()
}

#[test]
fn the_callback_keeps_the_query_byte_for_byte() {
    let query = "?code=a%2Fb&state=x+y&state=%C3%A4%26&redirect=a%3Db&empty=&flag&code=2";
    assert_eq!(
        location(query),
        format!("/api/v1/auth/oidc/callback{query}")
    );
    assert_eq!(
        location("?a=%2f&b=%2F&c=%252F"),
        "/api/v1/auth/oidc/callback?a=%2f&b=%2F&c=%252F"
    );
}

#[test]
fn the_callback_without_a_query_has_no_question_mark() {
    assert_eq!(location(""), "/api/v1/auth/oidc/callback");
    assert_eq!(location("?"), "/api/v1/auth/oidc/callback");
}

#[test]
fn the_callback_answer_is_not_cached_and_not_a_page() {
    let reply = send("GET", "/auth/callback/oidc?code=1&state=2");
    assert_eq!(reply.status, StatusCode::TEMPORARY_REDIRECT);
    assert_eq!(header_of(&reply, "cache-control"), "no-store");
    assert_eq!(header_of(&reply, "referrer-policy"), "no-referrer");
    assert_eq!(header_of(&reply, "x-content-type-options"), "nosniff");
    assert!(
        !header_of(&reply, header::CONTENT_TYPE.as_str()).contains("html"),
        "a redirect is never a page"
    );
    assert!(reply.body.is_empty());
}

#[test]
fn other_methods_on_the_callback_get_the_method_error() {
    for method in ["POST", "PUT", "PATCH", "DELETE"] {
        let reply = send(method, "/auth/callback/oidc?code=1");
        assert_eq!(reply.status, StatusCode::METHOD_NOT_ALLOWED, "{method}");
        assert_eq!(code(&reply), "method_not_allowed");
        assert!(header_of(&reply, "allow").contains("GET"));
        assert_eq!(header_of(&reply, "content-type"), "application/json");
        assert!(reply.headers.get(header::LOCATION).is_none());
    }
}

#[test]
fn sign_in_and_sign_out_answer_with_the_api_error() {
    for path in ["/auth/oidc", "/auth/signout"] {
        for target in [path.to_owned(), format!("{path}?next=%2Fstore")] {
            let reply = send("GET", &target);
            assert_eq!(reply.status, StatusCode::NOT_IMPLEMENTED, "{target}");
            assert_eq!(code(&reply), "not_implemented");
            assert_eq!(header_of(&reply, "content-type"), "application/json");
            assert_eq!(header_of(&reply, "cache-control"), "no-store");
            assert!(reply.headers.get(header::SET_COOKIE).is_none());
        }
    }
}

#[test]
fn sign_in_and_sign_out_refuse_other_methods() {
    for path in ["/auth/oidc", "/auth/signout"] {
        for method in ["POST", "PUT", "PATCH", "DELETE"] {
            let reply = send(method, path);
            assert_eq!(
                reply.status,
                StatusCode::METHOD_NOT_ALLOWED,
                "{method} {path}"
            );
            assert_eq!(code(&reply), "method_not_allowed");
        }
    }
}

#[test]
fn the_three_routes_are_public_gets_in_the_inventory() {
    let mut registry = Registry::new();
    for route in routes() {
        registry.register(route).unwrap();
    }
    let inventory = registry.inventory();
    let listed: Vec<(&str, &str)> = inventory
        .iter()
        .map(|info| (info.method, info.path.as_str()))
        .collect();
    assert_eq!(
        listed,
        [
            ("GET", "/auth/callback/oidc"),
            ("GET", "/auth/oidc"),
            ("GET", "/auth/signout"),
        ]
    );
    assert!(
        inventory
            .iter()
            .all(|info| info.policy.public_reason().is_some())
    );
}
