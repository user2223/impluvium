// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Turning errors into the web error body.
//!
//! A handler or extractor fails with an [`HttpError`]. The response it
//! produces is still empty and only carries the code; the layer assembled by
//! the router renders the JSON body once the request id is known. The same
//! layer also normalises error responses that bypassed this type (for
//! example rejections of framework extractors, which carry texts that may
//! echo request input) so that every error answer has the same shape and
//! never repeats input.

use axum::body::Body;
use axum::extract::Request;
use axum::http::{HeaderMap, HeaderName, HeaderValue, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use server_api_types::{ApiError, ErrorCode, FieldError};

use crate::request_id::RequestId;

/// A failure with a stable code and, for validation errors, the invalid fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpError {
    code: ErrorCode,
    fields: Vec<FieldError>,
}

impl HttpError {
    /// An error with a code and no field details.
    pub fn new(code: ErrorCode) -> Self {
        Self {
            code,
            fields: Vec::new(),
        }
    }

    /// A validation error that names the invalid fields.
    pub fn with_fields(code: ErrorCode, fields: Vec<FieldError>) -> Self {
        Self { code, fields }
    }

    /// The code.
    pub fn code(&self) -> ErrorCode {
        self.code
    }
}

/// The code and fields of an error response whose body is not rendered yet.
#[derive(Debug, Clone)]
struct Pending {
    code: ErrorCode,
    fields: Vec<FieldError>,
}

fn status_of(code: ErrorCode) -> StatusCode {
    StatusCode::from_u16(code.status()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR)
}

impl IntoResponse for HttpError {
    fn into_response(self) -> Response {
        let mut response = Response::new(Body::empty());
        *response.status_mut() = status_of(self.code);
        response.extensions_mut().insert(Pending {
            code: self.code,
            fields: self.fields,
        });
        response
    }
}

/// An error response with a code and no field details.
pub(crate) fn error_response(code: ErrorCode) -> Response {
    HttpError::new(code).into_response()
}

/// Response headers that belong to the status and survive the rendering,
/// with all their values.
const KEPT: [HeaderName; 5] = [
    header::ALLOW,
    header::RETRY_AFTER,
    header::WWW_AUTHENTICATE,
    header::CONTENT_RANGE,
    header::SET_COOKIE,
];

/// Marks a response that carries its own error envelope or a page rendered
/// by the handler, such as the answer of a protocol endpoint. The error
/// layer passes it through unchanged. Every response without this marker
/// that has an error status is rewritten into the web error shape.
#[derive(Debug, Clone)]
pub struct PreformattedError<R>(pub R);

#[derive(Debug, Clone, Copy)]
struct PreformattedMarker;

impl<R: IntoResponse> IntoResponse for PreformattedError<R> {
    fn into_response(self) -> Response {
        let mut response = self.0.into_response();
        response.extensions_mut().insert(PreformattedMarker);
        response
    }
}

/// The generic code for a status of a response that carries no code. The
/// response keeps its status; the code only names it.
fn code_for_status(status: StatusCode) -> ErrorCode {
    match status.as_u16() {
        401 => ErrorCode::Unauthenticated,
        403 => ErrorCode::Forbidden,
        404 => ErrorCode::NotFound,
        405 => ErrorCode::MethodNotAllowed,
        408 => ErrorCode::RequestTimeout,
        413 => ErrorCode::PayloadTooLarge,
        415 => ErrorCode::UnsupportedMediaType,
        503 => ErrorCode::NotReady,
        400..=499 => ErrorCode::BadRequest,
        _ => ErrorCode::InternalError,
    }
}

fn render(
    status: StatusCode,
    code: ErrorCode,
    fields: &[FieldError],
    request_id: &str,
    kept: HeaderMap,
) -> Response {
    let body = serde_json::to_vec(&ApiError::new(code, request_id, fields)).unwrap_or_default();
    let mut response = Response::new(Body::from(body));
    *response.status_mut() = status;
    let headers = response.headers_mut();
    for (name, value) in &kept {
        headers.append(name.clone(), value.clone());
    }
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

/// Renders pending errors and normalises other error responses. It reads the
/// request id the outer layer assigned.
pub(crate) async fn render_errors(request: Request, next: Next) -> Response {
    let id = request.extensions().get::<RequestId>().cloned();
    let request_id = id.as_ref().map_or("", RequestId::as_str).to_owned();
    let mut response = next.run(request).await;
    let pending = response.extensions_mut().remove::<Pending>();
    let status = response.status();
    let failed = status.is_client_error() || status.is_server_error();
    let preformatted = response.extensions().get::<PreformattedMarker>().is_some();
    if pending.is_none() && (!failed || preformatted) {
        return response;
    }
    let mut kept = HeaderMap::new();
    for name in &KEPT {
        for value in response.headers().get_all(name) {
            kept.append(name.clone(), value.clone());
        }
    }
    match pending {
        Some(Pending { code, fields }) => render(status_of(code), code, &fields, &request_id, kept),
        None => render(status, code_for_status(status), &[], &request_id, kept),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statuses_map_to_generic_codes() {
        for status in [400, 401, 403, 404, 405, 408, 413, 415, 500, 503] {
            let code = code_for_status(StatusCode::from_u16(status).unwrap());
            assert_eq!(code.status(), status);
        }
        assert_eq!(
            code_for_status(StatusCode::TOO_MANY_REQUESTS),
            ErrorCode::BadRequest
        );
        assert_eq!(
            code_for_status(StatusCode::BAD_GATEWAY),
            ErrorCode::InternalError
        );
    }

    #[test]
    fn http_error_response_carries_the_status_and_no_body_yet() {
        let response = HttpError::new(ErrorCode::Forbidden).into_response();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert!(response.extensions().get::<Pending>().is_some());
    }
}
