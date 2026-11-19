// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Authentication at the HTTP boundary.
//!
//! The policy gate of each route reads the credential, resolves the client
//! address, runs the CSRF check where the contract asks for it and decides on
//! the policy. Only then does it hand the principal to the handler, through a
//! request extension that nothing else creates. A handler that asks for
//! [`CurrentPrincipal`] on a route that did not pass the gate fails before it
//! can do anything.

mod cookie;
mod credentials;
mod origin;

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::response::{IntoResponse, Response};
use server_api_types::ErrorCode;
use server_domain::credential::CredentialPort;
use server_domain::principal::Principal;

pub use cookie::{
    InvalidCookieValue, PLAIN_COOKIE_NAME, SECURE_COOKIE_NAME, clear_session_cookie,
    session_cookie_name, set_session_cookie,
};
pub use credentials::{CredentialsError, extract as extract_credential};
pub use origin::{ExternalOrigin, OriginError, Scheme};

use crate::error::error_response;

/// Everything the gate needs besides the policy of a route: the external
/// origin, the trusted proxies and the credential port.
pub struct SecurityContext {
    origin: ExternalOrigin,
    proxies: crate::proxy::TrustedProxies,
    credentials: Arc<dyn CredentialPort>,
}

impl SecurityContext {
    /// Assembles the context.
    pub fn new(
        origin: ExternalOrigin,
        proxies: crate::proxy::TrustedProxies,
        credentials: Arc<dyn CredentialPort>,
    ) -> Self {
        Self {
            origin,
            proxies,
            credentials,
        }
    }

    pub(crate) fn origin(&self) -> &ExternalOrigin {
        &self.origin
    }

    pub(crate) fn proxies(&self) -> &crate::proxy::TrustedProxies {
        &self.proxies
    }

    pub(crate) fn credentials(&self) -> &dyn CredentialPort {
        self.credentials.as_ref()
    }
}

/// The socket address of the peer. The code that serves connections puts it
/// into the request extensions; the gate rejects a request without it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PeerAddress(pub SocketAddr);

/// The client address as resolved by the gate. Handlers read it with
/// `axum::Extension<ClientIp>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClientIp(pub IpAddr);

/// The caller of the request. Only the policy gate creates it, so a handler
/// that extracts it is certain that the gate ran.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurrentPrincipal(Principal);

impl CurrentPrincipal {
    pub(crate) fn new(principal: Principal) -> Self {
        Self(principal)
    }

    /// The principal.
    pub fn principal(&self) -> &Principal {
        &self.0
    }
}

/// Rejection of a handler that asks for the principal without a gate.
#[derive(Debug, Clone, Copy)]
pub struct GateMissing;

impl IntoResponse for GateMissing {
    fn into_response(self) -> Response {
        error_response(ErrorCode::InternalError)
    }
}

impl<S: Send + Sync> FromRequestParts<S> for CurrentPrincipal {
    type Rejection = GateMissing;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<CurrentPrincipal>()
            .cloned()
            .ok_or(GateMissing)
    }
}
