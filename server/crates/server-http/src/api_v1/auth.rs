// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! `/api/v1/auth`: sign-in methods, second factors, single sign-on, passkeys and registration.
//!
//! Routes that start or finish a sign-in carry the session cookie: see
//! [`Contract::sets_session_cookie`].

use server_api_types::contracts::EmptyObject;
use server_api_types::contracts::auth::{
    AuthInfo, MfaMethods, OidcCallbackQuery, OidcLogoutForm, OidcLogoutResponse, SigninRequest,
    SigninResponse, SignupAccount, SignupInvitation, SignupQuery, SignupRequest, TotpRequest,
    WebAuthnAssertion, WebAuthnRequestOptions,
};
use server_domain::policy::{MfaLevel, Policy, Requirement, SessionKind};

use super::{JSON_BODY_LIMIT, limits, not_implemented, public};
use crate::registry::{BodyKind, Contract, Csrf, Method, ParamType, ResponseKind, Route};

/// A browser session whose password step is done and whose second factor is still missing.
fn mfa_pending() -> Policy {
    Policy::protected(
        Requirement::authenticated()
            .require_session(SessionKind::Interactive)
            .require_mfa(MfaLevel::Pending),
    )
}

fn json_post(limit: u64) -> Contract {
    Contract::new(ResponseKind::Json, &[200], limits(limit)).csrf(Csrf::Required)
}

fn json_body_post() -> Contract {
    json_post(JSON_BODY_LIMIT).request_body(BodyKind::Json)
}

pub(super) fn routes() -> Vec<Route> {
    let mut routes = Vec::new();
    let mut add = |route: Result<Route, crate::registry::RegistryError>| {
        routes.push(route.expect("the route is valid"));
    };
    add(Route::new(
        Method::Get,
        "/api/v1/auth",
        public("tells the sign-in page which methods are on"),
        Contract::new(ResponseKind::Json, &[200], limits(0)).response_shape::<AuthInfo>(),
        not_implemented,
    ));
    add(Route::new(
        Method::Get,
        "/api/v1/auth/mfa",
        mfa_pending(),
        Contract::new(ResponseKind::Json, &[200], limits(0)).response_shape::<MfaMethods>(),
        not_implemented,
    ));
    add(Route::new(
        Method::Post,
        "/api/v1/auth/mfa/totp",
        mfa_pending(),
        json_body_post()
            .request_shape::<TotpRequest>()
            .response_shape::<EmptyObject>(),
        not_implemented,
    ));
    add(Route::new(
        Method::Post,
        "/api/v1/auth/mfa/webauthn/start",
        mfa_pending(),
        json_post(0).response_shape::<WebAuthnRequestOptions>(),
        not_implemented,
    ));
    add(Route::new(
        Method::Post,
        "/api/v1/auth/mfa/webauthn/finish",
        mfa_pending(),
        json_body_post()
            .request_shape::<WebAuthnAssertion>()
            .response_shape::<EmptyObject>(),
        not_implemented,
    ));
    add(Route::new(
        Method::Get,
        "/api/v1/auth/oidc/callback",
        public("the address the identity provider sends the browser back to after sign-in"),
        Contract::new(ResponseKind::Redirect, &[302], limits(0))
            .query_param("code", ParamType::String)
            .query_param("state", ParamType::String)
            .query_shape::<OidcCallbackQuery>()
            .sets_session_cookie(),
        not_implemented,
    ));
    add(Route::new(
        Method::Post,
        "/api/v1/auth/oidc/logout",
        public(
            "the back-channel notification of the identity provider; the signed logout token is the proof",
        ),
        Contract::new(ResponseKind::Json, &[200], limits(JSON_BODY_LIMIT))
            .request_body(BodyKind::Form)
            .request_shape::<OidcLogoutForm>()
            .response_shape::<OidcLogoutResponse>()
            .csrf(Csrf::Exempt(
                "the identity provider posts from its own server and sends no browser origin",
            )),
        not_implemented,
    ));
    add(Route::new(
        Method::Post,
        "/api/v1/auth/passkey/start",
        public("starts a sign-in with a passkey, before any account is known"),
        json_post(0)
            .response_shape::<WebAuthnRequestOptions>()
            .sets_session_cookie(),
        not_implemented,
    ));
    add(Route::new(
        Method::Post,
        "/api/v1/auth/passkey/finish",
        public("completes a sign-in with a passkey; the signed answer is the proof"),
        json_body_post()
            .request_shape::<WebAuthnAssertion>()
            .response_shape::<EmptyObject>()
            .sets_session_cookie(),
        not_implemented,
    ));
    add(Route::new(
        Method::Post,
        "/api/v1/auth/signin/simple",
        public("signs in with a name and a password"),
        json_body_post()
            .request_shape::<SigninRequest>()
            .response_shape::<SigninResponse>()
            .sets_session_cookie(),
        not_implemented,
    ));
    add(Route::new(
        Method::Get,
        "/api/v1/auth/signup/simple",
        public(
            "shows the registration page what its invitation fixes; the invitation id is the secret",
        ),
        Contract::new(ResponseKind::Json, &[200], limits(0))
            .query_param("id", ParamType::String)
            .query_shape::<SignupQuery>()
            .response_shape::<SignupInvitation>(),
        not_implemented,
    ));
    add(Route::new(
        Method::Post,
        "/api/v1/auth/signup/simple",
        public("registers an account with an invitation; the invitation id is the secret"),
        json_body_post()
            .request_shape::<SignupRequest>()
            .response_shape::<SignupAccount>(),
        not_implemented,
    ));
    routes
}
