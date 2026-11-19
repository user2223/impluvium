// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! What the request logging layer hands to its sink.

mod common;

use std::sync::{Arc, Mutex};

use common::{get, router_with, send};
use server_http::observe::{RequestLogEntry, RequestLogSink};
use server_http::router::AppOptions;

#[derive(Default)]
struct Capture(Mutex<Vec<RequestLogEntry>>);

impl RequestLogSink for Capture {
    fn request(&self, entry: &RequestLogEntry) {
        self.0.lock().unwrap().push(entry.clone());
    }
}

fn capture() -> (Arc<Capture>, axum::Router) {
    let sink = Arc::new(Capture::default());
    let options = AppOptions {
        log_sink: sink.clone(),
        ..AppOptions::default()
    };
    (sink, router_with(common::registry(), &options))
}

#[test]
fn entries_carry_the_route_template_and_no_secrets() {
    let (sink, router) = capture();
    let reply = send(
        &router,
        "GET",
        "/api/v1/items/7?token=SECRET-QUERY",
        &[
            ("authorization", "Bearer SECRET-BEARER"),
            ("cookie", "session=SECRET-COOKIE"),
            ("proxy-authorization", "Basic SECRET-PROXY"),
        ],
        "",
    );
    assert_eq!(reply.status, 200);
    let entries = sink.0.lock().unwrap();
    assert_eq!(entries.len(), 1);
    let entry = &entries[0];
    assert_eq!(entry.method, "GET");
    assert_eq!(entry.path, "/api/v1/items/{id}");
    assert_eq!(entry.status, 200);
    assert_eq!(reply.headers["x-request-id"], entry.request_id.as_str());
    let dump = format!("{entries:?}");
    for secret in [
        "SECRET-QUERY",
        "SECRET-BEARER",
        "SECRET-COOKIE",
        "SECRET-PROXY",
        "token",
    ] {
        assert!(!dump.contains(secret), "{secret} in {dump}");
    }
}

#[test]
fn unmatched_paths_are_not_logged_verbatim_and_panics_log_a_500() {
    let (sink, router) = capture();
    get(&router, "/api/v1/SECRET-IN-PATH");
    let panicked = get(&router, "/api/v1/boom");
    assert_eq!(panicked.status, 500);
    let entries = sink.0.lock().unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].path, "(unmatched)");
    assert_eq!(entries[0].status, 404);
    assert_eq!(entries[1].path, "/api/v1/boom");
    assert_eq!(entries[1].status, 500);
    let dump = format!("{entries:?}");
    assert!(!dump.contains("SECRET-IN-PATH") && !dump.contains("SECRET-PANIC-PAYLOAD"));
}
