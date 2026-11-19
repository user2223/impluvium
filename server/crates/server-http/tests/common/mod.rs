// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Shared helpers of the integration tests: a registry with one route per
//! behaviour under test, and a way to send requests through the real router.

#![allow(dead_code)]

use std::convert::Infallible;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use axum::Router;
use axum::body::{Body, Bytes, to_bytes};
use axum::handler::Handler;
use axum::http::{HeaderMap, Request, Response, StatusCode, header};
use axum::response::{Html, IntoResponse, Redirect};
use http_body::Frame;
use serde::Deserialize;
use server_api_types::{ErrorCode, FieldError};
use server_domain::credential::NoCredentials;
use server_domain::policy::Policy;
use server_http::auth::{ExternalOrigin, PeerAddress, Scheme, SecurityContext};
use server_http::error::{HttpError, PreformattedError};
use server_http::extract::{ApiJson, ApiPath, ApiQuery};
use server_http::headers::SpaShell;
use server_http::proxy::TrustedProxies;
use server_http::registry::{
    BodyKind, Contract, Csrf, Limits, Method, ParamType, Registry, ResponseKind, Route,
};
use server_http::router::{AppOptions, build_with};
use tower::ServiceExt;

pub const ORIGIN: &str = "https://app.example.test";

pub fn context() -> SecurityContext {
    SecurityContext::new(
        ExternalOrigin::new(Scheme::Https, "app.example.test", None).unwrap(),
        TrustedProxies::default(),
        Arc::new(NoCredentials),
    )
}

pub struct Reply {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub body: Vec<u8>,
}

impl Reply {
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }

    pub fn json(&self) -> serde_json::Value {
        serde_json::from_slice(&self.body).unwrap()
    }
}

pub fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
}

pub fn request(method: &str, uri: &str, headers: &[(&str, &str)], body: &str) -> Request<Body> {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .extension(PeerAddress(SocketAddr::from(([192, 0, 2, 1], 4000))));
    for (name, value) in headers {
        builder = builder.header(*name, *value);
    }
    builder.body(Body::from(body.to_owned())).unwrap()
}

pub fn send(
    router: &Router,
    method: &str,
    uri: &str,
    headers: &[(&str, &str)],
    body: &str,
) -> Reply {
    let router = router.clone();
    let request = request(method, uri, headers, body);
    runtime().block_on(async move {
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

pub fn get(router: &Router, uri: &str) -> Reply {
    send(router, "GET", uri, &[], "")
}

pub fn post_json(router: &Router, uri: &str, body: &str) -> Reply {
    send(
        router,
        "POST",
        uri,
        &[("origin", ORIGIN), ("content-type", "application/json")],
        body,
    )
}

fn limits() -> Limits {
    Limits::new(16, Duration::from_secs(5))
}

fn public() -> Policy {
    Policy::public("test route").unwrap()
}

fn read() -> Contract {
    Contract::new(ResponseKind::Json, &[200], limits())
}

fn write() -> Contract {
    read().request_body(BodyKind::Json).csrf(Csrf::Required)
}

/// A body that yields one chunk and then never finishes.
pub struct Chunks {
    sent: bool,
}

impl http_body::Body for Chunks {
    type Data = Bytes;
    type Error = Infallible;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        _context: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, Infallible>>> {
        if self.sent {
            Poll::Pending
        } else {
            self.sent = true;
            Poll::Ready(Some(Ok(Frame::data(Bytes::from_static(b"first")))))
        }
    }
}

#[derive(Deserialize)]
pub struct Item {
    pub name: String,
}

#[derive(Deserialize)]
pub struct Paging {
    pub page: u32,
}

fn json_response(text: &'static str) -> Response<Body> {
    Response::builder()
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::CONTENT_LENGTH, text.len().to_string())
        .body(Body::from(text))
        .unwrap()
}

async fn ok() -> Response<Body> {
    json_response("{\"ok\":true}")
}

async fn hostile() -> Response<Body> {
    Response::builder()
        .header(header::CONTENT_TYPE, "application/json")
        .header("access-control-allow-origin", "*")
        .header("access-control-allow-credentials", "true")
        .header("strict-transport-security", "max-age=1")
        .header("cross-origin-embedder-policy", "require-corp")
        .header("cross-origin-resource-policy", "cross-origin")
        .header("x-xss-protection", "1")
        .header("x-content-type-options", "sniff")
        .header("x-content-type-options", "other")
        .header("referrer-policy", "unsafe-url")
        .header("content-security-policy", "default-src *")
        .body(Body::from("{}"))
        .unwrap()
}

async fn spa() -> impl IntoResponse {
    SpaShell(Html("<!doctype html><title>app</title>"))
}

async fn html_error_page() -> impl IntoResponse {
    PreformattedError(
        Response::builder()
            .status(StatusCode::NOT_FOUND)
            .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
            .body(Body::from("<p>gone</p>"))
            .unwrap(),
    )
}

async fn echo_json() -> (StatusCode, axum::Json<serde_json::Value>) {
    (
        StatusCode::BAD_REQUEST,
        axum::Json(serde_json::json!({ "echo": "<input>" })),
    )
}

async fn marked_json() -> impl IntoResponse {
    PreformattedError(echo_json().await)
}

async fn throttled() -> Response<Body> {
    Response::builder()
        .status(StatusCode::TOO_MANY_REQUESTS)
        .header(header::RETRY_AFTER, "7")
        .body(Body::empty())
        .unwrap()
}

async fn out_of_range() -> Response<Body> {
    Response::builder()
        .status(StatusCode::RANGE_NOT_SATISFIABLE)
        .header(header::CONTENT_RANGE, "bytes */10")
        .body(Body::empty())
        .unwrap()
}

async fn challenged() -> Response<Body> {
    Response::builder()
        .status(StatusCode::UNAUTHORIZED)
        .header(header::WWW_AUTHENTICATE, "Basic realm=\"a\"")
        .header(header::WWW_AUTHENTICATE, "Bearer")
        .header(header::SET_COOKIE, "a=1")
        .header(header::SET_COOKIE, "b=2")
        .body(Body::empty())
        .unwrap()
}

async fn file() -> Response<Body> {
    Response::builder()
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .header(header::CONTENT_LENGTH, "5")
        .body(Body::from("bytes"))
        .unwrap()
}

async fn redirect() -> Redirect {
    Redirect::to("/api/v1/ok")
}

async fn empty() -> StatusCode {
    StatusCode::NO_CONTENT
}

async fn not_modified() -> Response<Body> {
    Response::builder()
        .status(StatusCode::NOT_MODIFIED)
        .header(header::CONTENT_LENGTH, "42")
        .body(Body::empty())
        .unwrap()
}

async fn stream() -> Response<Body> {
    Response::new(Body::new(Chunks { sent: false }))
}

async fn failing() -> HttpError {
    HttpError::with_fields(
        ErrorCode::InvalidRequestBody,
        vec![FieldError::new("username", "too_short")],
    )
}

async fn json_name(ApiJson(item): ApiJson<Item>) -> String {
    item.name.len().to_string()
}

async fn paged(ApiQuery(paging): ApiQuery<Paging>) -> String {
    paging.page.to_string()
}

async fn by_id(ApiPath(id): ApiPath<u32>) -> String {
    id.to_string()
}

async fn boom() -> &'static str {
    panic!("SECRET-PANIC-PAYLOAD /home/someone/.secret")
}

fn route<H, T>(method: Method, path: &str, contract: Contract, handler: H) -> Route
where
    H: Handler<T, ()>,
    T: 'static,
{
    Route::new(method, path, public(), contract, handler).unwrap()
}

fn special(response: ResponseKind, status: u16) -> Contract {
    Contract::new(response, &[status], limits())
}

/// The registry of the integration tests.
pub fn registry() -> Registry {
    let routes = vec![
        route(Method::Get, "/api/v1/ok", read(), ok),
        route(Method::Get, "/api/v1/hostile", read(), hostile),
        route(Method::Get, "/app", read(), spa),
        route(Method::Get, "/gone", read(), html_error_page),
        route(Method::Get, "/api/v1/file", read(), file),
        route(
            Method::Get,
            "/api/v1/redirect",
            special(ResponseKind::Redirect, 303),
            redirect,
        ),
        route(
            Method::Get,
            "/api/v1/empty",
            special(ResponseKind::Empty, 204),
            empty,
        ),
        route(
            Method::Get,
            "/api/v1/not-modified",
            special(ResponseKind::Empty, 304),
            not_modified,
        ),
        route(
            Method::Get,
            "/api/v1/stream",
            special(ResponseKind::Stream, 200),
            stream,
        ),
        route(Method::Get, "/api/v1/failing", read(), failing),
        route(Method::Post, "/api/v1/json", write(), json_name),
        route(
            Method::Get,
            "/api/v1/paged",
            read().query_param("page", ParamType::Integer),
            paged,
        ),
        route(
            Method::Get,
            "/api/v1/items/{id}",
            read().path_param("id", ParamType::Integer),
            by_id,
        ),
        route(Method::Get, "/api/v1/boom", read(), boom),
        route(Method::Get, "/api/v1/echo-json", read(), echo_json),
        route(Method::Get, "/api/v1/marked-json", read(), marked_json),
        route(Method::Get, "/api/v1/throttled", read(), throttled),
        route(Method::Get, "/api/v1/out-of-range", read(), out_of_range),
        route(Method::Get, "/api/v1/challenged", read(), challenged),
    ];
    let mut registry = Registry::new();
    for route in routes {
        registry.register(route).unwrap();
    }
    registry
}

pub fn router_with(registry: Registry, options: &AppOptions) -> Router {
    build_with(registry, context(), options).into_router()
}

pub fn router() -> Router {
    router_with(registry(), &AppOptions::default())
}
