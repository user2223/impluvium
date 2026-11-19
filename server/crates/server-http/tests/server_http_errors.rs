// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! The error shape, request identifiers and panic handling through the real router.

mod common;

use std::collections::HashSet;

use axum::http::{StatusCode, header};
use common::{Reply, get, post_json, router, send};
use server_api_types::ErrorCode;

/// Asserts that a reply is an error with the given status and code, in the full shape.
fn error_of(reply: &Reply, status: StatusCode, code: ErrorCode) {
    assert_eq!(reply.status, status, "{}", reply.text());
    assert_eq!(reply.headers[header::CONTENT_TYPE], "application/json");
    assert_eq!(reply.headers[header::CACHE_CONTROL], "no-store");
    let body = reply.json();
    assert_eq!(body["code"], code.as_str());
    assert_eq!(body["message"], code.message());
    let id = body["request_id"].as_str().unwrap();
    assert!(!id.is_empty());
    assert_eq!(reply.headers["x-request-id"], id);
    let keys: HashSet<&str> = body
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert!(keys.is_subset(&HashSet::from(["code", "message", "request_id", "fields"])));
}

#[test]
fn handler_errors_carry_code_and_field_details() {
    let reply = get(&router(), "/api/v1/failing");
    error_of(
        &reply,
        StatusCode::BAD_REQUEST,
        ErrorCode::InvalidRequestBody,
    );
    assert_eq!(
        reply.json()["fields"],
        serde_json::json!([{ "field": "username", "code": "too_short" }])
    );
}

#[test]
fn json_errors_are_mapped() {
    let router = router();
    let ok = post_json(&router, "/api/v1/json", "{\"name\":\"abc\"}");
    assert_eq!((ok.status, ok.text()), (StatusCode::OK, "3".to_owned()));
    for body in ["{not json", "{\"name\":5}", "{}", ""] {
        let reply = post_json(&router, "/api/v1/json", body);
        error_of(&reply, StatusCode::BAD_REQUEST, ErrorCode::InvalidJsonBody);
    }
    let wrong_type = send(
        &router,
        "POST",
        "/api/v1/json",
        &[("origin", common::ORIGIN), ("content-type", "text/plain")],
        "{\"name\":\"abc\"}",
    );
    error_of(
        &wrong_type,
        StatusCode::UNSUPPORTED_MEDIA_TYPE,
        ErrorCode::UnsupportedMediaType,
    );
    let missing_type = send(
        &router,
        "POST",
        "/api/v1/json",
        &[("origin", common::ORIGIN)],
        "{\"name\":\"abc\"}",
    );
    error_of(
        &missing_type,
        StatusCode::UNSUPPORTED_MEDIA_TYPE,
        ErrorCode::UnsupportedMediaType,
    );
}

#[test]
fn oversized_bodies_are_rejected_with_the_shape() {
    let big = format!("{{\"name\":\"{}\"}}", "a".repeat(64));
    let reply = post_json(&router(), "/api/v1/json", &big);
    error_of(
        &reply,
        StatusCode::PAYLOAD_TOO_LARGE,
        ErrorCode::PayloadTooLarge,
    );
}

#[test]
fn query_and_path_errors_are_mapped() {
    let router = router();
    assert_eq!(get(&router, "/api/v1/paged?page=3").text(), "3");
    for uri in [
        "/api/v1/paged",
        "/api/v1/paged?page=x",
        "/api/v1/paged?page=-1",
    ] {
        error_of(
            &get(&router, uri),
            StatusCode::BAD_REQUEST,
            ErrorCode::InvalidQuery,
        );
    }
    assert_eq!(get(&router, "/api/v1/items/42").text(), "42");
    error_of(
        &get(&router, "/api/v1/items/abc"),
        StatusCode::BAD_REQUEST,
        ErrorCode::InvalidPathParam,
    );
}

#[test]
fn unknown_paths_and_methods_use_the_shape() {
    let router = router();
    error_of(
        &get(&router, "/api/v1/nope"),
        StatusCode::NOT_FOUND,
        ErrorCode::NotFound,
    );
    let reply = send(&router, "DELETE", "/api/v1/ok", &[], "");
    error_of(
        &reply,
        StatusCode::METHOD_NOT_ALLOWED,
        ErrorCode::MethodNotAllowed,
    );
    assert_eq!(reply.headers[header::ALLOW], "GET, HEAD");
}

#[test]
fn messages_never_echo_input() {
    let router = router();
    let hostile = [
        "http://evil.example/steal",
        "/etc/passwd",
        "SELECT%20*%20FROM%20users",
        "C:%5CUsers%5Csecret",
    ];
    let mut replies = Vec::new();
    for input in hostile {
        replies.push(get(&router, &format!("/api/v1/items/{input}")));
        replies.push(get(&router, &format!("/api/v1/paged?page={input}")));
        replies.push(get(&router, &format!("/api/v1/{input}")));
        replies.push(post_json(
            &router,
            "/api/v1/json",
            &format!("{{\"name\": {input}"),
        ));
    }
    replies.push(send(
        &router,
        "GET",
        "/api/v1/nope",
        &[("host", "evil.example")],
        "",
    ));
    for reply in replies {
        let text = reply.text();
        for needle in [
            "evil.example",
            "passwd",
            "SELECT",
            "Users",
            "secret",
            "http://",
            "://",
        ] {
            assert!(!text.contains(needle), "{needle} in {text}");
        }
        assert!(reply.json()["message"].as_str().unwrap().len() <= 200);
    }
}

#[test]
fn every_response_has_a_fresh_server_assigned_request_id() {
    let router = router();
    let mut seen = HashSet::new();
    for _ in 0..20 {
        let reply = send(
            &router,
            "GET",
            "/api/v1/ok",
            &[("x-request-id", "attacker-chosen")],
            "",
        );
        let id = reply.headers["x-request-id"].to_str().unwrap().to_owned();
        assert!(!id.contains("attacker"));
        assert!(seen.insert(id));
    }
    let error = send(
        &router,
        "GET",
        "/api/v1/nope",
        &[("x-request-id", "attacker-chosen")],
        "",
    );
    assert!(!error.text().contains("attacker"));
}

#[test]
fn a_panic_becomes_an_internal_error_without_the_payload() {
    let router = router();
    let reply = get(&router, "/api/v1/boom");
    error_of(
        &reply,
        StatusCode::INTERNAL_SERVER_ERROR,
        ErrorCode::InternalError,
    );
    let text = reply.text();
    assert!(!text.contains("SECRET-PANIC-PAYLOAD"));
    assert!(!text.contains("/home/someone"));
    // The router keeps serving after a panic.
    assert_eq!(get(&router, "/api/v1/ok").status, StatusCode::OK);
}

#[test]
fn an_unmarked_json_error_cannot_echo_input() {
    let reply = get(&router(), "/api/v1/echo-json");
    error_of(&reply, StatusCode::BAD_REQUEST, ErrorCode::BadRequest);
    let text = reply.text();
    assert!(
        !text.contains("<input>") && !text.contains("echo"),
        "{text}"
    );
}

#[test]
fn a_marked_error_passes_through_unchanged() {
    let reply = get(&router(), "/api/v1/marked-json");
    assert_eq!(reply.status, StatusCode::BAD_REQUEST);
    assert_eq!(reply.text(), "{\"echo\":\"<input>\"}");
}

#[test]
fn uncoded_errors_keep_their_status_and_status_headers() {
    let router = router();
    let throttled = get(&router, "/api/v1/throttled");
    error_of(
        &throttled,
        StatusCode::TOO_MANY_REQUESTS,
        ErrorCode::BadRequest,
    );
    assert_eq!(throttled.headers[header::RETRY_AFTER], "7");
    let range = get(&router, "/api/v1/out-of-range");
    assert_eq!(range.status, StatusCode::RANGE_NOT_SATISFIABLE);
    assert_eq!(range.json()["code"], "bad_request");
    assert_eq!(range.headers[header::CONTENT_RANGE], "bytes */10");
    let challenged = get(&router, "/api/v1/challenged");
    assert_eq!(challenged.status, StatusCode::UNAUTHORIZED);
    assert_eq!(challenged.json()["code"], "unauthenticated");
    assert_eq!(
        challenged
            .headers
            .get_all(header::WWW_AUTHENTICATE)
            .iter()
            .count(),
        2
    );
    assert_eq!(
        challenged
            .headers
            .get_all(header::SET_COOKIE)
            .iter()
            .count(),
        2
    );
}
