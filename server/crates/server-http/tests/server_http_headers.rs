// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! The protective header contract, checked against a committed fixture.

mod common;

use std::future::poll_fn;
use std::task::Poll;

use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use common::{get, request, router, router_with, runtime, send};
use http_body::Body as _;
use serde_json::{Map, Value};
use server_http::headers::ScriptHash;
use server_http::router::AppOptions;
use tower::ServiceExt;

const FIXTURE: &str = include_str!("fixtures/security-headers.json");

fn fixture() -> Value {
    serde_json::from_str(FIXTURE).unwrap()
}

/// Compares a header map with the expectation of one class. A string value
/// demands exactly one header value that equals it; `null` demands that the
/// header is absent. Headers whose name starts with a listed prefix must be
/// absent as well. Returns one line per mismatch.
fn compare(headers: &HeaderMap, expected: &Map<String, Value>, prefixes: &[String]) -> Vec<String> {
    let mut problems = Vec::new();
    for (name, want) in expected {
        let values: Vec<&HeaderValue> = headers.get_all(name.as_str()).iter().collect();
        match want {
            Value::Null => {
                if !values.is_empty() {
                    problems.push(format!("{name}: expected absent"));
                }
            }
            Value::String(text) => {
                if values.len() != 1 {
                    problems.push(format!(
                        "{name}: expected one value, found {}",
                        values.len()
                    ));
                } else if values[0].to_str().ok() != Some(text.as_str()) {
                    problems.push(format!("{name}: wrong value"));
                }
            }
            other => problems.push(format!("{name}: unsupported expectation {other}")),
        }
    }
    for name in headers.keys() {
        if prefixes
            .iter()
            .any(|prefix| name.as_str().starts_with(prefix))
        {
            problems.push(format!("{name}: forbidden prefix"));
        }
    }
    problems
}

fn prefixes() -> Vec<String> {
    fixture()["absent_prefixes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_str().unwrap().to_owned())
        .collect()
}

fn expectation(class: &str) -> Map<String, Value> {
    fixture()["classes"][class].as_object().unwrap().clone()
}

fn class_request(class: &str) -> (&'static str, StatusCode) {
    match class {
        "spa_html" => ("/app", StatusCode::OK),
        "api_json" => ("/api/v1/ok", StatusCode::OK),
        "error_json" => ("/api/v1/missing", StatusCode::NOT_FOUND),
        "file" => ("/api/v1/file", StatusCode::OK),
        "redirect" => ("/api/v1/redirect", StatusCode::SEE_OTHER),
        other => panic!("unknown class {other}"),
    }
}

#[test]
fn every_class_matches_the_fixture() {
    let router = router();
    let classes = fixture()["classes"].as_object().unwrap().clone();
    assert_eq!(classes.len(), 5);
    for class in classes.keys() {
        let (path, status) = class_request(class);
        let reply = get(&router, path);
        assert_eq!(reply.status, status, "{class}");
        let problems = compare(&reply.headers, &expectation(class), &prefixes());
        assert!(problems.is_empty(), "{class}: {problems:?}");
    }
}

#[test]
fn comparator_detects_missing_wrong_duplicate_and_forbidden_headers() {
    let reply = get(&router(), "/api/v1/ok");
    let want = expectation("api_json");
    assert!(compare(&reply.headers, &want, &prefixes()).is_empty());

    let mut missing = reply.headers.clone();
    missing.remove("x-content-type-options");
    assert!(!compare(&missing, &want, &prefixes()).is_empty());

    let mut wrong = reply.headers.clone();
    wrong.insert("referrer-policy", HeaderValue::from_static("origin"));
    assert!(!compare(&wrong, &want, &prefixes()).is_empty());

    let mut duplicate = reply.headers.clone();
    duplicate.append(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    assert!(!compare(&duplicate, &want, &prefixes()).is_empty());

    let mut extra = reply.headers.clone();
    extra.insert("x-frame-options", HeaderValue::from_static("DENY"));
    assert!(!compare(&extra, &want, &prefixes()).is_empty());

    let mut cors = reply.headers.clone();
    cors.insert("access-control-allow-origin", HeaderValue::from_static("*"));
    assert!(!compare(&cors, &want, &prefixes()).is_empty());
}

#[test]
fn headers_set_by_a_handler_are_replaced_or_removed() {
    let reply = get(&router(), "/api/v1/hostile");
    assert_eq!(reply.status, StatusCode::OK);
    let problems = compare(&reply.headers, &expectation("api_json"), &prefixes());
    assert!(problems.is_empty(), "{problems:?}");
}

#[test]
fn error_pages_rendered_as_html_get_the_html_headers_without_the_shell_policy() {
    let reply = get(&router(), "/gone");
    assert_eq!(reply.status, StatusCode::NOT_FOUND);
    let mut want = expectation("spa_html");
    want.insert(
        "content-security-policy".into(),
        expectation("api_json")["content-security-policy"].clone(),
    );
    // A handler-made HTML error page keeps its body; only headers are checked.
    let problems = compare(&reply.headers, &want, &prefixes());
    assert!(problems.is_empty(), "{problems:?}");
}

#[test]
fn script_hashes_extend_only_the_shell_policy() {
    let hash = "sha256-47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU=";
    let options = AppOptions {
        script_hashes: vec![ScriptHash::parse(hash).unwrap()],
        ..AppOptions::default()
    };
    let router = router_with(common::registry(), &options);
    let shell = get(&router, "/app");
    let csp = shell.headers[header::CONTENT_SECURITY_POLICY]
        .to_str()
        .unwrap();
    assert!(csp.starts_with(&format!(
        "default-src 'none'; script-src 'self' '{hash}'; style-src"
    )));
    let api = get(&router, "/api/v1/ok");
    assert!(
        !api.headers[header::CONTENT_SECURITY_POLICY]
            .to_str()
            .unwrap()
            .contains("sha256")
    );
}

#[test]
fn body_and_content_length_are_preserved() {
    let router = router();
    let reply = get(&router, "/api/v1/ok");
    assert_eq!(reply.body, b"{\"ok\":true}");
    assert_eq!(reply.headers[header::CONTENT_LENGTH], "11");
    assert_eq!(reply.headers[header::CONTENT_TYPE], "application/json");
    let file = get(&router, "/api/v1/file");
    assert_eq!(file.body, b"bytes");
    assert_eq!(file.headers[header::CONTENT_LENGTH], "5");
}

#[test]
fn empty_answers_stay_empty() {
    let router = router();
    let no_content = get(&router, "/api/v1/empty");
    assert_eq!(no_content.status, StatusCode::NO_CONTENT);
    assert!(no_content.body.is_empty());
    // The router may report `content-length: 0` for an empty body; the HTTP
    // server drops it on the wire for 204, which the end-to-end harness checks.
    assert!(
        no_content
            .headers
            .get(header::CONTENT_LENGTH)
            .is_none_or(|value| value == "0")
    );
    let head = send(&router, "HEAD", "/api/v1/ok", &[], "");
    assert_eq!(head.status, StatusCode::OK);
    assert!(head.body.is_empty());
    assert_eq!(head.headers["x-content-type-options"], "nosniff");
    let redirect = get(&router, "/api/v1/redirect");
    assert_eq!(redirect.status, StatusCode::SEE_OTHER);
    assert_eq!(redirect.headers[header::LOCATION], "/api/v1/ok");
}

#[test]
fn a_304_keeps_the_content_length_of_the_handler() {
    let reply = get(&router(), "/api/v1/not-modified");
    assert_eq!(reply.status, StatusCode::NOT_MODIFIED);
    assert_eq!(reply.headers[header::CONTENT_LENGTH], "42");
    assert_eq!(reply.headers["x-content-type-options"], "nosniff");
}

#[test]
fn a_streaming_body_is_not_buffered() {
    let router = router();
    runtime().block_on(async move {
        let response = router
            .oneshot(request("GET", "/api/v1/stream", &[], ""))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["x-content-type-options"], "nosniff");
        let mut body = Box::pin(response.into_body());
        let first = poll_fn(|context| body.as_mut().poll_frame(context)).await;
        let frame = first.unwrap().unwrap();
        assert_eq!(frame.into_data().unwrap().as_ref(), b"first");
        // The source never finishes; the next poll must stay pending.
        let pending =
            poll_fn(|context| Poll::Ready(body.as_mut().poll_frame(context).is_pending())).await;
        assert!(pending);
    });
}
