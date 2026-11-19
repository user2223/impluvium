// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

use std::time::Duration;

use axum::Router;
use axum::body::{Body, Bytes, to_bytes};
use axum::http::{Request, StatusCode, header};
use axum::routing::get;
use server_domain::policy::{Policy, Requirement};
use tower::ServiceExt;

use super::{ApplicationRouter, build, enforce};
use crate::registry::{
    BodyKind, Contract, Csrf, Limits, Method, ParamType, PathError, PathTemplate, Registry,
    RegistryError, ResponseKind, Route,
};

fn limits() -> Limits {
    Limits::new(8, Duration::from_secs(5))
}

fn read_contract() -> Contract {
    Contract::new(ResponseKind::Json, &[200], limits())
}

fn write_contract() -> Contract {
    Contract::new(ResponseKind::Json, &[200], limits())
        .request_body(BodyKind::Json)
        .csrf(Csrf::Required)
}

fn public() -> Policy {
    Policy::public("test route").unwrap()
}

fn protected() -> Policy {
    Policy::protected(Requirement::authenticated())
}

async fn ping() -> &'static str {
    "ping"
}

async fn ping_post() -> &'static str {
    "ping-post"
}

async fn echo_length(body: Bytes) -> String {
    body.len().to_string()
}

fn route(method: Method, path: &str, policy: Policy, contract: Contract) -> Route {
    match method {
        Method::Post => Route::new(method, path, policy, contract, ping_post).unwrap(),
        _ => Route::new(method, path, policy, contract, ping).unwrap(),
    }
}

fn sample() -> Registry {
    let mut registry = Registry::new();
    registry
        .register(route(
            Method::Get,
            "/api/v1/ping",
            public(),
            read_contract(),
        ))
        .unwrap();
    registry
        .register(route(
            Method::Post,
            "/api/v1/ping",
            public(),
            write_contract(),
        ))
        .unwrap();
    registry
        .register(route(
            Method::Get,
            "/api/v1/items/{id}",
            protected(),
            read_contract().path_param("id", ParamType::Integer),
        ))
        .unwrap();
    registry
        .register(
            Route::new(
                Method::Post,
                "/api/v1/upload",
                public(),
                write_contract(),
                echo_length,
            )
            .unwrap(),
        )
        .unwrap();
    registry
}

struct Reply {
    status: StatusCode,
    headers: axum::http::HeaderMap,
    body: String,
}

fn send(router: Router, method: &str, path: &str, body: &'static str) -> Reply {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    runtime.block_on(async move {
        let request = Request::builder()
            .method(method)
            .uri(path)
            .body(Body::from(body))
            .unwrap();
        let response = router.oneshot(request).await.unwrap();
        let (parts, body) = response.into_parts();
        let bytes = to_bytes(body, usize::MAX).await.unwrap();
        Reply {
            status: parts.status,
            headers: parts.headers,
            body: String::from_utf8(bytes.to_vec()).unwrap(),
        }
    })
}

fn app(registry: Registry) -> Router {
    let ApplicationRouter { router } = build(registry);
    router
}

#[test]
fn duplicate_method_and_path_is_rejected() {
    let mut registry = Registry::new();
    registry
        .register(route(
            Method::Get,
            "/api/v1/ping",
            public(),
            read_contract(),
        ))
        .unwrap();
    let error = registry
        .register(route(
            Method::Get,
            "/api/v1/ping",
            public(),
            read_contract(),
        ))
        .unwrap_err();
    assert_eq!(
        error,
        RegistryError::DuplicateRoute {
            method: Method::Get,
            path: "/api/v1/ping".into()
        }
    );
    // The same path with another method is a different key.
    registry
        .register(route(
            Method::Post,
            "/api/v1/ping",
            public(),
            write_contract(),
        ))
        .unwrap();
    assert_eq!(registry.len(), 2);
}

#[test]
fn renamed_path_parameter_conflicts() {
    let mut registry = Registry::new();
    let first = route(
        Method::Get,
        "/a/{id}",
        public(),
        read_contract().path_param("id", ParamType::Integer),
    );
    let second = route(
        Method::Post,
        "/a/{name}",
        public(),
        write_contract().path_param("name", ParamType::String),
    );
    registry.register(first).unwrap();
    assert_eq!(
        registry.register(second).unwrap_err(),
        RegistryError::ConflictingPath {
            existing: "/a/{id}".into(),
            new: "/a/{name}".into()
        }
    );
}

#[test]
fn public_route_without_reason_cannot_be_registered() {
    // The policy value cannot exist without a reason, so no route can be built from it.
    let error = Policy::public("   ").unwrap_err();
    assert_eq!(
        error,
        server_domain::policy::PolicyError::PublicWithoutReason
    );
}

#[test]
fn malformed_route_declarations_name_their_cause() {
    let declare = |method, path: &str, contract| {
        Route::new(method, path, public(), contract, ping).map(|_| ())
    };
    assert_eq!(
        declare(Method::Get, "api", read_contract()),
        Err(RegistryError::InvalidPath(PathError::NotAbsolute))
    );
    assert_eq!(
        declare(Method::Get, "/a/{id}", read_contract()),
        Err(RegistryError::PathParamMismatch("/a/{id}".into()))
    );
    assert_eq!(
        declare(
            Method::Get,
            "/a",
            read_contract().path_param("id", ParamType::Uuid)
        ),
        Err(RegistryError::PathParamMismatch("/a".into()))
    );
    assert_eq!(
        declare(
            Method::Get,
            "/a",
            read_contract()
                .query_param("q", ParamType::String)
                .query_param("q", ParamType::Integer)
        ),
        Err(RegistryError::DuplicateQueryParam("/a".into()))
    );
    for statuses in [&[][..], &[199][..], &[400][..]] {
        assert_eq!(
            declare(
                Method::Get,
                "/a",
                Contract::new(ResponseKind::Json, statuses, limits())
            ),
            Err(RegistryError::InvalidSuccessStatuses("/a".into()))
        );
    }
    assert_eq!(
        declare(
            Method::Get,
            "/a",
            read_contract().request_body(BodyKind::Json)
        ),
        Err(RegistryError::BodyOnSafeMethod("/a".into()))
    );
    assert_eq!(
        declare(
            Method::Get,
            "/a",
            Contract::new(ResponseKind::Json, &[200], Limits::new(8, Duration::ZERO))
        ),
        Err(RegistryError::InvalidLimits("/a".into()))
    );
    assert_eq!(
        declare(
            Method::Post,
            "/a",
            Contract::new(ResponseKind::Json, &[200], limits()).request_body(BodyKind::Json)
        ),
        Err(RegistryError::CsrfMismatch("/a".into()))
    );
    assert_eq!(
        declare(Method::Post, "/a", write_contract().csrf(Csrf::Exempt(" "))),
        Err(RegistryError::CsrfMismatch("/a".into()))
    );
    assert_eq!(
        declare(Method::Get, "/a", read_contract().csrf(Csrf::Required)),
        Err(RegistryError::CsrfMismatch("/a".into()))
    );
}

#[test]
fn inventory_and_router_agree() {
    let registry = sample();
    let inventory = registry.inventory();
    let listed: Vec<(&str, &str)> = inventory
        .iter()
        .map(|info| (info.method, info.path.as_str()))
        .collect();
    assert_eq!(
        listed,
        [
            ("GET", "/api/v1/items/{id}"),
            ("GET", "/api/v1/ping"),
            ("POST", "/api/v1/ping"),
            ("POST", "/api/v1/upload"),
        ]
    );
    assert_eq!(inventory.len(), registry.len());
    let router = app(registry);
    for info in &inventory {
        let path = info.path.replace("{id}", "7");
        let reply = send(router.clone(), info.method, &path, "");
        let expected = if info.policy.public_reason().is_some() {
            StatusCode::OK
        } else {
            StatusCode::UNAUTHORIZED
        };
        assert_eq!(reply.status, expected, "{} {}", info.method, info.path);
    }
    // Bodies identify which registered handler answered.
    assert_eq!(send(router.clone(), "GET", "/api/v1/ping", "").body, "ping");
    assert_eq!(send(router, "POST", "/api/v1/ping", "").body, "ping-post");
}

#[test]
fn unmarked_response_becomes_an_internal_error() {
    let raw = Router::new().route("/unmarked", get(|| async { "bypass" }));
    let reply = send(enforce(raw), "GET", "/unmarked", "");
    assert_eq!(reply.status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(reply.body, "{\"code\":\"internal_error\"}");
    assert!(!reply.body.contains("bypass"));
}

#[test]
fn build_installs_the_marker_check() {
    // A route whose endpoint bypasses the policy gate must be caught by the
    // router that build() assembles, not only by enforce() in isolation.
    let mut registry = Registry::new();
    registry
        .register(Route {
            method: Method::Get,
            path: PathTemplate::parse("/api/v1/bypass").unwrap(),
            policy: public(),
            contract: read_contract(),
            endpoint: get(|| async { "bypass" }),
        })
        .unwrap();
    let reply = send(app(registry), "GET", "/api/v1/bypass", "");
    assert_eq!(reply.status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(reply.body, "{\"code\":\"internal_error\"}");
}

#[test]
fn routes_sharing_a_parameter_position_under_different_names_are_rejected() {
    let param = |name| read_contract().path_param(name, ParamType::Integer);
    let mut registry = Registry::new();
    registry
        .register(route(Method::Get, "/a/{x}/b", public(), param("x")))
        .unwrap();
    assert_eq!(
        registry
            .register(route(Method::Get, "/a/{y}/c", public(), param("y")))
            .unwrap_err(),
        RegistryError::ConflictingPath {
            existing: "/a/{x}/b".into(),
            new: "/a/{y}/c".into()
        }
    );
    // The same names are fine, and the router builds without panicking.
    registry
        .register(route(Method::Get, "/a/{x}/c", public(), param("x")))
        .unwrap();
    let router = app(registry);
    assert_eq!(
        send(router.clone(), "GET", "/a/1/b", "").status,
        StatusCode::OK
    );
    assert_eq!(send(router, "GET", "/a/1/c", "").status, StatusCode::OK);
}

#[test]
fn head_and_options_do_not_bypass_the_policy() {
    let router = app(sample());
    let head_public = send(router.clone(), "HEAD", "/api/v1/ping", "");
    assert_eq!(head_public.status, StatusCode::OK);
    assert!(head_public.body.is_empty());
    let head_protected = send(router.clone(), "HEAD", "/api/v1/items/7", "");
    assert_eq!(head_protected.status, StatusCode::UNAUTHORIZED);
    let options = send(router.clone(), "OPTIONS", "/api/v1/ping", "");
    assert_eq!(options.status, StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(options.headers[header::ALLOW], "GET, HEAD, POST");
    assert!(
        options
            .headers
            .keys()
            .all(|name| !name.as_str().starts_with("access-control-"))
    );
    let options_protected = send(router, "OPTIONS", "/api/v1/items/7", "");
    assert_eq!(options_protected.status, StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(options_protected.body, "{\"code\":\"method_not_allowed\"}");
}

#[test]
fn unsupported_method_answers_405_and_never_the_handler() {
    let router = app(sample());
    let reply = send(router, "DELETE", "/api/v1/items/7", "");
    assert_eq!(reply.status, StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(reply.headers[header::ALLOW], "GET, HEAD");
    assert_eq!(reply.body, "{\"code\":\"method_not_allowed\"}");
}

#[test]
fn unknown_paths_get_the_api_error_not_a_page() {
    let router = app(sample());
    for path in [
        "/",
        "/api/v1/missing",
        "/index.html",
        "/api/v1/ping/extra",
        "/api/v1/ping/",
        "//api/v1/ping",
        "/API/v1/ping",
        "/api/v1/%70ing",
    ] {
        let reply = send(router.clone(), "GET", path, "");
        assert_eq!(reply.status, StatusCode::NOT_FOUND, "{path}");
        assert_eq!(reply.headers[header::CONTENT_TYPE], "application/json");
        assert_eq!(reply.body, "{\"code\":\"not_found\"}");
    }
    // An empty registry serves nothing but the built-in answers.
    assert_eq!(
        send(app(Registry::new()), "GET", "/api/v1/ping", "").status,
        StatusCode::NOT_FOUND
    );
}

#[test]
fn protected_routes_deny_anonymous_callers() {
    let reply = send(app(sample()), "GET", "/api/v1/items/7", "");
    assert_eq!(reply.status, StatusCode::UNAUTHORIZED);
    assert_eq!(reply.body, "{\"code\":\"unauthenticated\"}");
}

#[test]
fn body_limit_of_the_contract_applies() {
    let router = app(sample());
    let small = send(router.clone(), "POST", "/api/v1/upload", "1234");
    assert_eq!((small.status, small.body.as_str()), (StatusCode::OK, "4"));
    let large = send(router, "POST", "/api/v1/upload", "123456789");
    assert_eq!(large.status, StatusCode::PAYLOAD_TOO_LARGE);
}

#[test]
fn built_in_fallbacks_are_listed_with_a_public_reason() {
    let fallbacks = Registry::new().fallbacks();
    let kinds: Vec<(&str, u16)> = fallbacks
        .iter()
        .map(|info| (info.kind, info.status))
        .collect();
    assert_eq!(kinds, [("unknown_path", 404), ("method_not_allowed", 405)]);
    assert!(
        fallbacks
            .iter()
            .all(|info| info.policy.public_reason().is_some())
    );
}
