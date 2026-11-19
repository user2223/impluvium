// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Assembles the framework router from the registry.
//!
//! Default deny: only registered routes exist. Every registered route runs
//! its policy gate. Unknown paths and unsupported methods are answered by
//! built-in public handlers with an API error, never by a page. A final
//! layer turns any response that bypassed a policy into an internal error.

mod gate;

use std::collections::BTreeMap;
use std::sync::Arc;

use axum::http::{StatusCode, header};
use axum::middleware;
use axum::response::Response;
use axum::routing::MethodRouter;
use axum::{Extension, Router};
use server_domain::policy::Policy;

use crate::auth::SecurityContext;
use crate::registry::{Method, Registry};

pub(crate) use gate::error_response;
pub(crate) use gate::{GateState, enforce_policy};
use gate::{mark, require_marker};

const UNKNOWN_PATH_REASON: &str = "unknown paths answer with an API error";
const METHOD_NOT_ALLOWED_REASON: &str = "unsupported methods answer with an API error";

/// A built-in answer that is not a registered route.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FallbackInfo {
    /// Stable name of the case.
    pub kind: &'static str,
    /// Status of the answer.
    pub status: u16,
    /// Public policy with its justification.
    pub policy: Policy,
}

pub(crate) fn fallbacks() -> Vec<FallbackInfo> {
    [
        ("unknown_path", StatusCode::NOT_FOUND, UNKNOWN_PATH_REASON),
        (
            "method_not_allowed",
            StatusCode::METHOD_NOT_ALLOWED,
            METHOD_NOT_ALLOWED_REASON,
        ),
    ]
    .into_iter()
    .filter_map(|(kind, status, reason)| {
        Policy::public(reason).ok().map(|policy| FallbackInfo {
            kind,
            status: status.as_u16(),
            policy,
        })
    })
    .collect()
}

/// The assembled application router. The framework type stays private.
pub struct ApplicationRouter {
    #[allow(dead_code)] // Read by the server start-up once it exists, and by the tests.
    pub(crate) router: Router,
}

/// Builds the router from the registry. The security context reaches every
/// route gate, including the built-in fallbacks; a gate that does not find
/// it answers with an internal error.
pub fn build(registry: Registry, context: SecurityContext) -> ApplicationRouter {
    let mut grouped: BTreeMap<String, (Vec<Method>, MethodRouter)> = BTreeMap::new();
    for route in registry.into_routes() {
        let path = route.path.as_str().to_owned();
        match grouped.remove(&path) {
            Some((mut methods, existing)) => {
                methods.push(route.method);
                grouped.insert(path, (methods, existing.merge(route.endpoint)));
            }
            None => {
                grouped.insert(path, (vec![route.method], route.endpoint));
            }
        }
    }
    let mut router = Router::new();
    for (path, (methods, endpoint)) in grouped {
        let allow = allow_header(&methods);
        let endpoint = endpoint.fallback(move || {
            let allow = allow.clone();
            async move { method_not_allowed(allow) }
        });
        router = router.route(&path, endpoint);
    }
    let router =
        router.fallback(|| async { mark(error_response(StatusCode::NOT_FOUND, "not_found")) });
    ApplicationRouter {
        router: enforce(router.layer(Extension(Arc::new(context)))),
    }
}

/// Adds the final marker check. Every router that serves requests passes
/// through here, including fallbacks.
pub(crate) fn enforce(router: Router) -> Router {
    router.layer(middleware::from_fn(require_marker))
}

fn allow_header(methods: &[Method]) -> String {
    let mut names: Vec<&str> = methods.iter().map(|method| method.as_str()).collect();
    if methods.contains(&Method::Get) {
        names.push("HEAD");
    }
    names.sort_unstable();
    names.join(", ")
}

fn method_not_allowed(allow: String) -> Response {
    let mut response = error_response(StatusCode::METHOD_NOT_ALLOWED, "method_not_allowed");
    if let Ok(value) = header::HeaderValue::from_str(&allow) {
        response.headers_mut().insert(header::ALLOW, value);
    }
    mark(response)
}

#[cfg(test)]
mod auth_tests;
#[cfg(test)]
mod tests;
