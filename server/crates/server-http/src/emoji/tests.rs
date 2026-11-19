// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

use std::collections::HashMap;
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

use super::{EmojiFiles, EmojiFilesError, MAX_PARTS, is_codepoint, routes};
use crate::auth::{ExternalOrigin, PeerAddress, Scheme, SecurityContext};
use crate::proxy::TrustedProxies;
use crate::registry::Registry;
use crate::router::{AppOptions, build_with};
use crate::spa::{SpaEntry, SpaFiles};

const SVG: &[u8] = b"<svg xmlns=\"http://www.w3.org/2000/svg\"/>";
const SESSION: &str = "__Host-impluvium_session";

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

fn read_right() -> RightSet {
    RightSet::from_rights(&[Right::new("emoji:read").unwrap()])
}

fn session(role: Role, stage: SessionStage) -> Principal {
    Principal::Session(Session::new(role, stage, SessionClass::Browser, 5))
}

fn table() -> Table {
    let done = SessionStage::Authenticated;
    let mut entries = HashMap::new();
    entries.insert("user", session(Role::User, done));
    entries.insert("admin", session(Role::Admin, done));
    entries.insert("pending", session(Role::User, SessionStage::MfaPending));
    entries.insert(
        "tok-ok",
        Principal::Token(Token::new(
            TokenKind::Personal,
            &read_right(),
            &role_rights(Role::User),
        )),
    );
    entries.insert(
        "tok-owner-lost",
        Principal::Token(Token::new(
            TokenKind::Personal,
            &read_right(),
            &RightSet::empty(),
        )),
    );
    entries.insert(
        "tok-no-right",
        Principal::Token(Token::new(
            TokenKind::Service,
            &RightSet::empty(),
            &role_rights(Role::Admin),
        )),
    );
    Table(entries)
}

fn files() -> Arc<EmojiFiles> {
    Arc::new(
        EmojiFiles::new(vec![
            ("1f600".to_owned(), SVG.to_vec()),
            ("1f1e9-1f1ea".to_owned(), b"<svg id=\"de\"/>".to_vec()),
        ])
        .unwrap(),
    )
}

fn router_with(files: Arc<EmojiFiles>) -> Router {
    let mut registry = Registry::new();
    for route in routes(files) {
        registry.register(route).unwrap();
    }
    let context = SecurityContext::new(
        ExternalOrigin::new(Scheme::Https, "app.example.test", None).unwrap(),
        TrustedProxies::default(),
        Arc::new(table()),
    );
    let shell = SpaFiles::new(vec![SpaEntry {
        name: "index.html".to_owned(),
        bytes: b"<!doctype html><title>app</title>".to_vec(),
        sha256: "AAAAindex=".to_owned(),
    }])
    .unwrap();
    let options = AppOptions {
        spa: Some(Arc::new(shell)),
        ..AppOptions::default()
    };
    build_with(registry, context, &options).into_router()
}

struct Reply {
    status: StatusCode,
    headers: HeaderMap,
    body: Vec<u8>,
}

fn send(router: &Router, method: &str, target: &str, headers: &[(&str, String)]) -> Reply {
    let mut builder = Request::builder()
        .method(method)
        .uri(target)
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

fn cookie(value: &str) -> (&'static str, String) {
    ("cookie", format!("{SESSION}={value}"))
}

fn bearer(value: &str) -> (&'static str, String) {
    ("authorization", format!("Bearer {value}"))
}

fn code(reply: &Reply) -> String {
    let value: serde_json::Value = serde_json::from_slice(&reply.body).unwrap();
    value["code"].as_str().unwrap().to_owned()
}

fn as_user(router: &Router, target: &str) -> Reply {
    send(router, "GET", target, &[cookie("user")])
}

/// One row of the principal matrix: label, credentials, expected status, expected error code.
type Case<'a> = (&'a str, Option<(&'a str, String)>, u16, &'a str);

#[test]
fn the_principal_matrix_follows_the_right() {
    let router = router_with(files());
    let target = "/api/v1/emoji/1f600";
    let cases: [Case<'_>; 7] = [
        ("user", Some(cookie("user")), 200, ""),
        ("admin", Some(cookie("admin")), 200, ""),
        (
            "token with the right and an owner holding it",
            Some(bearer("tok-ok")),
            200,
            "",
        ),
        (
            "token with the right whose owner lost it",
            Some(bearer("tok-owner-lost")),
            403,
            "forbidden",
        ),
        (
            "token without the right",
            Some(bearer("tok-no-right")),
            403,
            "forbidden",
        ),
        ("mfa pending", Some(cookie("pending")), 403, "forbidden"),
        ("anonymous", None, 401, "unauthenticated"),
    ];
    for (name, credential, status, error) in cases {
        let headers: Vec<(&str, String)> = credential.into_iter().collect();
        let reply = send(&router, "GET", target, &headers);
        assert_eq!(reply.status.as_u16(), status, "{name}");
        if !error.is_empty() {
            assert_eq!(code(&reply), error, "{name}");
        }
    }
}

#[test]
fn a_refused_caller_never_learns_whether_the_picture_exists() {
    let router = router_with(files());
    for target in [
        "/api/v1/emoji/1f600",
        "/api/v1/emoji/1f9999",
        "/api/v1/emoji/NOPE",
    ] {
        let reply = send(&router, "GET", target, &[]);
        assert_eq!(reply.status, StatusCode::UNAUTHORIZED, "{target}");
    }
}

#[test]
fn a_picture_is_served_as_a_private_cached_svg() {
    let router = router_with(files());
    let reply = as_user(&router, "/api/v1/emoji/1f1e9-1f1ea");
    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(reply.body, b"<svg id=\"de\"/>");
    let header = |name: &str| {
        reply
            .headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
    };
    assert_eq!(header("content-type"), "image/svg+xml");
    assert_eq!(header("cache-control"), "private, max-age=31536000");
    assert_eq!(header("x-content-type-options"), "nosniff");
    let policy = header("content-security-policy");
    assert!(policy.starts_with("default-src 'none'"), "{policy}");
    assert!(!policy.contains("script-src"), "{policy}");
    assert_eq!(header("referrer-policy"), "no-referrer");
}

#[test]
fn only_strict_code_points_are_accepted() {
    let router = router_with(files());
    let too_many = vec!["1f600"; MAX_PARTS + 1].join("-");
    let too_long_number = "1f60000";
    let bad = [
        "1F600",
        "1f600.svg",
        "..",
        ".",
        "%2e%2e",
        "%2E%2E%2Fetc",
        "..%2F..%2Fetc%2Fpasswd",
        "1f600%2F1f601",
        "1f600%00",
        "1f600%0a",
        "1f600-",
        "-1f600",
        "1f600--1f601",
        "1g00",
        "0x1f600",
        "1f600%20",
        too_long_number,
        too_many.as_str(),
    ];
    for name in bad {
        let reply = as_user(&router, &format!("/api/v1/emoji/{name}"));
        assert_eq!(reply.status, StatusCode::BAD_REQUEST, "{name}");
        assert_eq!(code(&reply), "invalid_path_param", "{name}");
    }
}

#[test]
fn an_unknown_well_formed_name_is_not_found_and_an_empty_one_is_a_miss() {
    let router = router_with(files());
    let reply = as_user(&router, "/api/v1/emoji/1f9999");
    assert_eq!(reply.status, StatusCode::NOT_FOUND);
    assert_eq!(code(&reply), "not_found");
    let longest = ["1f600"; MAX_PARTS].join("-");
    assert_eq!(
        as_user(&router, &format!("/api/v1/emoji/{longest}")).status,
        StatusCode::NOT_FOUND
    );
    // No segment: either no route matches or the grammar refuses the empty name. Both are API
    // errors, never a page or a file.
    for target in ["/api/v1/emoji/", "/api/v1/emoji"] {
        let reply = as_user(&router, target);
        assert!(
            matches!(reply.status.as_u16(), 400 | 404),
            "{target}: {}",
            reply.status
        );
        assert!(matches!(
            code(&reply).as_str(),
            "not_found" | "invalid_path_param"
        ));
    }
}

#[test]
fn an_empty_set_answers_not_found_for_every_name() {
    let router = router_with(Arc::new(EmojiFiles::empty()));
    assert_eq!(
        as_user(&router, "/api/v1/emoji/1f600").status,
        StatusCode::NOT_FOUND
    );
}

#[test]
fn other_methods_get_the_method_error() {
    let router = router_with(files());
    for method in ["POST", "PUT", "PATCH", "DELETE"] {
        let reply = send(&router, method, "/api/v1/emoji/1f600", &[cookie("user")]);
        assert_eq!(reply.status, StatusCode::METHOD_NOT_ALLOWED, "{method}");
        assert_eq!(code(&reply), "method_not_allowed");
    }
}

#[test]
fn the_grammar_is_decided_by_one_function() {
    for good in [
        "a9",
        "1f600",
        "23-20e3",
        "1f1e9-1f1ea",
        "1f3f4-200d-2620-fe0f",
        "10ffff",
        "000000",
    ] {
        assert!(is_codepoint(good), "{good}");
    }
    for bad in [
        "", "-", "A", "1F600", "1f6000x", "1 f", "1_f", "é", "1f600\n",
    ] {
        assert!(!is_codepoint(bad), "{bad:?}");
    }
    assert!(is_codepoint(&["1f600"; MAX_PARTS].join("-")));
    assert!(!is_codepoint(&vec!["1f600"; MAX_PARTS + 1].join("-")));
}

#[test]
fn a_set_refuses_bad_and_repeated_names() {
    assert_eq!(
        EmojiFiles::new(vec![("1F600".to_owned(), vec![])]),
        Err(EmojiFilesError::BadName("1F600".to_owned()))
    );
    assert_eq!(
        EmojiFiles::new(vec![("../x".to_owned(), vec![])]),
        Err(EmojiFilesError::BadName("../x".to_owned()))
    );
    assert_eq!(
        EmojiFiles::new(vec![
            ("1f600".to_owned(), vec![]),
            ("1f600".to_owned(), vec![])
        ]),
        Err(EmojiFilesError::DuplicateName("1f600".to_owned()))
    );
    assert!(EmojiFiles::empty().is_empty());
    assert_eq!(files().len(), 2);
}

#[test]
fn the_route_is_listed_with_its_right() {
    let mut registry = Registry::new();
    for route in routes(files()) {
        registry.register(route).unwrap();
    }
    let inventory = registry.inventory();
    assert_eq!(inventory.len(), 1);
    assert_eq!(inventory[0].method, "GET");
    assert_eq!(inventory[0].path, "/api/v1/emoji/{codepoint}");
    let requirement = inventory[0]
        .policy
        .requirement()
        .expect("a protected route");
    let rights: Vec<&str> = requirement.rights().iter().map(|r| r.as_str()).collect();
    assert_eq!(rights, ["emoji:read"]);
}
