// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! The policy gate and the run-time marker that proves it ran.

use std::sync::Arc;

use axum::extract::{Request, State};
use axum::http::{StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use server_domain::credential::Credential;
use server_domain::policy::{Decision, Policy};
use server_domain::principal::Principal;

use crate::auth::{ClientIp, CurrentPrincipal, PeerAddress, SecurityContext, extract_credential};
use crate::csrf;
use crate::proxy;
use crate::registry::Csrf;

/// Present in the extensions of every response that went through a policy.
#[derive(Debug, Clone, Copy)]
pub(crate) struct PolicyApplied;

/// The policy of one route, shared with its gate.
#[derive(Clone)]
pub(crate) struct GateState {
    policy: Arc<Policy>,
    csrf: Csrf,
}

impl GateState {
    pub(crate) fn new(policy: Policy, csrf: Csrf) -> Self {
        Self {
            policy: Arc::new(policy),
            csrf,
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

/// Runs the gate of a route before its handler: client address, credential,
/// CSRF check where the contract demands it, and the policy decision. The
/// handler only runs when all of them pass.
pub(crate) async fn enforce_policy(
    State(state): State<GateState>,
    request: Request,
    next: Next,
) -> Response {
    let response = match admit(&state, request).await {
        Ok(request) => next.run(request).await,
        Err(Reject(status, code)) => error_response(status, code),
    };
    mark(response)
}

/// Why the gate turned a request away: status and stable error code.
struct Reject(StatusCode, &'static str);

const INTERNAL: Reject = Reject(StatusCode::INTERNAL_SERVER_ERROR, "internal_error");

async fn admit(state: &GateState, mut request: Request) -> Result<Request, Reject> {
    let Some(context) = request.extensions().get::<Arc<SecurityContext>>().cloned() else {
        return Err(INTERNAL);
    };
    let Some(PeerAddress(peer)) = request.extensions().get::<PeerAddress>().copied() else {
        return Err(INTERNAL);
    };
    let client_ip = proxy::client_ip(context.proxies(), peer, request.headers())
        .map_err(|_| Reject(StatusCode::BAD_REQUEST, "invalid_forwarding_headers"))?;
    let credential = extract_credential(request.headers(), context.origin())
        .map_err(|_| Reject(StatusCode::UNAUTHORIZED, "invalid_credentials"))?;
    // Without a bearer token the check runs first, so a request from another
    // origin never reaches the credential port. A bearer token is exempt only
    // when it resolves to a principal; an unresolved one is checked as well.
    let needs_check = state.csrf == Csrf::Required;
    let is_bearer = matches!(credential, Some(Credential::Bearer(_)));
    if needs_check && !is_bearer {
        check_csrf(&request, &context)?;
    }
    let resolved = match &credential {
        Some(credential) => resolve(&context, credential).await,
        None => None,
    };
    if needs_check && is_bearer && resolved.is_none() {
        check_csrf(&request, &context)?;
    }
    let principal = resolved.unwrap_or(Principal::Anonymous);
    match state.policy.decide(&principal) {
        Decision::Allow => {}
        Decision::Unauthenticated => {
            return Err(Reject(StatusCode::UNAUTHORIZED, "unauthenticated"));
        }
        Decision::Forbidden => return Err(Reject(StatusCode::FORBIDDEN, "forbidden")),
    }
    request.extensions_mut().insert(ClientIp(client_ip));
    request
        .extensions_mut()
        .insert(CurrentPrincipal::new(principal));
    Ok(request)
}

fn check_csrf(request: &Request, context: &SecurityContext) -> Result<(), Reject> {
    csrf::check(request.headers(), context.origin())
        .map_err(|_| Reject(StatusCode::FORBIDDEN, "csrf_rejected"))
}

/// Resolves a credential. Unknown, expired and revoked credentials give no
/// principal, and neither does an answer of the wrong kind: a cookie only
/// resolves to a session, a bearer token only to a token or a device.
async fn resolve(context: &SecurityContext, credential: &Credential) -> Option<Principal> {
    let principal = context.credentials().resolve(credential).await.ok()?;
    let fits = matches!(
        (credential, &principal),
        (Credential::SessionCookie(_), Principal::Session(_))
            | (
                Credential::Bearer(_),
                Principal::Token(_) | Principal::Device(_)
            )
    );
    fits.then_some(principal)
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
