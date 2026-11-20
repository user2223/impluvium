// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::body::{Body, Bytes, to_bytes};
use axum::http::{Request, StatusCode, header};
use axum::routing::get;
use server_domain::credential::NoCredentials;
use server_domain::policy::{Policy, Requirement};
use tower::ServiceExt;

use super::{AppOptions, ApplicationRouter, build, enforce, finish};
use crate::auth::{ExternalOrigin, PeerAddress, Scheme, SecurityContext};
use crate::proxy::TrustedProxies;
use crate::registry::{
    BodyKind, Contract, Csrf, Limits, Method, ParamType, PathError, PathTemplate, Registry,
    RegistryError, ResponseKind, Route,
};

/// The external origin of the test router.
pub(super) const TEST_ORIGIN: &str = "https://app.example.test";

/// The peer of requests that do not name one.
pub(super) fn test_peer() -> SocketAddr {
    SocketAddr::from(([192, 0, 2, 1], 4000))
}

/// A context with the test origin, no trusted proxy and no credential.
pub(super) fn test_context() -> SecurityContext {
    SecurityContext::new(
        ExternalOrigin::new(Scheme::Https, "app.example.test", None).unwrap(),
        TrustedProxies::default(),
        Arc::new(NoCredentials),
    )
}

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

async fn answers_without_cache_control() -> &'static str {
    "ok"
}

type WithHeader = ([(header::HeaderName, &'static str); 1], &'static str);

async fn answers_with_its_own_cache_control() -> WithHeader {
    ([(header::CACHE_CONTROL, "max-age=60")], "ok")
}

fn cache_values(reply: &Reply) -> Vec<String> {
    reply
        .headers
        .get_all(header::CACHE_CONTROL)
        .iter()
        .map(|value| value.to_str().unwrap().to_owned())
        .collect()
}

fn cache_app(routes: Vec<Route>) -> Router {
    let mut registry = Registry::new();
    for route in routes {
        registry.register(route).unwrap();
    }
    app(registry)
}

fn get_route<H, T>(path: &str, contract: Contract, handler: H) -> Route
where
    H: axum::handler::Handler<T, ()>,
    T: 'static,
{
    Route::new(Method::Get, path, public(), contract, handler).unwrap()
}

#[test]
fn successful_api_answers_are_not_cached_unless_the_handler_says_otherwise() {
    let router = cache_app(vec![
        get_route(
            "/api/v1/plain",
            read_contract(),
            answers_without_cache_control,
        ),
        get_route(
            "/other/plain",
            read_contract(),
            answers_without_cache_control,
        ),
        get_route(
            "/api/v1/own",
            read_contract(),
            answers_with_its_own_cache_control,
        ),
        get_route(
            "/other/own",
            read_contract(),
            answers_with_its_own_cache_control,
        ),
        get_route(
            "/api/v1/forced",
            read_contract().no_store(),
            answers_with_its_own_cache_control,
        ),
        get_route(
            "/other/forced",
            read_contract().no_store(),
            answers_without_cache_control,
        ),
    ]);
    let cases = [
        ("/api/v1/plain", vec!["no-store"]),
        ("/other/plain", vec![]),
        // The handler's own header is kept without the flag.
        ("/api/v1/own", vec!["max-age=60"]),
        ("/other/own", vec!["max-age=60"]),
        // With the flag exactly one `no-store` remains.
        ("/api/v1/forced", vec!["no-store"]),
        ("/other/forced", vec!["no-store"]),
    ];
    for (path, expected) in cases {
        let reply = send(router.clone(), "GET", path, "");
        assert_eq!(reply.status, StatusCode::OK, "{path}");
        assert_eq!(cache_values(&reply), expected, "{path}");
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
            .header(header::ORIGIN, TEST_ORIGIN)
            .extension(PeerAddress(test_peer()))
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

/// The error code of a body that follows the web error shape.
fn code_of(body: &str) -> String {
    let value: serde_json::Value = serde_json::from_str(body).unwrap();
    assert!(value["message"].is_string() && value["request_id"].is_string());
    value["code"].as_str().unwrap().to_owned()
}

fn app(registry: Registry) -> Router {
    let ApplicationRouter { router } = build(registry, test_context());
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
    let router = finish(enforce(raw), &AppOptions::default());
    let reply = send(router, "GET", "/unmarked", "");
    assert_eq!(reply.status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(code_of(&reply.body), "internal_error");
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
    assert_eq!(code_of(&reply.body), "internal_error");
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
    assert_eq!(code_of(&options_protected.body), "method_not_allowed");
}

#[test]
fn unsupported_method_answers_405_and_never_the_handler() {
    let router = app(sample());
    let reply = send(router, "DELETE", "/api/v1/items/7", "");
    assert_eq!(reply.status, StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(reply.headers[header::ALLOW], "GET, HEAD");
    assert_eq!(code_of(&reply.body), "method_not_allowed");
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
        assert_eq!(code_of(&reply.body), "not_found");
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
    assert_eq!(code_of(&reply.body), "unauthenticated");
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

async fn direct_body_length(request: axum::extract::Request) -> String {
    // Intentionally bypasses Bytes and DefaultBodyLimit to exercise the raw-body guard.
    to_bytes(request.into_body(), usize::MAX)
        .await
        .unwrap()
        .len()
        .to_string()
}

fn binary_contract() -> Contract {
    Contract::new(ResponseKind::Json, &[200], limits())
        .request_body(BodyKind::Binary)
        .csrf(Csrf::Required)
}

struct UploadFrames(std::vec::IntoIter<Result<Bytes, std::io::Error>>);

impl http_body::Body for UploadFrames {
    type Data = Bytes;
    type Error = std::io::Error;

    fn poll_frame(
        mut self: std::pin::Pin<&mut Self>,
        _context: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Result<http_body::Frame<Self::Data>, Self::Error>>> {
        std::task::Poll::Ready(self.0.next().map(|chunk| chunk.map(http_body::Frame::data)))
    }
}

#[test]
fn binary_contracts_require_a_limit_and_csrf_and_have_stable_inventory_names() {
    assert_eq!(BodyKind::Binary.as_str(), "binary");
    assert_eq!(ResponseKind::BinaryOrEmpty.as_str(), "binary-or-empty");
    for contract in [
        Contract::new(
            ResponseKind::Json,
            &[200],
            Limits::new(0, Duration::from_secs(1)),
        )
        .request_body(BodyKind::Binary)
        .csrf(Csrf::Required),
        Contract::new(ResponseKind::Json, &[200], limits()).request_body(BodyKind::Binary),
    ] {
        assert!(
            Route::new(
                Method::Post,
                "/api/v1/raw",
                public(),
                contract,
                direct_body_length
            )
            .is_err()
        );
    }
    assert!(
        Route::new(
            Method::Get,
            "/api/v1/raw",
            public(),
            binary_contract(),
            ping
        )
        .is_err()
    );
}

#[test]
fn binary_uploads_are_bounded_before_direct_request_consumers() {
    let router = cache_app(vec![
        Route::new(
            Method::Post,
            "/api/v1/raw",
            public(),
            binary_contract(),
            direct_body_length,
        )
        .unwrap(),
    ]);
    for body in ["", "1234", "12345678"] {
        let reply = send(router.clone(), "POST", "/api/v1/raw", body);
        assert_eq!(reply.status, StatusCode::OK);
        assert_eq!(reply.body, body.len().to_string());
    }
    // No Content-Length is supplied: the bound also covers streamed/chunked bodies.
    let large = send(router.clone(), "POST", "/api/v1/raw", "123456789");
    assert_eq!(large.status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(code_of(&large.body), "payload_too_large");
    assert_eq!(cache_values(&large), ["no-store"]);
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(async move {
            for (chunks, status, code) in [
                (
                    vec![
                        Ok(Bytes::from_static(b"1234")),
                        Ok(Bytes::from_static(b"5678")),
                    ],
                    StatusCode::OK,
                    None,
                ),
                (
                    vec![
                        Ok(Bytes::from_static(b"1234")),
                        Ok(Bytes::from_static(b"56789")),
                    ],
                    StatusCode::PAYLOAD_TOO_LARGE,
                    Some("payload_too_large"),
                ),
                (
                    vec![Err(std::io::Error::other("broken upload"))],
                    StatusCode::BAD_REQUEST,
                    Some("bad_request"),
                ),
            ] {
                let body = Body::new(UploadFrames(chunks.into_iter()));
                let request = Request::builder()
                    .method("POST")
                    .uri("/api/v1/raw")
                    .header(header::ORIGIN, TEST_ORIGIN)
                    .extension(PeerAddress(test_peer()))
                    .body(body)
                    .unwrap();
                let reply = router.clone().oneshot(request).await.unwrap();
                assert_eq!(reply.status(), status);
                let bytes = to_bytes(reply.into_body(), usize::MAX).await.unwrap();
                let body = std::str::from_utf8(&bytes).unwrap();
                if let Some(code) = code {
                    assert_eq!(code_of(body), code);
                } else {
                    assert_eq!(body, "8");
                }
            }
        });
}

#[test]
fn binary_upload_csrf_runs_before_body_consumption() {
    let router = cache_app(vec![
        Route::new(
            Method::Post,
            "/api/v1/raw",
            public(),
            binary_contract(),
            direct_body_length,
        )
        .unwrap(),
    ]);
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(async move {
            let request = Request::builder()
                .method("POST")
                .uri("/api/v1/raw")
                .extension(PeerAddress(test_peer()))
                .body(Body::from("123456789"))
                .unwrap();
            let reply = router.oneshot(request).await.unwrap();
            // The body exceeds the bound, but CSRF must reject first.
            assert_eq!(reply.status(), StatusCode::FORBIDDEN);
            let bytes = to_bytes(reply.into_body(), usize::MAX).await.unwrap();
            assert_eq!(
                code_of(std::str::from_utf8(&bytes).unwrap()),
                "csrf_rejected"
            );
        });
}

async fn binary_download_fixture(request: axum::extract::Request) -> axum::response::Response {
    let head = request.method() == axum::http::Method::HEAD;
    let unchanged = request
        .headers()
        .get(header::IF_NONE_MATCH)
        .is_some_and(|value| value == "hash");
    let mut response = axum::response::Response::new(if head || unchanged {
        Body::empty()
    } else {
        Body::from("image")
    });
    if unchanged {
        *response.status_mut() = StatusCode::NOT_MODIFIED;
    }
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, "image/png".parse().unwrap());
    response
        .headers_mut()
        .insert(header::ETAG, "hash".parse().unwrap());
    if !head {
        response.headers_mut().insert(
            header::CACHE_CONTROL,
            "private, max-age=31536000, s-maxage=31536000, immutable"
                .parse()
                .unwrap(),
        );
    }
    response
}

#[test]
fn binary_download_headers_survive_on_get_head_and_not_modified() {
    let router = cache_app(vec![get_route(
        "/api/v1/binary",
        Contract::new(ResponseKind::BinaryOrEmpty, &[200, 304], limits()),
        binary_download_fixture,
    )]);
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(async move {
            for (method, conditional, status, body) in [
                ("GET", false, StatusCode::OK, "image"),
                ("GET", true, StatusCode::NOT_MODIFIED, ""),
                ("HEAD", false, StatusCode::OK, ""),
                ("HEAD", true, StatusCode::NOT_MODIFIED, ""),
            ] {
                let mut request = Request::builder()
                    .method(method)
                    .uri("/api/v1/binary")
                    .extension(PeerAddress(test_peer()));
                if conditional {
                    request = request.header(header::IF_NONE_MATCH, "hash");
                }
                let reply = router
                    .clone()
                    .oneshot(request.body(Body::empty()).unwrap())
                    .await
                    .unwrap();
                assert_eq!(reply.status(), status);
                assert_eq!(reply.headers()[header::CONTENT_TYPE], "image/png");
                assert_eq!(reply.headers()[header::ETAG], "hash");
                assert_eq!(
                    reply.headers()[header::CACHE_CONTROL],
                    if method == "HEAD" {
                        "no-store"
                    } else {
                        "private, max-age=31536000, s-maxage=31536000, immutable"
                    }
                );
                let bytes = to_bytes(reply.into_body(), usize::MAX).await.unwrap();
                assert_eq!(bytes.as_ref(), body.as_bytes());
            }
        });
}

struct UnreadUpload;

impl http_body::Body for UnreadUpload {
    type Data = Bytes;
    type Error = std::io::Error;

    fn poll_frame(
        self: std::pin::Pin<&mut Self>,
        _context: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Result<http_body::Frame<Self::Data>, Self::Error>>> {
        panic!("the rejected request body must not be read")
    }
}

fn upload_route(kind: BodyKind, policy: Policy) -> Route {
    Route::new(
        Method::Post,
        "/api/v1/form",
        policy,
        Contract::new(ResponseKind::Json, &[200], limits())
            .request_body(kind)
            .csrf(Csrf::Required),
        direct_body_length,
    )
    .unwrap()
}

#[test]
fn declared_oversized_binary_and_multipart_bodies_are_refused_without_reading() {
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(async {
            for (kind, length) in [
                (BodyKind::Binary, "9"),
                (BodyKind::Multipart, "9"),
                (BodyKind::Binary, "99999999999999999999999999999999"),
                (BodyKind::Multipart, "99999999999999999999999999999999"),
            ] {
                let router = cache_app(vec![upload_route(kind, public())]);
                let request = Request::builder()
                    .method("POST")
                    .uri("/api/v1/form")
                    .header(header::ORIGIN, TEST_ORIGIN)
                    .header(header::CONTENT_LENGTH, length)
                    .header(
                        header::CONTENT_TYPE,
                        "multipart/form-data; boundary=fixture",
                    )
                    .extension(PeerAddress(test_peer()))
                    .body(Body::new(UnreadUpload))
                    .unwrap();
                let response = router.oneshot(request).await.unwrap();
                assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
            }
        });
}

#[test]
fn multipart_media_type_boundary_and_streamed_bounds_are_checked() {
    let router = cache_app(vec![upload_route(BodyKind::Multipart, public())]);
    tokio::runtime::Builder::new_current_thread().build().unwrap().block_on(async {
        for (content_type, chunks, status) in [
            ("multipart/form-data; boundary=fixture", vec!["1234", "5678"], StatusCode::OK),
            ("MULTIPART/FORM-DATA; charset=utf-8; boundary=\"fixture\"", vec!["1234"], StatusCode::OK),
            ("multipart/form-data; boundary=fixture", vec!["1234", "56789"], StatusCode::PAYLOAD_TOO_LARGE),
            ("application/json", vec![], StatusCode::BAD_REQUEST),
            ("multipart/form-data", vec![], StatusCode::BAD_REQUEST),
            ("multipart/form-data; boundary=", vec![], StatusCode::BAD_REQUEST),
            ("multipart/form-data; boundary=\"\"", vec![], StatusCode::BAD_REQUEST),
            ("multipart/form-data; boundary=one; boundary=two", vec![], StatusCode::BAD_REQUEST),
            ("multipart/form-data; boundary=\"unterminated", vec![], StatusCode::BAD_REQUEST),
            ("multipart/form-data; boundary=bad@value", vec![], StatusCode::BAD_REQUEST),
            ("multipart/form-data; boundary=bad:value", vec![], StatusCode::BAD_REQUEST),
            ("multipart/form-data; boundary=\"trailing \"", vec![], StatusCode::BAD_REQUEST),
            ("multipart/form-data; boundary=abcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrs", vec![], StatusCode::BAD_REQUEST),
            ("multipart/form-data; boundary=\"a b\"", vec!["1234"], StatusCode::OK),
        ] {
            let chunks = chunks.into_iter().map(|chunk| Ok(Bytes::from_static(chunk.as_bytes()))).collect::<Vec<_>>();
            let request = Request::builder().method("POST").uri("/api/v1/form")
                .header(header::ORIGIN, TEST_ORIGIN).header(header::CONTENT_TYPE, content_type)
                // A lying length must not disable frame counting.
                .header(header::CONTENT_LENGTH, "1")
                .extension(PeerAddress(test_peer()))
                .body(Body::new(UploadFrames(chunks.into_iter()))).unwrap();
            let response = router.clone().oneshot(request).await.unwrap();
            assert_eq!(response.status(), status, "{content_type}");
        }
        let chunks = vec![Ok(Bytes::from_static(b"1234")), Ok(Bytes::from_static(b"56789"))];
        let request = Request::builder().method("POST").uri("/api/v1/form")
            .header(header::ORIGIN, TEST_ORIGIN)
            .header(header::CONTENT_TYPE, "multipart/form-data; boundary=fixture")
            .extension(PeerAddress(test_peer()))
            .body(Body::new(UploadFrames(chunks.into_iter()))).unwrap();
        assert_eq!(router.clone().oneshot(request).await.unwrap().status(), StatusCode::PAYLOAD_TOO_LARGE);
        let request = Request::builder().method("POST").uri("/api/v1/form")
            .header(header::ORIGIN, TEST_ORIGIN).extension(PeerAddress(test_peer()))
            .body(Body::new(UnreadUpload)).unwrap();
        assert_eq!(router.oneshot(request).await.unwrap().status(), StatusCode::BAD_REQUEST);
    });
}

#[test]
fn multipart_csrf_and_policy_gates_run_before_length_type_or_frames() {
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(async {
            for (policy, with_origin, expected) in [
                (public(), false, StatusCode::FORBIDDEN),
                (protected(), true, StatusCode::UNAUTHORIZED),
            ] {
                let router = cache_app(vec![upload_route(BodyKind::Multipart, policy)]);
                let mut builder = Request::builder()
                    .method("POST")
                    .uri("/api/v1/form")
                    .header(header::CONTENT_LENGTH, "999")
                    .header(header::CONTENT_TYPE, "application/json")
                    .extension(PeerAddress(test_peer()));
                if with_origin {
                    builder = builder.header(header::ORIGIN, TEST_ORIGIN);
                }
                let response = router
                    .oneshot(builder.body(Body::new(UnreadUpload)).unwrap())
                    .await
                    .unwrap();
                assert_eq!(response.status(), expected);
            }
        });
}

#[test]
fn multipart_contracts_require_limits_csrf_and_use_stable_inventory_names() {
    assert_eq!(BodyKind::Multipart.as_str(), "multipart");
    assert_eq!(ResponseKind::JsonOrEmpty.as_str(), "json-or-empty");
    for contract in [
        Contract::new(
            ResponseKind::Json,
            &[200],
            Limits::new(0, Duration::from_secs(1)),
        )
        .request_body(BodyKind::Multipart)
        .csrf(Csrf::Required),
        Contract::new(ResponseKind::Json, &[200], limits()).request_body(BodyKind::Multipart),
    ] {
        assert!(Route::new(Method::Post, "/api/v1/form", public(), contract, ping).is_err());
    }
}
