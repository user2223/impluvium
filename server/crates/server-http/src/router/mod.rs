// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Assembles the framework router from the registry.
//!
//! Default deny: only registered routes exist. Every registered route runs
//! its policy gate. Unknown paths and unsupported methods are answered by
//! built-in public handlers with an API error, never by a page, unless the
//! options carry the web application: then the [`spa`](crate::spa) module
//! answers what no route took. A final
//! layer turns any response that bypassed a policy into an internal error.
//! Around the routes sit, from the inside out: the panic guard, the error
//! renderer, the protective headers and the request identity with logging.

mod gate;

use std::collections::BTreeMap;
use std::sync::Arc;

use axum::extract::Request;
use axum::http::{StatusCode, header};
use axum::middleware;
use axum::response::Response;
use axum::routing::MethodRouter;
use axum::{Extension, Router};
use server_api_types::ErrorCode;
use server_domain::policy::Policy;

use crate::auth::SecurityContext;
use crate::error::{error_response, render_errors};
use crate::guard::catch_panics;
use crate::headers::{HeaderPolicy, ScriptHash, apply};
use crate::observe::{DiscardLogs, RequestLogSink, observe};
use crate::registry::{Method, Registry};
use crate::spa::{self, SpaFiles};

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

/// The assembled application router.
pub struct ApplicationRouter {
    pub(crate) router: Router,
}

impl ApplicationRouter {
    /// The framework router, for the code that serves connections and for
    /// tests of the whole stack. Callers outside this crate use it only to
    /// serve it.
    #[doc(hidden)]
    pub fn into_router(self) -> Router {
        self.router
    }
}

/// What the layers around the routes need besides the routes themselves.
#[derive(Clone)]
pub struct AppOptions {
    /// Receives one entry per request.
    pub log_sink: Arc<dyn RequestLogSink>,
    /// Hashes of the inline scripts of the single-page application shell.
    pub script_hashes: Vec<ScriptHash>,
    /// The files of the web application. Requests that no route takes are answered from them;
    /// without them, such requests get the API error. The executable always sets them.
    pub spa: Option<Arc<SpaFiles>>,
}

impl Default for AppOptions {
    fn default() -> Self {
        Self {
            log_sink: Arc::new(DiscardLogs),
            script_hashes: Vec::new(),
            spa: None,
        }
    }
}

/// Builds the router from the registry with default options.
pub fn build(registry: Registry, context: SecurityContext) -> ApplicationRouter {
    build_with(registry, context, &AppOptions::default())
}

/// Builds the router from the registry. The security context reaches every
/// route gate, including the built-in fallbacks; a gate that does not find
/// it answers with an internal error.
pub fn build_with(
    registry: Registry,
    context: SecurityContext,
    options: &AppOptions,
) -> ApplicationRouter {
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
    let router = match &options.spa {
        Some(files) => {
            let files = Arc::clone(files);
            router.fallback(move |request: Request| {
                let files = Arc::clone(&files);
                async move { mark(spa::respond(&files, &request)) }
            })
        }
        None => router.fallback(|| async { mark(error_response(ErrorCode::NotFound)) }),
    };
    ApplicationRouter {
        router: finish(enforce(router.layer(Extension(Arc::new(context)))), options),
    }
}

/// Adds the layers around the routes: the panic guard, the error renderer,
/// the protective headers, and the request identity with logging outermost.
pub(crate) fn finish(router: Router, options: &AppOptions) -> Router {
    let policy = Arc::new(HeaderPolicy::new(&options.script_hashes));
    router
        .layer(middleware::from_fn(catch_panics))
        .layer(middleware::from_fn(render_errors))
        .layer(middleware::from_fn_with_state(policy, apply))
        .layer(middleware::from_fn_with_state(
            Arc::clone(&options.log_sink),
            observe,
        ))
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
    let mut response = error_response(ErrorCode::MethodNotAllowed);
    if let Ok(value) = header::HeaderValue::from_str(&allow) {
        response.headers_mut().insert(header::ALLOW, value);
    }
    mark(response)
}

#[cfg(test)]
mod auth_tests;
#[cfg(test)]
mod tests;
