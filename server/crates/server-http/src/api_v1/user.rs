// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! `/api/v1/user`: the signed-in account, its devices and its second factors.
//!
//! Who may call follows the rules of the handlers they replace:
//!
//! - The account pages that change nothing about security (the profile of an account, the list
//!   of devices) name a right, so a token can be given it and the store window of the desktop
//!   client keeps what it shows. Only the profile of an account is part of the store window.
//! - The overview of sign-in methods, second factors and tokens is for a browser session only.
//! - Adding or removing a second factor needs a browser session with a recent sign-in: any
//!   sign-in at most 600 seconds ago counts, the first one of the session included. Today's
//!   handlers ask for a repeated confirmation instead; counting the first sign-in is a
//!   deliberate change.
//! - `GET /api/v1/user` answers `null` to a caller that is not signed in, and the web interface
//!   relies on that. It also answers `null` to a session that still owes its second factor and
//!   to a caller without `profile:read`; a browser session or the store window that holds the
//!   right gets the account, and so does a personal token that holds `profile:read`. A service
//!   token has no account and gets `null`. A policy cannot say "answer nothing" yet, so the
//!   route is public and its handler must resolve the credential itself, apply these rules and
//!   never answer more than `null` otherwise.
//!
//! Rules for the handlers that come later:
//!
//! - The delete routes (`DELETE /api/v1/user/client/{id}`, `DELETE /api/v1/user/mfa/webauthn`)
//!   stay within the account of the caller, refuse an id that is unknown there, and never delete
//!   a fallback element when the id matches nothing.
//! - A route that is guarded by a right only still needs an account behind the caller: a
//!   service token holds no account and is rejected by the handler.
//! - `POST /api/v1/user/mfa/totp/start` and `GET /api/v1/user` are marked `no_store`: the
//!   first answers the secret of the authenticator app, the second the account of the caller.
//! - The `created` field of a passkey is a timestamp string; the handler converts the
//!   milliseconds it stores today. This is a deliberate change of the wire form.

use server_api_types::contracts::auth::TotpRequest;
use server_api_types::contracts::user::{
    ClientList, CurrentUser, MfaStatus, PasskeyDelete, PasskeyName, SuperlevelStatus, TotpSetup,
    UserAuth, UserProfile, WebAuthnCreationOptions, WebAuthnRegistration,
};
use server_domain::policy::{MfaLevel, Policy, Requirement, Right, SessionKind};

use super::{JSON_BODY_LIMIT, limits, not_implemented, public, session_only};
use crate::registry::{BodyKind, Contract, Csrf, Method, ParamType, ResponseKind, Route};

const PROFILE_RIGHT: &str = "profile:read";
const CLIENTS_READ_RIGHT: &str = "clients:read";
const CLIENTS_REVOKE_RIGHT: &str = "clients:revoke";

/// How long after any sign-in, the first one included, a second factor may be changed, in
/// seconds.
const SUPERLEVEL_SECS: u32 = 10 * 60;

fn with_right(name: &'static str) -> Policy {
    let right = Right::new(name).expect("the right name is valid");
    Policy::protected(
        Requirement::authenticated()
            .require_rights(&[right])
            .expect("the list of rights is not empty"),
    )
}

/// A browser session whose sign-in is at most [`SUPERLEVEL_SECS`] old.
fn recent_sign_in() -> Policy {
    Policy::protected(
        Requirement::authenticated()
            .require_session(SessionKind::Interactive)
            .require_mfa(MfaLevel::Completed)
            .require_fresh_login_within(SUPERLEVEL_SECS)
            .expect("the freshness is not zero"),
    )
}

pub(super) fn routes() -> Vec<Route> {
    let mut routes = Vec::new();
    let mut add = |route: Result<Route, crate::registry::RegistryError>| {
        routes.push(route.expect("the route is valid"));
    };
    add(Route::new(
        Method::Get,
        "/api/v1/user",
        public(
            "answers the account of the caller, or null when nobody is signed in; the web interface asks before it knows who is there",
        ),
        Contract::new(ResponseKind::Json, &[200], limits(0))
            .response_shape::<CurrentUser>()
            .no_store(),
        not_implemented,
    ));
    add(Route::new(
        Method::Get,
        "/api/v1/user/{id}",
        with_right(PROFILE_RIGHT),
        Contract::new(ResponseKind::Json, &[200], limits(0))
            .path_param("id", ParamType::String)
            .response_shape::<UserProfile>(),
        not_implemented,
    ));
    add(Route::new(
        Method::Get,
        "/api/v1/user/auth",
        session_only(),
        Contract::new(ResponseKind::Json, &[200], limits(0)).response_shape::<UserAuth>(),
        not_implemented,
    ));
    add(Route::new(
        Method::Get,
        "/api/v1/user/client",
        with_right(CLIENTS_READ_RIGHT),
        Contract::new(ResponseKind::Json, &[200], limits(0)).response_shape::<ClientList>(),
        not_implemented,
    ));
    add(Route::new(
        Method::Delete,
        "/api/v1/user/client/{id}",
        with_right(CLIENTS_REVOKE_RIGHT),
        Contract::new(ResponseKind::Empty, &[204], limits(0))
            .path_param("id", ParamType::String)
            .csrf(Csrf::Required),
        not_implemented,
    ));
    add(Route::new(
        Method::Get,
        "/api/v1/user/mfa",
        session_only(),
        Contract::new(ResponseKind::Json, &[200], limits(0)).response_shape::<MfaStatus>(),
        not_implemented,
    ));
    add(Route::new(
        Method::Post,
        "/api/v1/user/mfa/totp/start",
        recent_sign_in(),
        Contract::new(ResponseKind::Json, &[200], limits(0))
            .csrf(Csrf::Required)
            .response_shape::<TotpSetup>()
            .no_store(),
        not_implemented,
    ));
    add(Route::new(
        Method::Post,
        "/api/v1/user/mfa/totp/finish",
        recent_sign_in(),
        Contract::new(ResponseKind::Empty, &[204], limits(JSON_BODY_LIMIT))
            .request_body(BodyKind::Json)
            .request_shape::<TotpRequest>()
            .csrf(Csrf::Required),
        not_implemented,
    ));
    add(Route::new(
        Method::Post,
        "/api/v1/user/mfa/webauthn/start",
        recent_sign_in(),
        Contract::new(ResponseKind::Json, &[200], limits(JSON_BODY_LIMIT))
            .request_body(BodyKind::Json)
            .request_shape::<PasskeyName>()
            .response_shape::<WebAuthnCreationOptions>()
            .csrf(Csrf::Required),
        not_implemented,
    ));
    add(Route::new(
        Method::Post,
        "/api/v1/user/mfa/webauthn/finish",
        recent_sign_in(),
        Contract::new(ResponseKind::Empty, &[204], limits(JSON_BODY_LIMIT))
            .request_body(BodyKind::Json)
            .request_shape::<WebAuthnRegistration>()
            .csrf(Csrf::Required),
        not_implemented,
    ));
    add(Route::new(
        Method::Delete,
        "/api/v1/user/mfa/webauthn",
        recent_sign_in(),
        Contract::new(ResponseKind::Empty, &[204], limits(JSON_BODY_LIMIT))
            .request_body(BodyKind::Json)
            .request_shape::<PasskeyDelete>()
            .csrf(Csrf::Required),
        not_implemented,
    ));
    // The handler today answers every method; only the web interface's `GET` is kept.
    add(Route::new(
        Method::Get,
        "/api/v1/user/superlevel",
        session_only(),
        Contract::new(ResponseKind::Json, &[200], limits(0)).response_shape::<SuperlevelStatus>(),
        not_implemented,
    ));
    routes
}
