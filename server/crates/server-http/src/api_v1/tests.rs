// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

use std::collections::{BTreeSet, HashMap};
use std::net::SocketAddr;
use std::sync::Arc;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{HeaderMap, Request, StatusCode};
use server_domain::credential::{BoxFuture, Credential, CredentialError, CredentialPort};
use server_domain::permissions::{RightSet, role_rights};
use server_domain::policy::Right;
use server_domain::principal::{
    Principal, Role, Session, SessionClass, SessionStage, Token, TokenKind,
};
use tower::ServiceExt;

use super::routes;
use crate::auth::{ExternalOrigin, PeerAddress, Scheme, SecurityContext};
use crate::proxy::TrustedProxies;
use crate::registry::{self, Registry};
use crate::router::{AppOptions, build_with};

const SESSION: &str = "__Host-impluvium_session";
const ORIGIN: &str = "https://app.example.test";

#[path = "catalog_tests.rs"]
mod catalog_tests;

struct Table(HashMap<&'static str, Principal>);

impl CredentialPort for Table {
    fn resolve<'a>(
        &'a self,
        credential: &'a Credential,
    ) -> BoxFuture<'a, Result<Principal, CredentialError>> {
        let answer = self
            .0
            .get(credential.secret())
            .cloned()
            .ok_or(CredentialError::Unknown);
        Box::pin(async move { answer })
    }
}

fn session(role: Role, stage: SessionStage) -> Principal {
    Principal::Session(Session::new(role, stage, SessionClass::Browser, 5))
}

fn token(granted: &[&'static str], owner: RightSet) -> Principal {
    let granted: Vec<Right> = granted.iter().map(|n| Right::new(n).unwrap()).collect();
    Principal::Token(Token::new(
        TokenKind::Service,
        &RightSet::from_rights(&granted),
        &owner,
    ))
}

fn table() -> Table {
    let done = SessionStage::Authenticated;
    let setup = RightSet::from_rights(&[Right::new("setup:run").unwrap()]);
    let mut entries = HashMap::new();
    entries.insert("user", session(Role::User, done));
    entries.insert("admin", session(Role::Admin, done));
    entries.insert("pending", session(Role::User, SessionStage::MfaPending));
    entries.insert(
        "admin-pending",
        session(Role::Admin, SessionStage::MfaPending),
    );
    entries.insert(
        "tok-settings",
        token(&["settings:read"], role_rights(Role::User)),
    );
    entries.insert("tok-emoji", token(&["emoji:read"], role_rights(Role::User)));
    entries.insert(
        "store",
        Principal::Session(Session::new(
            Role::User,
            SessionStage::Authenticated,
            SessionClass::Store,
            5,
        )),
    );
    entries.insert("setup", token(&["setup:run"], setup));
    entries.insert(
        "tok-library-add",
        token(&["library:add"], role_rights(Role::User)),
    );
    entries.insert(
        "tok-library-remove",
        token(&["library:remove"], role_rights(Role::User)),
    );
    entries.insert(
        "tok-collection-add",
        token(&["collections:add"], role_rights(Role::User)),
    );
    entries.insert(
        "tok-admin",
        token(&["admin:manage"], role_rights(Role::Admin)),
    );
    entries.insert(
        "tok-profile",
        token(&["profile:read"], role_rights(Role::User)),
    );
    entries.insert(
        "tok-clients",
        token(&["clients:read"], role_rights(Role::User)),
    );
    entries.insert(
        "tok-revoke",
        token(&["clients:revoke"], role_rights(Role::User)),
    );
    // A browser session whose sign-in is older than the ten minutes a second factor may be
    // changed in.
    entries.insert(
        "old",
        Principal::Session(Session::new(
            Role::User,
            SessionStage::Authenticated,
            SessionClass::Browser,
            601,
        )),
    );
    Table(entries)
}

fn built() -> Registry {
    let mut registry = Registry::new();
    for route in routes() {
        registry.register(route).unwrap();
    }
    registry
}

fn router() -> Router {
    let context = SecurityContext::new(
        ExternalOrigin::new(Scheme::Https, "app.example.test", None).unwrap(),
        TrustedProxies::default(),
        Arc::new(table()),
    );
    build_with(built(), context, &AppOptions::default()).into_router()
}

struct Reply {
    status: StatusCode,
    headers: HeaderMap,
    body: Vec<u8>,
}

impl Reply {
    fn code(&self) -> String {
        let value: serde_json::Value = serde_json::from_slice(&self.body).unwrap();
        value["code"].as_str().unwrap().to_owned()
    }
}

/// An id for the `{id}` of a path template.
const ID: &str = "3f2b8c1e-0000-4000-8000-000000000000";

fn send(router: &Router, method: &str, target: &str, headers: &[(&str, String)]) -> Reply {
    let target = target.replace("{id}", ID);
    let mut builder = Request::builder()
        .method(method)
        .uri(target.as_str())
        .extension(PeerAddress(SocketAddr::from(([192, 0, 2, 1], 4000))));
    for (name, value) in headers {
        builder = builder.header(*name, value);
    }
    let request = builder.body(Body::empty()).unwrap();
    let router = router.clone();
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(async move {
            let response = router.oneshot(request).await.unwrap();
            let (parts, body) = response.into_parts();
            let bytes = to_bytes(body, usize::MAX).await.unwrap();
            Reply {
                status: parts.status,
                headers: parts.headers,
                body: bytes.to_vec(),
            }
        })
}

fn cache_control(reply: &Reply) -> Option<&str> {
    reply
        .headers
        .get("cache-control")
        .map(|value| value.to_str().unwrap())
}

fn cookie(value: &str) -> (&'static str, String) {
    ("cookie", format!("{SESSION}={value}"))
}

fn bearer(value: &str) -> (&'static str, String) {
    ("authorization", format!("Bearer {value}"))
}

fn origin() -> (&'static str, String) {
    ("origin", ORIGIN.to_owned())
}

/// Calls a route as a browser would: a state-changing method carries the origin.
fn call(router: &Router, method: &str, path: &str, credential: Option<(&str, String)>) -> Reply {
    let mut headers: Vec<(&str, String)> = credential.into_iter().collect();
    if method != "GET" {
        headers.push(origin());
    }
    send(router, method, path, &headers)
}

/// Method, path of every route here, with whether it is public.
const ROUTES: [(&str, &str, bool); 66] = [
    ("GET", "/api/v1", true),
    ("GET", "/api/v1/settings", false),
    ("POST", "/api/v1/setup", false),
    ("GET", "/api/v1/auth", true),
    ("GET", "/api/v1/auth/mfa", false),
    ("POST", "/api/v1/auth/mfa/totp", false),
    ("POST", "/api/v1/auth/mfa/webauthn/start", false),
    ("POST", "/api/v1/auth/mfa/webauthn/finish", false),
    ("GET", "/api/v1/auth/oidc/callback", true),
    ("POST", "/api/v1/auth/oidc/logout", true),
    ("POST", "/api/v1/auth/passkey/start", true),
    ("POST", "/api/v1/auth/passkey/finish", true),
    ("POST", "/api/v1/auth/signin/simple", true),
    ("GET", "/api/v1/auth/signup/simple", true),
    ("POST", "/api/v1/auth/signup/simple", true),
    ("GET", "/api/v1/admin/auth", false),
    ("GET", "/api/v1/admin/auth/invitation", false),
    ("POST", "/api/v1/admin/auth/invitation", false),
    ("DELETE", "/api/v1/admin/auth/invitation", false),
    ("GET", "/api/v1/user", true),
    ("GET", "/api/v1/user/{id}", false),
    ("GET", "/api/v1/user/auth", false),
    ("GET", "/api/v1/user/client", false),
    ("DELETE", "/api/v1/user/client/{id}", false),
    ("GET", "/api/v1/user/mfa", false),
    ("POST", "/api/v1/user/mfa/totp/start", false),
    ("POST", "/api/v1/user/mfa/totp/finish", false),
    ("POST", "/api/v1/user/mfa/webauthn/start", false),
    ("POST", "/api/v1/user/mfa/webauthn/finish", false),
    ("DELETE", "/api/v1/user/mfa/webauthn", false),
    ("GET", "/api/v1/user/superlevel", false),
    ("GET", "/api/v1/user/token", false),
    ("POST", "/api/v1/user/token", false),
    ("DELETE", "/api/v1/user/token/{id}", false),
    ("GET", "/api/v1/user/token/acls", false),
    ("GET", "/api/v1/token", true),
    ("GET", "/api/v1/admin/token", false),
    ("POST", "/api/v1/admin/token", false),
    ("DELETE", "/api/v1/admin/token/{id}", false),
    ("GET", "/api/v1/admin/token/acls", false),
    ("GET", "/api/v1/admin/users", false),
    ("GET", "/api/v1/admin/users/{id}", false),
    ("DELETE", "/api/v1/admin/users/{id}", false),
    ("GET", "/api/v1/store", false),
    ("GET", "/api/v1/store/featured", false),
    ("GET", "/api/v1/store/tags", false),
    ("GET", "/api/v1/games/{id}", false),
    ("GET", "/api/v1/companies/{id}", false),
    ("GET", "/api/v1/tags/{id}", false),
    ("GET", "/api/v1/collection", false),
    ("POST", "/api/v1/collection", false),
    ("GET", "/api/v1/collection/default", false),
    ("POST", "/api/v1/collection/default/entry", false),
    ("DELETE", "/api/v1/collection/default/entry", false),
    ("GET", "/api/v1/collection/{id}", false),
    ("DELETE", "/api/v1/collection/{id}", false),
    ("POST", "/api/v1/collection/{id}/entry", false),
    ("DELETE", "/api/v1/collection/{id}/entry", false),
    ("GET", "/api/v1/object/{id}", true),
    ("POST", "/api/v1/object/{id}", true),
    ("DELETE", "/api/v1/object/{id}", true),
    ("GET", "/api/v1/screenshots", false),
    ("GET", "/api/v1/screenshots/{id}", false),
    ("DELETE", "/api/v1/screenshots/{id}", false),
    ("GET", "/api/v1/screenshots/game/{id}", false),
    ("POST", "/api/v1/screenshots/game/{id}", false),
];

/// Method, path, response kind, success status, request body, request shape, query shape,
/// response shape.
type Expected = (
    &'static str,
    &'static str,
    &'static str,
    u16,
    &'static str,
    Option<&'static str>,
    Option<&'static str>,
    Option<&'static str>,
);

#[test]
fn the_server_and_sign_in_routes_are_registered_with_their_contracts() {
    let expected: [Expected; 19] = [
        (
            "GET",
            "/api/v1",
            "json",
            200,
            "none",
            None,
            None,
            Some("ServerInfo"),
        ),
        (
            "GET",
            "/api/v1/settings",
            "json",
            200,
            "none",
            None,
            None,
            Some("ServerSettings"),
        ),
        (
            "POST",
            "/api/v1/setup",
            "empty",
            204,
            "none",
            None,
            None,
            None,
        ),
        (
            "GET",
            "/api/v1/auth",
            "json",
            200,
            "none",
            None,
            None,
            Some("AuthInfo"),
        ),
        (
            "GET",
            "/api/v1/auth/mfa",
            "json",
            200,
            "none",
            None,
            None,
            Some("MfaMethods"),
        ),
        (
            "POST",
            "/api/v1/auth/mfa/totp",
            "json",
            200,
            "json",
            Some("TotpRequest"),
            None,
            Some("EmptyObject"),
        ),
        (
            "POST",
            "/api/v1/auth/mfa/webauthn/start",
            "json",
            200,
            "none",
            None,
            None,
            Some("WebAuthnRequestOptions"),
        ),
        (
            "POST",
            "/api/v1/auth/mfa/webauthn/finish",
            "json",
            200,
            "json",
            Some("WebAuthnAssertion"),
            None,
            Some("EmptyObject"),
        ),
        (
            "GET",
            "/api/v1/auth/oidc/callback",
            "redirect",
            302,
            "none",
            None,
            Some("OidcCallbackQuery"),
            None,
        ),
        (
            "POST",
            "/api/v1/auth/oidc/logout",
            "json",
            200,
            "form",
            Some("OidcLogoutForm"),
            None,
            Some("OidcLogoutResponse"),
        ),
        (
            "POST",
            "/api/v1/auth/passkey/start",
            "json",
            200,
            "none",
            None,
            None,
            Some("WebAuthnRequestOptions"),
        ),
        (
            "POST",
            "/api/v1/auth/passkey/finish",
            "json",
            200,
            "json",
            Some("WebAuthnAssertion"),
            None,
            Some("EmptyObject"),
        ),
        (
            "POST",
            "/api/v1/auth/signin/simple",
            "json",
            200,
            "json",
            Some("SigninRequest"),
            None,
            Some("SigninResponse"),
        ),
        (
            "GET",
            "/api/v1/auth/signup/simple",
            "json",
            200,
            "none",
            None,
            Some("SignupQuery"),
            Some("SignupInvitation"),
        ),
        (
            "POST",
            "/api/v1/auth/signup/simple",
            "json",
            200,
            "json",
            Some("SignupRequest"),
            None,
            Some("SignupAccount"),
        ),
        (
            "GET",
            "/api/v1/admin/auth",
            "json",
            200,
            "none",
            None,
            None,
            Some("AdminAuthInfo"),
        ),
        (
            "GET",
            "/api/v1/admin/auth/invitation",
            "json",
            200,
            "none",
            None,
            None,
            Some("InvitationList"),
        ),
        (
            "POST",
            "/api/v1/admin/auth/invitation",
            "json",
            200,
            "json",
            Some("InvitationCreate"),
            None,
            Some("Invitation"),
        ),
        (
            "DELETE",
            "/api/v1/admin/auth/invitation",
            "json",
            200,
            "json",
            Some("InvitationDelete"),
            None,
            Some("EmptyObject"),
        ),
    ];
    let inventory = built().inventory();
    assert_eq!(inventory.len(), ROUTES.len());
    for (method, path, response, status, body, request, query, answer) in expected {
        let info = inventory
            .iter()
            .find(|info| info.method == method && info.path == path)
            .unwrap_or_else(|| panic!("{method} {path} is not registered"));
        let contract = &info.contract;
        assert_eq!(contract.response().as_str(), response, "{method} {path}");
        assert_eq!(contract.success_statuses(), [status], "{method} {path}");
        assert_eq!(
            contract.request_body_kind().as_str(),
            body,
            "{method} {path}"
        );
        assert_eq!(contract.request_shape_name(), request, "{method} {path}");
        assert_eq!(contract.query_shape_name(), query, "{method} {path}");
        assert_eq!(contract.response_shape_name(), answer, "{method} {path}");
        let public = ROUTES
            .iter()
            .find(|(m, p, _)| *m == method && *p == path)
            .unwrap()
            .2;
        assert_eq!(
            info.policy.public_reason().is_some(),
            public,
            "{method} {path}"
        );
    }
}

#[test]
fn every_named_shape_is_an_exported_type() {
    let exported: BTreeSet<String> = server_api_types::export::declared_names()
        .unwrap()
        .into_iter()
        .collect();
    for info in built().inventory() {
        for name in [
            info.contract.request_shape_name(),
            info.contract.query_shape_name(),
            info.contract.response_shape_name(),
        ]
        .into_iter()
        .flatten()
        {
            assert!(
                exported.contains(name),
                "{} {}: {name}",
                info.method,
                info.path
            );
        }
    }
}

#[test]
fn public_routes_answer_anonymous_callers_with_the_missing_implementation() {
    let router = router();
    for (method, path, public) in ROUTES.iter().filter(|route| route.2) {
        let reply = call(&router, method, path, None);
        assert_eq!(
            reply.status,
            StatusCode::NOT_IMPLEMENTED,
            "{method} {path} {public}"
        );
        assert_eq!(reply.code(), "not_implemented", "{method} {path}");
        assert!(reply.headers.get("set-cookie").is_none(), "{method} {path}");
    }
}

#[test]
fn protected_routes_ask_anonymous_callers_to_sign_in() {
    let router = router();
    for (method, path, _) in ROUTES.iter().filter(|route| !route.2) {
        let reply = call(&router, method, path, None);
        assert_eq!(reply.status, StatusCode::UNAUTHORIZED, "{method} {path}");
        assert_eq!(reply.code(), "unauthenticated", "{method} {path}");
    }
}

#[test]
fn second_factor_routes_accept_only_a_pending_session() {
    let router = router();
    let steps = [
        ("GET", "/api/v1/auth/mfa"),
        ("POST", "/api/v1/auth/mfa/totp"),
        ("POST", "/api/v1/auth/mfa/webauthn/start"),
        ("POST", "/api/v1/auth/mfa/webauthn/finish"),
    ];
    for (method, path) in steps {
        let pending = call(&router, method, path, Some(cookie("pending")));
        assert_eq!(
            pending.status,
            StatusCode::NOT_IMPLEMENTED,
            "{method} {path}"
        );
        for finished in ["user", "admin"] {
            let reply = call(&router, method, path, Some(cookie(finished)));
            assert_eq!(
                reply.status,
                StatusCode::FORBIDDEN,
                "{method} {path} {finished}"
            );
            assert_eq!(reply.code(), "forbidden");
        }
        let by_token = call(&router, method, path, Some(bearer("tok-admin")));
        assert_eq!(by_token.status, StatusCode::FORBIDDEN, "{method} {path}");
    }
}

#[test]
fn administration_routes_pin_todays_rule_the_admin_right_without_the_setup_token() {
    // Today's rule, not the final one: how the first-run setup reaches these routes is open.
    let router = router();
    let paths = [
        ("GET", "/api/v1/admin/auth"),
        ("GET", "/api/v1/admin/auth/invitation"),
        ("POST", "/api/v1/admin/auth/invitation"),
        ("DELETE", "/api/v1/admin/auth/invitation"),
    ];
    for (method, path) in paths {
        for (who, credential, status) in [
            ("admin", cookie("admin"), 501),
            ("admin token", bearer("tok-admin"), 501),
            ("user", cookie("user"), 403),
            ("pending admin", cookie("admin-pending"), 403),
            ("setup token", bearer("setup"), 403),
        ] {
            let reply = call(&router, method, path, Some(credential));
            assert_eq!(reply.status.as_u16(), status, "{method} {path} {who}");
        }
    }
}

#[test]
fn settings_need_their_right_and_setup_needs_the_setup_token() {
    let router = router();
    for (who, credential, status) in [
        ("user", cookie("user"), 501),
        ("admin", cookie("admin"), 501),
        ("store window", cookie("store"), 501),
        ("token with the right", bearer("tok-settings"), 501),
        ("token without the right", bearer("tok-emoji"), 403),
        ("pending", cookie("pending"), 403),
    ] {
        let reply = call(&router, "GET", "/api/v1/settings", Some(credential));
        assert_eq!(reply.status.as_u16(), status, "{who}");
    }
    for (who, credential, status) in [
        ("setup token", bearer("setup"), 501),
        ("admin token", bearer("tok-admin"), 403),
        ("admin session", cookie("admin"), 403),
        ("user session", cookie("user"), 403),
    ] {
        let reply = call(&router, "POST", "/api/v1/setup", Some(credential));
        assert_eq!(reply.status.as_u16(), status, "{who}");
    }
}

#[test]
fn state_changing_routes_need_the_origin_except_the_logout_notification() {
    let router = router();
    let exempt = "/api/v1/auth/oidc/logout";
    for (method, path, _) in ROUTES.iter().filter(|route| route.0 != "GET") {
        let reply = send(&router, method, path, &[]);
        if *path == exempt {
            assert_eq!(reply.status, StatusCode::NOT_IMPLEMENTED, "{path}");
        } else {
            assert_eq!(reply.status, StatusCode::FORBIDDEN, "{method} {path}");
            assert_eq!(reply.code(), "csrf_rejected", "{method} {path}");
        }
    }
}

#[test]
fn only_the_sign_in_routes_set_the_session_cookie() {
    let listed: BTreeSet<(&str, String)> = built()
        .inventory()
        .into_iter()
        .filter(|info| info.contract.session_cookie_is_set())
        .map(|info| (info.method, info.path))
        .collect();
    let expected: BTreeSet<(&str, String)> = [
        ("GET", "/api/v1/auth/oidc/callback"),
        ("POST", "/api/v1/auth/passkey/start"),
        ("POST", "/api/v1/auth/passkey/finish"),
        ("POST", "/api/v1/auth/signin/simple"),
    ]
    .into_iter()
    .map(|(method, path)| (method, path.to_owned()))
    .collect();
    assert_eq!(listed, expected);
}

#[test]
fn other_methods_get_the_method_error() {
    let router = router();
    for (path, method) in [
        ("/api/v1", "POST"),
        ("/api/v1/settings", "DELETE"),
        ("/api/v1/setup", "GET"),
        ("/api/v1/auth/signin/simple", "GET"),
        ("/api/v1/admin/auth", "POST"),
        ("/api/v1/auth/oidc/callback", "POST"),
    ] {
        let reply = call(&router, method, path, None);
        assert_eq!(
            reply.status,
            StatusCode::METHOD_NOT_ALLOWED,
            "{method} {path}"
        );
        assert_eq!(reply.code(), "method_not_allowed");
    }
}

#[test]
fn the_application_registry_keeps_the_entry_routes_next_to_the_new_ones() {
    let inventory = registry::application().inventory();
    let paths: BTreeSet<&str> = inventory.iter().map(|info| info.path.as_str()).collect();
    for path in [
        "/auth/oidc",
        "/auth/signout",
        "/auth/callback/oidc",
        "/api/v1",
        "/api/v1/auth/signin/simple",
    ] {
        assert!(paths.contains(path), "{path}");
    }
    assert_eq!(inventory.len(), 6 + ROUTES.len());
}

/// Method, path, response kind, success status, request body, request shape, response shape,
/// path parameters (name and type).
type AccountExpected = (
    &'static str,
    &'static str,
    &'static str,
    u16,
    &'static str,
    Option<&'static str>,
    Option<&'static str>,
    &'static [(&'static str, &'static str)],
);

#[test]
fn the_account_token_and_user_administration_routes_are_registered_with_their_contracts() {
    let id_string: &[(&str, &str)] = &[("id", "string")];
    let id_uuid: &[(&str, &str)] = &[("id", "uuid")];
    let expected: [AccountExpected; 24] = [
        (
            "GET",
            "/api/v1/user",
            "json",
            200,
            "none",
            None,
            Some("CurrentUser"),
            &[],
        ),
        (
            "GET",
            "/api/v1/user/{id}",
            "json",
            200,
            "none",
            None,
            Some("UserProfile"),
            id_string,
        ),
        (
            "GET",
            "/api/v1/user/auth",
            "json",
            200,
            "none",
            None,
            Some("UserAuth"),
            &[],
        ),
        (
            "GET",
            "/api/v1/user/client",
            "json",
            200,
            "none",
            None,
            Some("ClientList"),
            &[],
        ),
        (
            "DELETE",
            "/api/v1/user/client/{id}",
            "empty",
            204,
            "none",
            None,
            None,
            id_string,
        ),
        (
            "GET",
            "/api/v1/user/mfa",
            "json",
            200,
            "none",
            None,
            Some("MfaStatus"),
            &[],
        ),
        (
            "POST",
            "/api/v1/user/mfa/totp/start",
            "json",
            200,
            "none",
            None,
            Some("TotpSetup"),
            &[],
        ),
        (
            "POST",
            "/api/v1/user/mfa/totp/finish",
            "empty",
            204,
            "json",
            Some("TotpRequest"),
            None,
            &[],
        ),
        (
            "POST",
            "/api/v1/user/mfa/webauthn/start",
            "json",
            200,
            "json",
            Some("PasskeyName"),
            Some("WebAuthnCreationOptions"),
            &[],
        ),
        (
            "POST",
            "/api/v1/user/mfa/webauthn/finish",
            "empty",
            204,
            "json",
            Some("WebAuthnRegistration"),
            None,
            &[],
        ),
        (
            "DELETE",
            "/api/v1/user/mfa/webauthn",
            "empty",
            204,
            "json",
            Some("PasskeyDelete"),
            None,
            &[],
        ),
        (
            "GET",
            "/api/v1/user/superlevel",
            "json",
            200,
            "none",
            None,
            Some("SuperlevelStatus"),
            &[],
        ),
        (
            "GET",
            "/api/v1/user/token",
            "json",
            200,
            "none",
            None,
            Some("ApiTokenList"),
            &[],
        ),
        (
            "POST",
            "/api/v1/user/token",
            "json",
            200,
            "json",
            Some("TokenCreate"),
            Some("NewApiToken"),
            &[],
        ),
        (
            "DELETE",
            "/api/v1/user/token/{id}",
            "empty",
            204,
            "none",
            None,
            None,
            id_uuid,
        ),
        (
            "GET",
            "/api/v1/user/token/acls",
            "json",
            200,
            "none",
            None,
            Some("AclDescriptions"),
            &[],
        ),
        (
            "GET",
            "/api/v1/token",
            "json",
            200,
            "none",
            None,
            Some("GrantedAcls"),
            &[],
        ),
        (
            "GET",
            "/api/v1/admin/token",
            "json",
            200,
            "none",
            None,
            Some("ApiTokenList"),
            &[],
        ),
        (
            "POST",
            "/api/v1/admin/token",
            "json",
            200,
            "json",
            Some("TokenCreate"),
            Some("NewApiToken"),
            &[],
        ),
        (
            "DELETE",
            "/api/v1/admin/token/{id}",
            "empty",
            204,
            "none",
            None,
            None,
            id_uuid,
        ),
        (
            "GET",
            "/api/v1/admin/token/acls",
            "json",
            200,
            "none",
            None,
            Some("AclDescriptions"),
            &[],
        ),
        (
            "GET",
            "/api/v1/admin/users",
            "json",
            200,
            "none",
            None,
            Some("AdminUserList"),
            &[],
        ),
        (
            "GET",
            "/api/v1/admin/users/{id}",
            "json",
            200,
            "none",
            None,
            Some("UserAccount"),
            id_string,
        ),
        (
            "DELETE",
            "/api/v1/admin/users/{id}",
            "json",
            200,
            "none",
            None,
            Some("UserDeleted"),
            id_string,
        ),
    ];
    let inventory = built().inventory();
    for (method, path, response, status, body, request, answer, params) in expected {
        let info = inventory
            .iter()
            .find(|info| info.method == method && info.path == path)
            .unwrap_or_else(|| panic!("{method} {path} is not registered"));
        let contract = &info.contract;
        assert_eq!(contract.response().as_str(), response, "{method} {path}");
        assert_eq!(contract.success_statuses(), [status], "{method} {path}");
        assert_eq!(
            contract.request_body_kind().as_str(),
            body,
            "{method} {path}"
        );
        assert_eq!(contract.request_shape_name(), request, "{method} {path}");
        assert_eq!(contract.query_shape_name(), None, "{method} {path}");
        assert_eq!(contract.response_shape_name(), answer, "{method} {path}");
        assert!(!contract.session_cookie_is_set(), "{method} {path}");
        let declared: Vec<(&str, &str)> = contract
            .path_params()
            .iter()
            .map(|param| (param.name, param.kind.as_str()))
            .collect();
        assert_eq!(declared, params, "{method} {path}");
        let public = ROUTES
            .iter()
            .find(|(m, p, _)| *m == method && *p == path)
            .unwrap()
            .2;
        assert_eq!(
            info.policy.public_reason().is_some(),
            public,
            "{method} {path}"
        );
    }
}

/// The status of each caller on each route: a credential and what the route answers.
fn assert_matrix(
    router: &Router,
    routes: &[(&str, &str)],
    callers: &[(&str, Option<&str>, bool, u16)],
) {
    for (method, path) in routes {
        for (who, credential, is_cookie, status) in callers {
            let credential = credential.map(|name| {
                if *is_cookie {
                    cookie(name)
                } else {
                    bearer(name)
                }
            });
            let reply = call(router, method, path, credential);
            assert_eq!(reply.status.as_u16(), *status, "{method} {path} {who}");
        }
    }
}

#[test]
fn overview_and_token_routes_of_the_account_are_for_a_finished_browser_session_only() {
    let router = router();
    let routes = [
        ("GET", "/api/v1/user/auth"),
        ("GET", "/api/v1/user/mfa"),
        ("GET", "/api/v1/user/superlevel"),
        ("GET", "/api/v1/user/token"),
        ("POST", "/api/v1/user/token"),
        ("DELETE", "/api/v1/user/token/{id}"),
        ("GET", "/api/v1/user/token/acls"),
    ];
    let callers = [
        ("user", Some("user"), true, 501),
        ("admin", Some("admin"), true, 501),
        ("old sign-in", Some("old"), true, 501),
        ("pending", Some("pending"), true, 403),
        ("store window", Some("store"), true, 403),
        ("admin token", Some("tok-admin"), false, 403),
        ("token with a right", Some("tok-profile"), false, 403),
        ("anonymous", None, true, 401),
    ];
    assert_matrix(&router, &routes, &callers);
}

#[test]
fn changing_a_second_factor_needs_a_recent_sign_in() {
    let router = router();
    let routes = [
        ("POST", "/api/v1/user/mfa/totp/start"),
        ("POST", "/api/v1/user/mfa/totp/finish"),
        ("POST", "/api/v1/user/mfa/webauthn/start"),
        ("POST", "/api/v1/user/mfa/webauthn/finish"),
        ("DELETE", "/api/v1/user/mfa/webauthn"),
    ];
    let callers = [
        ("recent user", Some("user"), true, 501),
        ("recent admin", Some("admin"), true, 501),
        ("old sign-in", Some("old"), true, 403),
        ("pending", Some("pending"), true, 403),
        ("store window", Some("store"), true, 403),
        ("admin token", Some("tok-admin"), false, 403),
        ("anonymous", None, true, 401),
    ];
    assert_matrix(&router, &routes, &callers);
}

#[test]
fn profiles_devices_and_the_store_window_follow_their_rights() {
    let router = router();
    // A profile is part of the store window; devices are not.
    let profile = [("GET", "/api/v1/user/{id}")];
    let callers = [
        ("user", Some("user"), true, 501),
        ("store window", Some("store"), true, 501),
        ("token with the right", Some("tok-profile"), false, 501),
        ("token without the right", Some("tok-emoji"), false, 403),
        ("pending", Some("pending"), true, 403),
        ("anonymous", None, true, 401),
    ];
    assert_matrix(&router, &profile, &callers);
    let list = [("GET", "/api/v1/user/client")];
    let callers = [
        ("user", Some("user"), true, 501),
        ("admin", Some("admin"), true, 501),
        ("store window", Some("store"), true, 403),
        ("token with the right", Some("tok-clients"), false, 501),
        ("token with the other right", Some("tok-revoke"), false, 403),
        ("pending", Some("pending"), true, 403),
        ("anonymous", None, true, 401),
    ];
    assert_matrix(&router, &list, &callers);
    let revoke = [("DELETE", "/api/v1/user/client/{id}")];
    let callers = [
        ("user", Some("user"), true, 501),
        ("store window", Some("store"), true, 403),
        ("token with the right", Some("tok-revoke"), false, 501),
        (
            "token with the other right",
            Some("tok-clients"),
            false,
            403,
        ),
        ("anonymous", None, true, 401),
    ];
    assert_matrix(&router, &revoke, &callers);
}

#[test]
fn the_current_user_and_the_granted_rights_are_public_for_every_credential() {
    let router = router();
    for path in ["/api/v1/user", "/api/v1/token"] {
        for credential in [
            None,
            Some(cookie("user")),
            Some(cookie("pending")),
            Some(bearer("tok-emoji")),
        ] {
            let reply = call(&router, "GET", path, credential);
            assert_eq!(reply.status, StatusCode::NOT_IMPLEMENTED, "{path}");
        }
    }
}

#[test]
fn administration_of_accounts_needs_the_admin_right() {
    let router = router();
    let routes = [
        ("GET", "/api/v1/admin/users"),
        ("GET", "/api/v1/admin/users/{id}"),
        ("DELETE", "/api/v1/admin/users/{id}"),
    ];
    let callers = [
        ("admin", Some("admin"), true, 501),
        ("admin token", Some("tok-admin"), false, 501),
        ("user", Some("user"), true, 403),
        ("store window", Some("store"), true, 403),
        ("pending admin", Some("admin-pending"), true, 403),
        ("token without the right", Some("tok-settings"), false, 403),
        ("anonymous", None, true, 401),
    ];
    assert_matrix(&router, &routes, &callers);
}

#[test]
fn administration_of_tokens_needs_the_admin_right_in_a_browser_session() {
    let router = router();
    let routes = [
        ("GET", "/api/v1/admin/token"),
        ("POST", "/api/v1/admin/token"),
        ("DELETE", "/api/v1/admin/token/{id}"),
        ("GET", "/api/v1/admin/token/acls"),
    ];
    let callers = [
        ("admin", Some("admin"), true, 501),
        // A token that holds the right cannot manage tokens.
        ("admin token", Some("tok-admin"), false, 403),
        ("user", Some("user"), true, 403),
        ("store window", Some("store"), true, 403),
        ("pending admin", Some("admin-pending"), true, 403),
        ("token without the right", Some("tok-settings"), false, 403),
        ("anonymous", None, true, 401),
    ];
    assert_matrix(&router, &routes, &callers);
}

#[test]
fn routes_with_secrets_or_the_account_of_the_caller_are_never_cached() {
    let flagged: BTreeSet<(&str, String)> = built()
        .inventory()
        .into_iter()
        .filter(|info| info.contract.is_no_store())
        .map(|info| (info.method, info.path))
        .collect();
    let expected: BTreeSet<(&str, String)> = [
        ("GET", "/api/v1/user"),
        ("GET", "/api/v1/token"),
        // Age-filtered catalog answers and account-owned library/screenshot answers.
        ("GET", "/api/v1/store"),
        ("GET", "/api/v1/store/featured"),
        ("GET", "/api/v1/games/{id}"),
        ("GET", "/api/v1/collection"),
        ("POST", "/api/v1/collection"),
        ("GET", "/api/v1/collection/default"),
        ("POST", "/api/v1/collection/default/entry"),
        ("DELETE", "/api/v1/collection/default/entry"),
        ("GET", "/api/v1/collection/{id}"),
        ("DELETE", "/api/v1/collection/{id}"),
        ("POST", "/api/v1/collection/{id}/entry"),
        ("DELETE", "/api/v1/collection/{id}/entry"),
        ("POST", "/api/v1/object/{id}"),
        ("DELETE", "/api/v1/object/{id}"),
        ("GET", "/api/v1/screenshots"),
        ("GET", "/api/v1/screenshots/{id}"),
        ("DELETE", "/api/v1/screenshots/{id}"),
        ("GET", "/api/v1/screenshots/game/{id}"),
        ("POST", "/api/v1/screenshots/game/{id}"),
        ("POST", "/api/v1/user/mfa/totp/start"),
        ("POST", "/api/v1/user/token"),
        ("POST", "/api/v1/admin/token"),
        ("GET", "/api/v1/admin/auth/invitation"),
        ("POST", "/api/v1/admin/auth/invitation"),
        // The invitation id of the registration page is a secret.
        ("GET", "/api/v1/auth/signup/simple"),
        // The routes that set the session cookie are forced by it.
        ("GET", "/api/v1/auth/oidc/callback"),
        ("POST", "/api/v1/auth/passkey/start"),
        ("POST", "/api/v1/auth/passkey/finish"),
        ("POST", "/api/v1/auth/signin/simple"),
    ]
    .into_iter()
    .map(|(method, path)| (method, path.to_owned()))
    .collect();
    assert_eq!(flagged, expected);
    // Only the answers of the missing handler and of the gate are seen here, and the error
    // renderer stamps `no-store` on both; the success answers are tested in the router tests.
    let router = router();
    for (method, path) in &expected {
        let allowed = call(&router, method, path, Some(cookie("admin")));
        assert_eq!(
            allowed.status,
            StatusCode::NOT_IMPLEMENTED,
            "{method} {path}"
        );
        assert_eq!(cache_control(&allowed), Some("no-store"), "{method} {path}");
        if *method == "POST" {
            // Without the origin the gate rejects the request; the header is still there.
            let rejected = send(&router, method, path, &[cookie("admin")]);
            assert_eq!(rejected.status, StatusCode::FORBIDDEN, "{method} {path}");
            assert_eq!(
                cache_control(&rejected),
                Some("no-store"),
                "{method} {path}"
            );
        }
        let is_public = ROUTES
            .iter()
            .any(|(m, p, public)| m == method && *p == path && *public);
        if !is_public {
            let anonymous = call(&router, method, path, None);
            assert_eq!(
                anonymous.status,
                StatusCode::UNAUTHORIZED,
                "{method} {path}"
            );
            assert_eq!(
                cache_control(&anonymous),
                Some("no-store"),
                "{method} {path}"
            );
        }
    }
}

#[test]
fn literal_account_paths_win_over_the_id_of_a_profile() {
    let router = router();
    // The store window may read a profile but not the overview that shares the position.
    for (path, status) in [
        ("/api/v1/user/mfa", 403),
        ("/api/v1/user/auth", 403),
        ("/api/v1/user/client", 403),
        ("/api/v1/user/superlevel", 403),
        ("/api/v1/user/token", 403),
        ("/api/v1/user/someone", 501),
    ] {
        let reply = call(&router, "GET", path, Some(cookie("store")));
        assert_eq!(reply.status.as_u16(), status, "{path}");
    }
}

#[test]
fn the_superlevel_check_answers_get_only() {
    let router = router();
    for method in ["POST", "PUT", "PATCH", "DELETE"] {
        let reply = call(
            &router,
            method,
            "/api/v1/user/superlevel",
            Some(cookie("user")),
        );
        assert_eq!(reply.status, StatusCode::METHOD_NOT_ALLOWED, "{method}");
        assert_eq!(reply.code(), "method_not_allowed");
    }
}
