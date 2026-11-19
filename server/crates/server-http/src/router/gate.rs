// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! The policy gate and the run-time marker that proves it ran.

use std::sync::Arc;

use axum::extract::{Request, State};
use axum::http::{StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use server_domain::policy::{Decision, Policy};

/// Present in the extensions of every response that went through a policy.
#[derive(Debug, Clone, Copy)]
pub(crate) struct PolicyApplied;

/// The policy of one route, shared with its gate.
#[derive(Clone)]
pub(crate) struct GateState {
    policy: Arc<Policy>,
}

impl GateState {
    pub(crate) fn new(policy: Policy) -> Self {
        Self {
            policy: Arc::new(policy),
        }
    }
}

/// Marks a response as produced under a policy.
pub(crate) fn mark(mut response: Response) -> Response {
    response.extensions_mut().insert(PolicyApplied);
    response
}

/// Minimal JSON error body. Stable error codes and the full error contract
/// replace this in the error layer.
pub(crate) fn error_response(status: StatusCode, code: &'static str) -> Response {
    (
        status,
        [(header::CONTENT_TYPE, "application/json")],
        format!("{{\"code\":\"{code}\"}}"),
    )
        .into_response()
}

/// Evaluates the route policy before the handler runs. Callers cannot be
/// authenticated yet, so protected routes deny.
pub(crate) async fn enforce_policy(
    State(state): State<GateState>,
    request: Request,
    next: Next,
) -> Response {
    let response = match state.policy.decide_anonymous() {
        Decision::Allow => next.run(request).await,
        Decision::Unauthenticated => error_response(StatusCode::UNAUTHORIZED, "unauthenticated"),
    };
    mark(response)
}

/// Outermost layer: a response without the marker means some path reached
/// the client without a policy, which is an internal error.
pub(crate) async fn require_marker(request: Request, next: Next) -> Response {
    let response = next.run(request).await;
    if response.extensions().get::<PolicyApplied>().is_some() {
        response
    } else {
        error_response(StatusCode::INTERNAL_SERVER_ERROR, "internal_error")
    }
}
