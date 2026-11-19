// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! The gate end to end: credentials, principals, CSRF and client addresses,
//! with synthetic credential providers.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use axum::Extension;
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{HeaderMap, Request, StatusCode};
use axum::routing::post;
use server_domain::credential::{BoxFuture, Credential, CredentialError, CredentialPort};
use server_domain::permissions::{RightSet, role_rights};
use server_domain::policy::{MfaLevel, Policy, Requirement, Right};
use server_domain::principal::{
    Device, Principal, Role, Session, SessionClass, SessionStage, Token, TokenKind,
};
use tower::ServiceExt;

use super::tests::{TEST_ORIGIN, test_context, test_peer};
use super::{ApplicationRouter, build, enforce};
use crate::auth::{
    ClientIp, CurrentPrincipal, ExternalOrigin, PeerAddress, Scheme, SecurityContext,
};
use crate::proxy::{Network, TrustedProxies};
use crate::registry::{
    BodyKind, Contract, Csrf, Limits, Method, Registry, RegistryError, ResponseKind, Route,
};

const SESSION: &str = "__Host-impluvium_session";

/// A provider that answers from a fixed table.
struct Table(HashMap<&'static str, Result<Principal, CredentialError>>);

impl CredentialPort for Table {
    fn resolve<'a>(
        &'a self,
        credential: &'a Credential,
    ) -> BoxFuture<'a, Result<Principal, CredentialError>> {
        let answer = self
            .0
            .get(credential.secret())
            .cloned()
            .unwrap_or(Err(CredentialError::Unknown));
        Box::pin(async move { answer })
    }
}

fn right(name: &'static str) -> Right {
    Right::new(name).unwrap()
}

fn session(role: Role, stage: SessionStage, class: SessionClass) -> Principal {
    Principal::Session(Session::new(role, stage, class, 5))
}

fn table() -> Arc<Table> {
    let browser = SessionClass::Browser;
    let done = SessionStage::Authenticated;
    let read = RightSet::from_rights(&[right("emoji:read")]);
    let mut entries: HashMap<_, Result<Principal, CredentialError>> = HashMap::new();
    entries.insert("user", Ok(session(Role::User, done, browser)));
    entries.insert("admin", Ok(session(Role::Admin, done, browser)));
    entries.insert(
        "pending",
        Ok(session(Role::Admin, SessionStage::MfaPending, browser)),
    );
    entries.insert("store", Ok(session(Role::Admin, done, SessionClass::Store)));
    entries.insert(
        "tok-read",
        Ok(Principal::Token(Token::new(
            TokenKind::Personal,
            &read,
            &role_rights(Role::Admin),
        ))),
    );
    entries.insert(
        "tok-empty",
        Ok(Principal::Token(Token::new(
            TokenKind::Service,
            &RightSet::empty(),
            &role_rights(Role::Admin),
        ))),
    );
    entries.insert(
        "tok-admin",
        Ok(Principal::Token(Token::new(
            TokenKind::Service,
            &RightSet::from_rights(&[right("admin:manage")]),
            &role_rights(Role::User),
        ))),
    );
    entries.insert(
        "dev",
        Ok(Principal::Device(Device::new(
            &read,
            &role_rights(Role::User),
        ))),
    );
    entries.insert("expired", Err(CredentialError::Expired));
    entries.insert("revoked", Err(CredentialError::Revoked));
    Arc::new(Table(entries))
}

fn describe(principal: &Principal) -> &'static str {
    match principal {
        Principal::Anonymous => "anonymous",
        Principal::Session(_) => "session",
        Principal::Token(_) => "token",
        Principal::Device(_) => "device",
    }
}

async fn whoami(principal: CurrentPrincipal) -> &'static str {
    describe(principal.principal())
}

async fn client_ip(Extension(ClientIp(address)): Extension<ClientIp>) -> String {
    address.to_string()
}

fn limits() -> Limits {
    Limits::new(1024, Duration::from_secs(5))
}

fn read_contract() -> Contract {
    Contract::new(ResponseKind::Json, &[200], limits())
}

fn write_contract(csrf: Csrf) -> Contract {
    Contract::new(ResponseKind::Json, &[200], limits())
        .request_body(BodyKind::Json)
        .csrf(csrf)
}

/// (case name, credential header, status on a read route, status on an admin route)
type CredentialCase = (&'static str, Option<(&'static str, String)>, u16, u16);

fn requires(names: &[&'static str]) -> Policy {
    let rights: Vec<Right> = names.iter().map(|name| right(name)).collect();
    Policy::protected(
        Requirement::authenticated()
            .require_rights(&rights)
            .unwrap(),
    )
}

fn routes() -> Registry {
    let mut registry = Registry::new();
    let mut add = |route: Route| registry.register(route).unwrap();
    let get =
        |path, policy| Route::new(Method::Get, path, policy, read_contract(), whoami).unwrap();
    let write = |path, policy, csrf| {
        Route::new(Method::Post, path, policy, write_contract(csrf), whoami).unwrap()
    };
    add(get(
        "/api/v1/me",
        Policy::protected(Requirement::authenticated()),
    ));
    add(get("/api/v1/read", requires(&["emoji:read"])));
    add(get("/api/v1/admin", requires(&["admin:manage"])));
    add(write(
        "/api/v1/things",
        requires(&["emoji:read"]),
        Csrf::Required,
    ));
    add(write(
        "/api/v1/login",
        Policy::public("sign-in").unwrap(),
        Csrf::Required,
    ));
    add(write(
        "/api/v1/hook",
        Policy::public("signed callback").unwrap(),
        Csrf::Exempt("the callback is authenticated by its signature"),
    ));
    add(write(
        "/api/v1/mfa",
        Policy::protected(Requirement::authenticated().require_mfa(MfaLevel::Pending)),
        Csrf::Required,
    ));
    add(Route::new(
        Method::Get,
        "/api/v1/ip",
        Policy::public("address probe").unwrap(),
        read_contract(),
        client_ip,
    )
    .unwrap());
    registry
}

fn context(origin: ExternalOrigin, proxies: &[(&str, u8)]) -> SecurityContext {
    let networks = proxies
        .iter()
        .map(|(address, prefix)| Network::new(address.parse().unwrap(), *prefix).unwrap())
        .collect();
    SecurityContext::new(origin, TrustedProxies::new(networks), table())
}

fn https_context(proxies: &[(&str, u8)]) -> SecurityContext {
    context(
        ExternalOrigin::new(Scheme::Https, "app.example.test", None).unwrap(),
        proxies,
    )
}

fn router(context: SecurityContext) -> Router {
    let ApplicationRouter { router } = build(routes(), context);
    router
}

struct Reply {
    status: StatusCode,
    headers: HeaderMap,
    body: String,
}

fn call(
    router: &Router,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    peer: Option<SocketAddr>,
) -> Reply {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    runtime.block_on(async {
        let mut builder = Request::builder().method(method).uri(path);
        for (name, value) in headers {
            builder = builder.header(*name, *value);
        }
        if let Some(peer) = peer {
            builder = builder.extension(PeerAddress(peer));
        }
        let request = builder.body(Body::empty()).unwrap();
        let response = router.clone().oneshot(request).await.unwrap();
        let (parts, body) = response.into_parts();
        let bytes = to_bytes(body, usize::MAX).await.unwrap();
        Reply {
            status: parts.status,
            headers: parts.headers,
            body: String::from_utf8(bytes.to_vec()).unwrap(),
        }
    })
}

fn get(router: &Router, path: &str, headers: &[(&str, &str)]) -> Reply {
    call(router, "GET", path, headers, Some(test_peer()))
}

fn post_with(router: &Router, path: &str, headers: &[(&str, &str)]) -> Reply {
    call(router, "POST", path, headers, Some(test_peer()))
}

fn cookie(value: &str) -> String {
    format!("{SESSION}={value}")
}

fn error_code(reply: &Reply) -> String {
    reply.body.clone()
}

fn code(name: &str) -> String {
    format!("{{\"code\":\"{name}\"}}")
}

#[test]
fn matrix_over_principals_on_rights_routes() {
    let router = router(https_context(&[]));
    // (credential header, status on a read route, status on an admin route)
    let cases: [CredentialCase; 13] = [
        ("anonymous", None, 401, 401),
        ("mfa pending", Some(("cookie", cookie("pending"))), 403, 403),
        ("user", Some(("cookie", cookie("user"))), 200, 403),
        ("admin", Some(("cookie", cookie("admin"))), 200, 200),
        ("store session", Some(("cookie", cookie("store"))), 200, 403),
        (
            "narrow token",
            Some(("authorization", "Bearer tok-read".into())),
            200,
            403,
        ),
        (
            "empty token",
            Some(("authorization", "Bearer tok-empty".into())),
            403,
            403,
        ),
        (
            "token of a plain user claiming admin",
            Some(("authorization", "Bearer tok-admin".into())),
            403,
            403,
        ),
        (
            "device",
            Some(("authorization", "Bearer dev".into())),
            200,
            403,
        ),
        (
            "expired cookie",
            Some(("cookie", cookie("expired"))),
            401,
            401,
        ),
        (
            "revoked token",
            Some(("authorization", "Bearer revoked".into())),
            401,
            401,
        ),
        (
            "unknown cookie",
            Some(("cookie", cookie("nobody"))),
            401,
            401,
        ),
        (
            "unknown token",
            Some(("authorization", "Bearer nobody".into())),
            401,
            401,
        ),
    ];
    for (name, credential, on_read, on_admin) in cases {
        let headers: Vec<(&str, &str)> = credential
            .iter()
            .map(|(header, value)| (*header, value.as_str()))
            .collect();
        assert_eq!(
            get(&router, "/api/v1/read", &headers).status.as_u16(),
            on_read,
            "{name}: read"
        );
        assert_eq!(
            get(&router, "/api/v1/admin", &headers).status.as_u16(),
            on_admin,
            "{name}: admin"
        );
    }
}

#[test]
fn the_handler_sees_the_principal_the_gate_decided_on() {
    let router = router(https_context(&[]));
    let session = cookie("user");
    assert_eq!(
        get(&router, "/api/v1/me", &[("cookie", session.as_str())]).body,
        "session"
    );
    assert_eq!(
        get(
            &router,
            "/api/v1/me",
            &[("authorization", "Bearer tok-read")]
        )
        .body,
        "token"
    );
    assert_eq!(
        get(&router, "/api/v1/me", &[("authorization", "Bearer dev")]).body,
        "device"
    );
    // A public route sees an unresolvable credential as anonymous.
    assert_eq!(
        post_with(
            &router,
            "/api/v1/login",
            &[
                ("origin", TEST_ORIGIN),
                ("cookie", cookie("expired").as_str())
            ]
        )
        .body,
        "anonymous"
    );
}

#[test]
fn a_credential_of_the_wrong_kind_resolves_to_nothing() {
    let router = router(https_context(&[]));
    // The provider would answer with a token for a cookie and with a session
    // for a bearer header; the gate does not accept either.
    assert_eq!(
        get(
            &router,
            "/api/v1/me",
            &[("cookie", cookie("tok-read").as_str())]
        )
        .status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        get(&router, "/api/v1/me", &[("authorization", "Bearer user")]).status,
        StatusCode::UNAUTHORIZED
    );
}

#[test]
fn same_origin_write_with_a_cookie_passes() {
    let router = router(https_context(&[]));
    let session = cookie("user");
    for extra in [
        &[][..],
        &[("sec-fetch-site", "same-origin")][..],
        &[("sec-fetch-site", "none")][..],
    ] {
        let mut headers = vec![("cookie", session.as_str()), ("origin", TEST_ORIGIN)];
        headers.extend_from_slice(extra);
        let reply = post_with(&router, "/api/v1/things", &headers);
        assert_eq!(
            (reply.status, reply.body.as_str()),
            (StatusCode::OK, "session")
        );
    }
}

#[test]
fn cookie_writes_without_a_matching_origin_fail_before_the_handler() {
    let router = router(https_context(&[]));
    let session = cookie("user");
    let attempts: [(&str, Vec<(&str, &str)>); 7] = [
        (
            "no headers but the cookie",
            vec![("cookie", session.as_str())],
        ),
        (
            "foreign origin",
            vec![
                ("cookie", session.as_str()),
                ("origin", "https://evil.test"),
            ],
        ),
        (
            "neighbour subdomain, same-site",
            vec![
                ("cookie", session.as_str()),
                ("origin", "https://other.example.test"),
                ("sec-fetch-site", "same-site"),
            ],
        ),
        (
            "opaque origin",
            vec![("cookie", session.as_str()), ("origin", "null")],
        ),
        (
            "fetch metadata alone",
            vec![
                ("cookie", session.as_str()),
                ("sec-fetch-site", "same-origin"),
            ],
        ),
        (
            "cross-site with the right origin",
            vec![
                ("cookie", session.as_str()),
                ("origin", TEST_ORIGIN),
                ("sec-fetch-site", "cross-site"),
            ],
        ),
        (
            "referer only",
            vec![
                ("cookie", session.as_str()),
                ("referer", "https://app.example.test/"),
            ],
        ),
    ];
    for (name, headers) in attempts {
        let reply = post_with(&router, "/api/v1/things", &headers);
        assert_eq!(reply.status, StatusCode::FORBIDDEN, "{name}");
        assert_eq!(error_code(&reply), code("csrf_rejected"), "{name}");
    }
}

#[test]
fn sign_in_is_protected_against_cross_site_requests_too() {
    let router = router(https_context(&[]));
    let blocked = post_with(&router, "/api/v1/login", &[]);
    assert_eq!(blocked.status, StatusCode::FORBIDDEN);
    assert_eq!(error_code(&blocked), code("csrf_rejected"));
    let allowed = post_with(&router, "/api/v1/login", &[("origin", TEST_ORIGIN)]);
    assert_eq!(allowed.status, StatusCode::OK);
}

#[test]
fn a_declared_exemption_skips_the_check() {
    let router = router(https_context(&[]));
    assert_eq!(
        post_with(&router, "/api/v1/hook", &[]).status,
        StatusCode::OK
    );
}

#[test]
fn only_a_resolved_bearer_principal_is_exempt_from_csrf() {
    let router = router(https_context(&[]));
    let ok = post_with(
        &router,
        "/api/v1/things",
        &[("authorization", "Bearer tok-read")],
    );
    assert_eq!((ok.status, ok.body.as_str()), (StatusCode::OK, "token"));
    // An unknown or revoked token does not buy the exemption.
    for bearer in ["Bearer nobody", "Bearer revoked", "Bearer user"] {
        let reply = post_with(&router, "/api/v1/things", &[("authorization", bearer)]);
        assert_eq!(reply.status, StatusCode::FORBIDDEN, "{bearer}");
        assert_eq!(error_code(&reply), code("csrf_rejected"), "{bearer}");
    }
}

#[test]
fn a_bearer_header_does_not_lift_a_session_boundary() {
    let router = router(https_context(&[]));
    // A pending second factor plus a valid token is a conflict, not a token.
    let reply = post_with(
        &router,
        "/api/v1/things",
        &[
            ("cookie", cookie("pending").as_str()),
            ("authorization", "Bearer tok-read"),
            ("origin", TEST_ORIGIN),
        ],
    );
    assert_eq!(reply.status, StatusCode::UNAUTHORIZED);
    assert_eq!(error_code(&reply), code("invalid_credentials"));
    // Without the token the pending session reaches nothing but its own step.
    let pending = cookie("pending");
    let denied = post_with(
        &router,
        "/api/v1/things",
        &[("cookie", pending.as_str()), ("origin", TEST_ORIGIN)],
    );
    assert_eq!(denied.status, StatusCode::FORBIDDEN);
    let step = post_with(
        &router,
        "/api/v1/mfa",
        &[("cookie", pending.as_str()), ("origin", TEST_ORIGIN)],
    );
    assert_eq!(step.status, StatusCode::OK);
    let finished = post_with(
        &router,
        "/api/v1/mfa",
        &[("cookie", cookie("user").as_str()), ("origin", TEST_ORIGIN)],
    );
    assert_eq!(finished.status, StatusCode::FORBIDDEN);
}

#[test]
fn ambiguous_and_malformed_credentials_are_rejected() {
    let router = router(https_context(&[]));
    let two_cookies = format!("{SESSION}=user; {SESSION}=admin");
    let line_one = cookie("user");
    let line_two = cookie("admin");
    let empty_cookie = format!("{SESSION}=");
    let attempts: [(&str, Vec<(&str, &str)>); 6] = [
        (
            "two cookies in one header",
            vec![("cookie", two_cookies.as_str())],
        ),
        (
            "two cookie headers",
            vec![("cookie", line_one.as_str()), ("cookie", line_two.as_str())],
        ),
        (
            "two authorization headers",
            vec![
                ("authorization", "Bearer tok-read"),
                ("authorization", "Bearer dev"),
            ],
        ),
        (
            "empty session cookie",
            vec![("cookie", empty_cookie.as_str())],
        ),
        ("basic scheme", vec![("authorization", "Basic abc")]),
        (
            "cookie and bearer",
            vec![
                ("cookie", line_one.as_str()),
                ("authorization", "Bearer tok-read"),
            ],
        ),
    ];
    for (name, headers) in attempts {
        let reply = get(&router, "/api/v1/me", &headers);
        assert_eq!(reply.status, StatusCode::UNAUTHORIZED, "{name}");
        assert_eq!(error_code(&reply), code("invalid_credentials"), "{name}");
        // Public routes reject them as well: no first match is picked.
        let reply = get(&router, "/api/v1/ip", &headers);
        assert_eq!(reply.status, StatusCode::UNAUTHORIZED, "{name} (public)");
    }
}

#[test]
fn a_plain_http_origin_reads_the_plain_cookie_only() {
    let origin = ExternalOrigin::new(Scheme::Http, "192.0.2.10", Some(8080)).unwrap();
    let router = router(context(origin, &[]));
    let ok = get(
        &router,
        "/api/v1/me",
        &[("cookie", "impluvium_session=user")],
    );
    assert_eq!(ok.status, StatusCode::OK);
    let prefixed = get(
        &router,
        "/api/v1/me",
        &[("cookie", cookie("user").as_str())],
    );
    assert_eq!(prefixed.status, StatusCode::UNAUTHORIZED);
    // The origin of this deployment is the plain http one.
    let write = post_with(
        &router,
        "/api/v1/things",
        &[
            ("cookie", "impluvium_session=user"),
            ("origin", "http://192.0.2.10:8080"),
        ],
    );
    assert_eq!(write.status, StatusCode::OK);
    let wrong = post_with(
        &router,
        "/api/v1/things",
        &[
            ("cookie", "impluvium_session=user"),
            ("origin", TEST_ORIGIN),
        ],
    );
    assert_eq!(wrong.status, StatusCode::FORBIDDEN);
}

#[test]
fn forwarded_headers_of_a_stranger_do_not_change_the_address() {
    let router = router(https_context(&[("192.0.2.0", 24)]));
    let stranger: SocketAddr = "198.51.100.9:5000".parse().unwrap();
    for header in [
        ("x-forwarded-for", "203.0.113.5"),
        ("forwarded", "for=203.0.113.5"),
        ("x-forwarded-for", "garbage"),
    ] {
        let reply = call(&router, "GET", "/api/v1/ip", &[header], Some(stranger));
        assert_eq!(
            (reply.status, reply.body.as_str()),
            (StatusCode::OK, "198.51.100.9")
        );
    }
}

#[test]
fn a_trusted_proxy_names_the_client() {
    let router = router(https_context(&[("192.0.2.0", 24), ("2001:db8:ffff::", 48)]));
    let proxy: SocketAddr = "192.0.2.7:5000".parse().unwrap();
    let ask =
        |headers: &[(&str, &str)], peer| call(&router, "GET", "/api/v1/ip", headers, Some(peer));
    let reply = ask(
        &[("x-forwarded-for", "198.51.100.1, 203.0.113.5, 192.0.2.99")],
        proxy,
    );
    assert_eq!(reply.body, "203.0.113.5");
    let reply = ask(&[("forwarded", "for=\"[2001:db8:1::5]:80\"")], proxy);
    assert_eq!(reply.body, "2001:db8:1::5");
    // A dual-stack listener reports the proxy as an IPv4-mapped address.
    let mapped: SocketAddr = "[::ffff:192.0.2.7]:5000".parse().unwrap();
    let reply = ask(&[("x-forwarded-for", "203.0.113.5")], mapped);
    assert_eq!(
        (reply.status, reply.body.as_str()),
        (StatusCode::OK, "203.0.113.5")
    );
    // No header: the proxy itself.
    assert_eq!(ask(&[], proxy).body, "192.0.2.7");
}

#[test]
fn broken_or_contradictory_headers_of_a_trusted_proxy_are_rejected() {
    let router = router(https_context(&[("192.0.2.0", 24)]));
    let proxy: SocketAddr = "192.0.2.7:5000".parse().unwrap();
    let attempts: [(&str, Vec<(&str, &str)>); 3] = [
        (
            "garbage chain",
            vec![("x-forwarded-for", "203.0.113.5, nonsense")],
        ),
        (
            "both header families",
            vec![
                ("x-forwarded-for", "203.0.113.5"),
                ("forwarded", "for=203.0.113.5"),
            ],
        ),
        ("obfuscated node", vec![("forwarded", "for=_hidden")]),
    ];
    for (name, headers) in attempts {
        let reply = call(&router, "GET", "/api/v1/ip", &headers, Some(proxy));
        assert_eq!(reply.status, StatusCode::BAD_REQUEST, "{name}");
        assert_eq!(
            error_code(&reply),
            code("invalid_forwarding_headers"),
            "{name}"
        );
    }
}

#[test]
fn a_request_without_a_peer_address_is_an_internal_error() {
    let router = router(https_context(&[]));
    let reply = call(&router, "GET", "/api/v1/ip", &[], None);
    assert_eq!(reply.status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(error_code(&reply), code("internal_error"));
}

#[test]
fn no_cors_header_appears_on_any_answer() {
    let router = router(https_context(&[]));
    let replies = [
        get(&router, "/api/v1/ip", &[("origin", "https://evil.test")]),
        get(&router, "/api/v1/me", &[("origin", "https://evil.test")]),
        post_with(&router, "/api/v1/login", &[("origin", "https://evil.test")]),
        post_with(&router, "/api/v1/login", &[("origin", TEST_ORIGIN)]),
        call(
            &router,
            "OPTIONS",
            "/api/v1/login",
            &[("origin", "https://evil.test")],
            Some(test_peer()),
        ),
        get(&router, "/api/v1/missing", &[]),
    ];
    for reply in replies {
        assert!(
            reply
                .headers
                .keys()
                .all(|name| !name.as_str().starts_with("access-control-")),
            "{:?}",
            reply.headers
        );
    }
}

#[test]
fn a_handler_without_the_gate_fails_before_any_effect() {
    static EFFECTS: AtomicUsize = AtomicUsize::new(0);
    async fn effect(_principal: CurrentPrincipal) -> &'static str {
        EFFECTS.fetch_add(1, Ordering::SeqCst);
        "done"
    }
    let ungated = enforce(Router::new().route("/ungated", post(effect)));
    let reply = call(&ungated, "POST", "/ungated", &[], Some(test_peer()));
    assert_eq!(reply.status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(error_code(&reply), code("internal_error"));
    assert_eq!(EFFECTS.load(Ordering::SeqCst), 0);
    // A gated route in a router without the security context fails closed too.
    let route = Route::new(
        Method::Post,
        "/gated",
        Policy::public("test").unwrap(),
        write_contract(Csrf::Required),
        effect,
    )
    .unwrap();
    let no_context = enforce(Router::new().route("/gated", route.endpoint));
    let reply = call(
        &no_context,
        "POST",
        "/gated",
        &[("origin", TEST_ORIGIN)],
        Some(test_peer()),
    );
    assert_eq!(reply.status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(EFFECTS.load(Ordering::SeqCst), 0);
}

#[test]
fn the_default_test_context_knows_no_credential() {
    // The shared router tests use a context without credentials.
    let ApplicationRouter { router } = build(routes(), test_context());
    let reply = get(
        &router,
        "/api/v1/me",
        &[("cookie", cookie("user").as_str())],
    );
    assert_eq!(reply.status, StatusCode::UNAUTHORIZED);
    assert_eq!(error_code(&reply), code("unauthenticated"));
}

/// A port that counts how often it is asked.
struct Counting {
    inner: Arc<Table>,
    calls: Arc<AtomicUsize>,
}

impl CredentialPort for Counting {
    fn resolve<'a>(
        &'a self,
        credential: &'a Credential,
    ) -> BoxFuture<'a, Result<Principal, CredentialError>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.inner.resolve(credential)
    }
}

#[test]
fn a_forged_request_does_not_reach_the_credential_port() {
    let calls = Arc::new(AtomicUsize::new(0));
    let port = Counting {
        inner: table(),
        calls: calls.clone(),
    };
    let origin = ExternalOrigin::new(Scheme::Https, "app.example.test", None).unwrap();
    let router = router(SecurityContext::new(
        origin,
        TrustedProxies::default(),
        Arc::new(port),
    ));
    let session = cookie("user");
    let forged = post_with(
        &router,
        "/api/v1/things",
        &[
            ("cookie", session.as_str()),
            ("origin", "https://evil.test"),
        ],
    );
    assert_eq!(forged.status, StatusCode::FORBIDDEN);
    assert_eq!(
        post_with(&router, "/api/v1/login", &[]).status,
        StatusCode::FORBIDDEN
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    // A same-origin cookie request is resolved, and so is a bearer token.
    let ok = post_with(
        &router,
        "/api/v1/things",
        &[("cookie", session.as_str()), ("origin", TEST_ORIGIN)],
    );
    assert_eq!(ok.status, StatusCode::OK);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let bearer = post_with(
        &router,
        "/api/v1/things",
        &[("authorization", "Bearer tok-read")],
    );
    assert_eq!(bearer.status, StatusCode::OK);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[test]
fn a_csrf_exemption_is_only_allowed_on_public_routes() {
    let exempt = || write_contract(Csrf::Exempt("signed callback"));
    let protected = Route::new(
        Method::Post,
        "/api/v1/hook",
        Policy::protected(Requirement::authenticated()),
        exempt(),
        whoami,
    );
    assert_eq!(
        protected.map(|_| ()),
        Err(RegistryError::CsrfMismatch("/api/v1/hook".into()))
    );
    let public = Route::new(
        Method::Post,
        "/api/v1/hook",
        Policy::public("signed callback").unwrap(),
        exempt(),
        whoami,
    );
    assert!(public.is_ok());
}
