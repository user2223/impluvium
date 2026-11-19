// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Route registry: the single source for the router and the route inventory.
//!
//! Each area of the API contributes a module that registers its routes; the
//! composition in [`application`] gathers them. The router and the inventory
//! are both derived from the resulting [`Registry`], so they cannot diverge.

mod path;
mod route;

use std::error::Error;
use std::fmt;
use std::sync::Arc;

use server_domain::policy::Policy;

pub use path::{PathError, PathTemplate};
pub use route::{
    BodyKind, Contract, Csrf, Limits, Method, ParamSpec, ParamType, ResponseKind, Route,
};

use crate::api_v1;
use crate::auth_paths;
use crate::emoji::{self, EmojiFiles};
use crate::health::{self, NoDependencies, ReadinessPort};
use crate::router::{FallbackInfo, fallbacks};

/// Why a route could not be built or registered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegistryError {
    /// The path template is malformed.
    InvalidPath(PathError),
    /// The same method and path were registered twice.
    DuplicateRoute {
        /// The method of both registrations.
        method: Method,
        /// The path of the second registration.
        path: String,
    },
    /// Two templates match the same requests but name their parameters differently.
    ConflictingPath {
        /// The template registered first.
        existing: String,
        /// The template rejected.
        new: String,
    },
    /// The declared path parameters differ from those of the template.
    PathParamMismatch(String),
    /// A query parameter is declared twice.
    DuplicateQueryParam(String),
    /// No success status, or one outside 200 to 399.
    InvalidSuccessStatuses(String),
    /// A safe method declares a request body.
    BodyOnSafeMethod(String),
    /// A zero timeout, or a zero body limit for a route with a body.
    InvalidLimits(String),
    /// The CSRF mode does not fit the method, or an exemption has no reason.
    CsrfMismatch(String),
}

impl fmt::Display for RegistryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPath(error) => write!(formatter, "invalid path: {error}"),
            Self::DuplicateRoute { method, path } => {
                write!(formatter, "duplicate route: {} {path}", method.as_str())
            }
            Self::ConflictingPath { existing, new } => {
                write!(formatter, "path {new} conflicts with {existing}")
            }
            Self::PathParamMismatch(path) => write!(
                formatter,
                "path parameters of {path} are not declared exactly"
            ),
            Self::DuplicateQueryParam(path) => {
                write!(formatter, "duplicate query parameter on {path}")
            }
            Self::InvalidSuccessStatuses(path) => {
                write!(formatter, "invalid success statuses on {path}")
            }
            Self::BodyOnSafeMethod(path) => {
                write!(formatter, "request body on a safe method at {path}")
            }
            Self::InvalidLimits(path) => write!(formatter, "invalid limits on {path}"),
            Self::CsrfMismatch(path) => {
                write!(formatter, "CSRF mode does not fit the method at {path}")
            }
        }
    }
}

impl Error for RegistryError {}

/// One entry of the route inventory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteInfo {
    /// Upper-case method name.
    pub method: &'static str,
    /// Path template as registered.
    pub path: String,
    /// Access policy.
    pub policy: Policy,
    /// Contract and limits.
    pub contract: Contract,
}

/// The routes of the server.
#[derive(Default)]
pub struct Registry {
    routes: Vec<Route>,
}

impl Registry {
    /// An empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a route. Rejects a second registration of the same method and
    /// path, and paths that name a shared parameter position
    /// differently.
    pub fn register(&mut self, route: Route) -> Result<(), RegistryError> {
        for existing in &self.routes {
            if existing.path.conflicts_with(&route.path) {
                return Err(RegistryError::ConflictingPath {
                    existing: existing.path.as_str().to_owned(),
                    new: route.path.as_str().to_owned(),
                });
            }
            if existing.path != route.path {
                continue;
            }
            if existing.method == route.method {
                return Err(RegistryError::DuplicateRoute {
                    method: route.method,
                    path: route.path.as_str().to_owned(),
                });
            }
        }
        self.routes.push(route);
        Ok(())
    }

    /// Number of registered routes.
    pub fn len(&self) -> usize {
        self.routes.len()
    }

    /// Whether no route is registered.
    pub fn is_empty(&self) -> bool {
        self.routes.is_empty()
    }

    /// The inventory of all registered routes, ordered by path and method.
    pub fn inventory(&self) -> Vec<RouteInfo> {
        let mut entries: Vec<(Method, RouteInfo)> = self
            .routes
            .iter()
            .map(|route| {
                (
                    route.method,
                    RouteInfo {
                        method: route.method.as_str(),
                        path: route.path.as_str().to_owned(),
                        policy: route.policy.clone(),
                        contract: route.contract.clone(),
                    },
                )
            })
            .collect();
        entries.sort_by(|a, b| (&a.1.path, a.0).cmp(&(&b.1.path, b.0)));
        entries.into_iter().map(|(_, info)| info).collect()
    }

    /// The built-in answers for unknown paths and unsupported methods.
    pub fn fallbacks(&self) -> Vec<FallbackInfo> {
        fallbacks()
    }

    pub(crate) fn into_routes(self) -> Vec<Route> {
        self.routes
    }
}

/// The registry of the running server. Area modules register their routes
/// here: the health probes, the routes under `/auth`, the contracts of the server, sign-in and
/// administration routes, and the emoji pictures.
/// This variant has no dependencies to wait for, so the readiness probe always
/// answers ready, and it holds no emoji pictures.
pub fn application() -> Registry {
    application_with(Arc::new(NoDependencies), Arc::new(EmojiFiles::empty()))
}

/// The registry of the running server with the readiness port that the
/// readiness probe consults and the emoji pictures the emoji route serves.
pub fn application_with(readiness: Arc<dyn ReadinessPort>, emoji: Arc<EmojiFiles>) -> Registry {
    let mut registry = Registry::new();
    let routes = health::routes(readiness)
        .into_iter()
        .chain(auth_paths::routes())
        .chain(api_v1::routes())
        .chain(emoji::routes(emoji));
    for route in routes {
        registry
            .register(route)
            .expect("the built-in routes do not conflict");
    }
    registry
}
