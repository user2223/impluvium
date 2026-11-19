// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Request identity and request logging.
//!
//! The outermost layer gives every request an identifier, returns it in the
//! `X-Request-Id` response header and hands one [`RequestLogEntry`] per
//! request to a [`RequestLogSink`]. The entry has no field for headers or the
//! query string, so credentials cannot reach a log through it. The writer of
//! the log lives in the executable; this crate only knows the trait.

use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::extract::{MatchedPath, Request, State};
use axum::http::{HeaderName, HeaderValue};
use axum::middleware::Next;
use axum::response::Response;

use crate::request_id::RequestId;

/// What is known about a finished request. It carries no header values and
/// no query string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestLogEntry {
    /// The identifier assigned to the request.
    pub request_id: String,
    /// The HTTP method, cut to 16 characters.
    pub method: String,
    /// The route template that matched, such as `/api/v1/items/{id}`, or
    /// `(unmatched)`. Raw request paths are never logged because they can
    /// carry identifiers and tokens.
    pub path: String,
    /// The response status.
    pub status: u16,
    /// Time from the start of the request to the response head.
    pub duration: Duration,
}

/// Receives one entry per request.
pub trait RequestLogSink: Send + Sync {
    /// Called once the response head is ready.
    fn request(&self, entry: &RequestLogEntry);
}

/// A sink that drops every entry.
#[derive(Debug, Clone, Copy, Default)]
pub struct DiscardLogs;

impl RequestLogSink for DiscardLogs {
    fn request(&self, _entry: &RequestLogEntry) {}
}

/// The header that carries the request id.
pub const REQUEST_ID_HEADER: HeaderName = HeaderName::from_static("x-request-id");

const UNMATCHED: &str = "(unmatched)";

/// Layer function: assigns the id, runs the request, sets the response
/// header and logs.
pub(crate) async fn observe(
    State(sink): State<Arc<dyn RequestLogSink>>,
    mut request: Request,
    next: Next,
) -> Response {
    let started = Instant::now();
    let id = RequestId::generate();
    request.extensions_mut().insert(id.clone());
    let method: String = request.method().as_str().chars().take(16).collect();
    let path = request
        .extensions()
        .get::<MatchedPath>()
        .map_or(UNMATCHED, MatchedPath::as_str)
        .to_owned();
    let mut response = next.run(request).await;
    if let Ok(value) = HeaderValue::from_str(id.as_str()) {
        response.headers_mut().insert(REQUEST_ID_HEADER, value);
    }
    sink.request(&RequestLogEntry {
        request_id: id.as_str().to_owned(),
        method,
        path,
        status: response.status().as_u16(),
        duration: started.elapsed(),
    });
    response
}
