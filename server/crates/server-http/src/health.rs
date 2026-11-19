// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Liveness and readiness probes.
//!
//! `GET /api/health/live` says the process serves requests and is always
//! `200` while it does. `GET /api/health/ready` answers `200` only when the
//! injected [`ReadinessPort`] reports every dependency available, and `503`
//! with the web error shape otherwise. Both are public and appear in the
//! route inventory.

use std::sync::Arc;
use std::time::Duration;

use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use server_api_types::ErrorCode;
use server_domain::credential::BoxFuture;
use server_domain::policy::Policy;

use crate::error::HttpError;
use crate::registry::{Contract, Limits, Method, ResponseKind, Route};

/// Reports whether every dependency of the server is available.
pub trait ReadinessPort: Send + Sync {
    /// `true` when the server can handle requests that need its dependencies.
    fn ready(&self) -> BoxFuture<'_, bool>;
}

/// The port of a server without dependencies: always ready.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoDependencies;

impl ReadinessPort for NoDependencies {
    fn ready(&self) -> BoxFuture<'_, bool> {
        Box::pin(async { true })
    }
}

fn contract() -> Contract {
    Contract::new(
        ResponseKind::Json,
        &[200],
        Limits::new(0, Duration::from_secs(5)),
    )
}

fn status_body(status: &'static str) -> Response {
    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "application/json"),
            (header::CACHE_CONTROL, "no-store"),
        ],
        status,
    )
        .into_response()
}

/// The two probe routes.
///
/// # Panics
///
/// Never in practice: paths, reasons and contracts are fixed and valid, which
/// the tests of this module check.
pub fn routes(readiness: Arc<dyn ReadinessPort>) -> Vec<Route> {
    let live = Route::new(
        Method::Get,
        "/api/health/live",
        public("liveness probe for supervisors and load balancers"),
        contract(),
        || async { status_body("{\"status\":\"live\"}") },
    )
    .expect("the liveness route is valid");
    let ready = Route::new(
        Method::Get,
        "/api/health/ready",
        public("readiness probe for supervisors and load balancers"),
        contract(),
        move || {
            let readiness = Arc::clone(&readiness);
            async move {
                if readiness.ready().await {
                    status_body("{\"status\":\"ready\"}")
                } else {
                    HttpError::new(ErrorCode::NotReady).into_response()
                }
            }
        },
    )
    .expect("the readiness route is valid");
    vec![live, ready]
}

fn public(reason: &'static str) -> Policy {
    Policy::public(reason).expect("the reason is not empty")
}
