// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! One route declaration: method, path, policy and contract.

use std::time::Duration;

use axum::extract::DefaultBodyLimit;
use axum::handler::Handler;
use axum::middleware;
use axum::routing::{self, MethodRouter};
use server_domain::policy::Policy;

use super::{PathTemplate, RegistryError};
use crate::router::{GateState, enforce_policy};

/// HTTP methods a route may use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Method {
    /// `GET`; also answers `HEAD`.
    Get,
    /// `POST`.
    Post,
    /// `PUT`.
    Put,
    /// `PATCH`.
    Patch,
    /// `DELETE`.
    Delete,
}

impl Method {
    /// Upper-case method name.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
            Self::Put => "PUT",
            Self::Patch => "PATCH",
            Self::Delete => "DELETE",
        }
    }

    /// Whether the method never changes state.
    pub fn is_safe(self) -> bool {
        self == Self::Get
    }
}

/// Type of a path or query parameter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamType {
    /// Arbitrary text.
    String,
    /// Unsigned integer.
    Integer,
    /// UUID in hyphenated form.
    Uuid,
    /// `true` or `false`.
    Boolean,
}

impl ParamType {
    /// Stable name for inventories.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::String => "string",
            Self::Integer => "integer",
            Self::Uuid => "uuid",
            Self::Boolean => "boolean",
        }
    }
}

/// A named, typed parameter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParamSpec {
    /// Parameter name; for path parameters the name used in the template.
    pub name: &'static str,
    /// Declared type.
    pub kind: ParamType,
}

/// Kind of request body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyKind {
    /// No body.
    None,
    /// A JSON document.
    Json,
}

impl BodyKind {
    /// Stable name for inventories.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Json => "json",
        }
    }
}

/// Kind of successful response.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResponseKind {
    /// A JSON document.
    Json,
    /// No body.
    Empty,
    /// A redirect.
    Redirect,
    /// A streamed body.
    Stream,
}

impl ResponseKind {
    /// Stable name for inventories.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::Empty => "empty",
            Self::Redirect => "redirect",
            Self::Stream => "stream",
        }
    }
}

/// How cross-site request forgery is handled for the route.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Csrf {
    /// Safe methods only; nothing to protect.
    NotApplicable,
    /// The request must pass the CSRF check.
    Required,
    /// A state-changing route that is deliberately exempt, with the reason.
    Exempt(&'static str),
}

impl Csrf {
    /// Stable name for inventories.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotApplicable => "not_applicable",
            Self::Required => "required",
            Self::Exempt(_) => "exempt",
        }
    }
}

/// Size and time limits of a route.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Largest accepted request body in bytes.
    pub max_body_bytes: u64,
    /// Time limit for handling the request.
    pub timeout: Duration,
}

impl Limits {
    /// Creates the limits; they are validated when the route is built.
    pub fn new(max_body_bytes: u64, timeout: Duration) -> Self {
        Self {
            max_body_bytes,
            timeout,
        }
    }
}

/// Everything a route promises besides its policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Contract {
    path_params: Vec<ParamSpec>,
    query_params: Vec<ParamSpec>,
    request_body: BodyKind,
    response: ResponseKind,
    success_statuses: Vec<u16>,
    csrf: Csrf,
    limits: Limits,
}

impl Contract {
    /// Starts a contract from the parts every route needs: the kind of
    /// response, the permitted success statuses (200 to 399) and the limits.
    /// The CSRF mode defaults to [`Csrf::NotApplicable`], which is only valid
    /// for safe methods.
    pub fn new(response: ResponseKind, success_statuses: &[u16], limits: Limits) -> Self {
        Self {
            path_params: Vec::new(),
            query_params: Vec::new(),
            request_body: BodyKind::None,
            response,
            success_statuses: success_statuses.to_vec(),
            csrf: Csrf::NotApplicable,
            limits,
        }
    }

    /// Declares a path parameter; every template parameter needs one.
    pub fn path_param(mut self, name: &'static str, kind: ParamType) -> Self {
        self.path_params.push(ParamSpec { name, kind });
        self
    }

    /// Declares a query parameter.
    pub fn query_param(mut self, name: &'static str, kind: ParamType) -> Self {
        self.query_params.push(ParamSpec { name, kind });
        self
    }

    /// Declares the kind of request body.
    pub fn request_body(mut self, kind: BodyKind) -> Self {
        self.request_body = kind;
        self
    }

    /// Sets the CSRF handling.
    pub fn csrf(mut self, csrf: Csrf) -> Self {
        self.csrf = csrf;
        self
    }

    /// Declared path parameters.
    pub fn path_params(&self) -> &[ParamSpec] {
        &self.path_params
    }

    /// Declared query parameters.
    pub fn query_params(&self) -> &[ParamSpec] {
        &self.query_params
    }

    /// Kind of request body.
    pub fn request_body_kind(&self) -> BodyKind {
        self.request_body
    }

    /// Kind of successful response.
    pub fn response(&self) -> ResponseKind {
        self.response
    }

    /// Permitted success statuses.
    pub fn success_statuses(&self) -> &[u16] {
        &self.success_statuses
    }

    /// CSRF handling.
    pub fn csrf_mode(&self) -> Csrf {
        self.csrf
    }

    /// Size and time limits.
    pub fn limits(&self) -> Limits {
        self.limits
    }
}

/// A validated route ready for registration.
pub struct Route {
    pub(crate) method: Method,
    pub(crate) path: PathTemplate,
    pub(crate) policy: Policy,
    pub(crate) contract: Contract,
    pub(crate) endpoint: MethodRouter,
}

impl Route {
    /// Declares a route. The policy and the contract are required arguments,
    /// so a route without them does not compile:
    ///
    /// ```compile_fail,E0061
    /// use std::time::Duration;
    /// use server_http::registry::{Contract, Limits, Method, ResponseKind, Route};
    ///
    /// async fn handler() -> &'static str { "ok" }
    ///
    /// let contract = Contract::new(
    ///     ResponseKind::Json,
    ///     &[200],
    ///     Limits::new(1024, Duration::from_secs(5)),
    /// );
    /// // The policy argument is missing.
    /// let _ = Route::new(Method::Get, "/api/v1/ping", contract, handler);
    /// ```
    ///
    /// The same call with a policy compiles:
    ///
    /// ```no_run
    /// use std::time::Duration;
    /// use server_domain::policy::Policy;
    /// use server_http::registry::{Contract, Limits, Method, ResponseKind, Route};
    ///
    /// async fn handler() -> &'static str { "ok" }
    ///
    /// let contract = Contract::new(
    ///     ResponseKind::Json,
    ///     &[200],
    ///     Limits::new(1024, Duration::from_secs(5)),
    /// );
    /// let policy = Policy::public("documentation example").unwrap();
    /// let _ = Route::new(Method::Get, "/api/v1/ping", policy, contract, handler);
    /// ```
    ///
    /// The handler is wrapped so that the policy runs before it and every
    /// response carries the marker the router checks at run time.
    pub fn new<H, T>(
        method: Method,
        path: &str,
        policy: Policy,
        contract: Contract,
        handler: H,
    ) -> Result<Self, RegistryError>
    where
        H: Handler<T, ()>,
        T: 'static,
    {
        let path = PathTemplate::parse(path).map_err(RegistryError::InvalidPath)?;
        validate(method, &path, &contract)?;
        let base: MethodRouter = match method {
            Method::Get => routing::get(handler),
            Method::Post => routing::post(handler),
            Method::Put => routing::put(handler),
            Method::Patch => routing::patch(handler),
            Method::Delete => routing::delete(handler),
        };
        let limit = usize::try_from(contract.limits.max_body_bytes).unwrap_or(usize::MAX);
        let gate = middleware::from_fn_with_state(GateState::new(policy.clone()), enforce_policy);
        // The body limit is the outer layer: it must be in place before the
        // handler reads the body.
        let gated: MethodRouter = base.layer(gate);
        let endpoint: MethodRouter = gated.layer(DefaultBodyLimit::max(limit));
        Ok(Self {
            method,
            path,
            policy,
            contract,
            endpoint,
        })
    }
}

fn validate(method: Method, path: &PathTemplate, contract: &Contract) -> Result<(), RegistryError> {
    let template = path.params();
    let declared: Vec<&str> = contract
        .path_params
        .iter()
        .map(|param| param.name)
        .collect();
    let mut sorted_declared = declared.clone();
    sorted_declared.sort_unstable();
    let mut sorted_template = template.clone();
    sorted_template.sort_unstable();
    if sorted_declared != sorted_template {
        return Err(RegistryError::PathParamMismatch(path.as_str().to_owned()));
    }
    let mut queries: Vec<&str> = contract
        .query_params
        .iter()
        .map(|param| param.name)
        .collect();
    queries.sort_unstable();
    if queries.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(RegistryError::DuplicateQueryParam(path.as_str().to_owned()));
    }
    if contract.success_statuses.is_empty()
        || contract
            .success_statuses
            .iter()
            .any(|status| !(200..=399).contains(status))
    {
        return Err(RegistryError::InvalidSuccessStatuses(
            path.as_str().to_owned(),
        ));
    }
    if method.is_safe() && contract.request_body != BodyKind::None {
        return Err(RegistryError::BodyOnSafeMethod(path.as_str().to_owned()));
    }
    let limits = contract.limits;
    if limits.timeout.is_zero()
        || (contract.request_body != BodyKind::None && limits.max_body_bytes == 0)
    {
        return Err(RegistryError::InvalidLimits(path.as_str().to_owned()));
    }
    let csrf_ok = match contract.csrf {
        Csrf::NotApplicable => method.is_safe(),
        Csrf::Required => !method.is_safe(),
        Csrf::Exempt(reason) => !method.is_safe() && !reason.trim().is_empty(),
    };
    if csrf_ok {
        Ok(())
    } else {
        Err(RegistryError::CsrfMismatch(path.as_str().to_owned()))
    }
}
