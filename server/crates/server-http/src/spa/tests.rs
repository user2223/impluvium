// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Router-level tests of the web application files. Requests are sent with raw targets, so the
//! client does not normalise them.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{HeaderMap, Request, StatusCode, Uri, header};
use server_domain::credential::NoCredentials;
use server_domain::policy::Policy;
use tower::ServiceExt;

use super::{SpaEntry, SpaFiles, SpaFilesError, accepted_encodings, quality};
use crate::auth::{ExternalOrigin, PeerAddress, Scheme, SecurityContext};
use crate::headers::ScriptHash;
use crate::proxy::TrustedProxies;
use crate::registry::{Contract, Limits, Method as RouteMethod, Registry, ResponseKind, Route};
use crate::router::{AppOptions, ApplicationRouter, build_with};

const INDEX: &str = "<!doctype html><title>app</title>";
const HASH: &str = "sha256-47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU=";
const SCRIPT: &str = "_nuxt/entry.D3f9a8b1.js";
const SCRIPT_URL: &str = "/_nuxt/entry.D3f9a8b1.js";
const RESTRICTIVE: &str = "default-src 'none'; base-uri 'none'";

fn entry(name: &str, bytes: &str, digest: &str) -> SpaEntry {
    SpaEntry {
        name: name.to_owned(),
        bytes: bytes.as_bytes().to_vec(),
        sha256: digest.to_owned(),
    }
}

fn files() -> SpaFiles {
    SpaFiles::new(vec![
        entry("index.html", INDEX, "AAAAindex="),
        entry("index.html.br", "stale shell", "AAAAhbr="),
        entry("loading.css", "body{}", "AAAAcss="),
        entry(SCRIPT, "plain", "AAAAplain="),
        entry(&format!("{SCRIPT}.br"), "brotli", "AAAAbr="),
        entry(&format!("{SCRIPT}.gz"), "gzipped", "AAAAgz="),
        entry("_nuxt/app.js", "app", "AAAAapp="),
    ])
    .unwrap()
}

async fn server_route() -> &'static str {
    "route"
}

fn router() -> Router {
    let mut registry = Registry::new();
    for path in ["/auth/oidc", "/auth/signout", "/auth/callback/oidc"] {
        let contract = Contract::new(
            ResponseKind::Json,
            &[200],
            Limits::new(8, Duration::from_secs(5)),
        );
        let policy = Policy::public("test route").unwrap();
        let route = Route::new(RouteMethod::Get, path, policy, contract, server_route).unwrap();
        registry.register(route).unwrap();
    }
    let context = SecurityContext::new(
        ExternalOrigin::new(Scheme::Https, "app.example.test", None).unwrap(),
        TrustedProxies::default(),
        Arc::new(NoCredentials),
    );
    let options = AppOptions {
        script_hashes: vec![ScriptHash::parse(HASH).unwrap()],
        spa: Some(Arc::new(files())),
        ..AppOptions::default()
    };
    let ApplicationRouter { router } = build_with(registry, context, &options);
    router
}

struct Reply {
    status: StatusCode,
    headers: HeaderMap,
    body: Vec<u8>,
}

impl Reply {
    fn header(&self, name: header::HeaderName) -> &str {
        self.headers
            .get(name)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("")
    }

    fn code(&self) -> String {
        let value: serde_json::Value = serde_json::from_slice(&self.body).unwrap();
        value["code"].as_str().unwrap().to_owned()
    }

    fn is_error(&self, status: u16, code: &str) -> bool {
        self.status.as_u16() == status
            && self.header(header::CONTENT_TYPE) == "application/json"
            && self.code() == code
    }
}

fn send(router: &Router, method: &str, target: &str, headers: &[(&str, &str)]) -> Reply {
    let uri = Uri::try_from(target).unwrap();
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .extension(PeerAddress(SocketAddr::from(([192, 0, 2, 1], 4000))));
    for (name, value) in headers {
        builder = builder.header(*name, *value);
    }
    let request = builder.body(Body::empty()).unwrap();
    let router = router.clone();
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(async move {
            let response = router.oneshot(request).await.unwrap();
            let (parts, body) = response.into_parts();
            Reply {
                status: parts.status,
                headers: parts.headers,
                body: to_bytes(body, usize::MAX).await.unwrap().to_vec(),
            }
        })
}

fn get(router: &Router, target: &str) -> Reply {
    send(router, "GET", target, &[])
}

#[test]
fn raw_request_targets_are_decided_on_the_canonical_path() {
    let router = router();
    let cases: [(&str, u16); 25] = [
        ("/missing.JS?v=1", 404),
        ("/_nuxt/missing%252Ejs", 400),
        ("/missing%252Ejs", 400),
        ("/%255Fnuxt/missing", 400),
        ("/missing.js;v=1", 400),
        ("/_nuxt/%2E%2E/csp-hashes.json", 400),
        ("/a%2Fb", 400),
        ("/a%5Cb", 400),
        ("/a%00b", 400),
        ("/a%3Bb", 400),
        ("//_nuxt/missing", 404),
        ("/_nuxt/./missing", 404),
        ("/./_nuxt/missing", 404),
        ("/%2e/_nuxt/missing", 404),
        ("/_nuxt/%2E", 404),
        ("/_nuxt/missing", 404),
        ("/_fonts/", 404),
        ("/_fonts", 404),
        ("/missing.png", 404),
        ("/csp-hashes.json", 404),
        ("/INDEX.HTML", 404),
        ("//store/x", 200),
        ("/%2E", 200),
        ("/_nuxt/./entry.D3f9a8b1.js", 200),
        ("//_nuxt//entry.D3f9a8b1.js?v=1", 200),
    ];
    for (target, status) in cases {
        let reply = get(&router, target);
        assert_eq!(reply.status.as_u16(), status, "{target}");
        match status {
            400 => assert!(reply.is_error(400, "bad_request"), "{target}"),
            404 => assert!(reply.is_error(404, "not_found"), "{target}"),
            _ => assert!(!reply.body.is_empty(), "{target}"),
        }
    }
    // The decoded path names the file: the encoded form reaches the same file.
    let plain = get(&router, SCRIPT_URL);
    let encoded = get(&router, "/_nuxt/entry%2ED3f9a8b1.js");
    assert_eq!(encoded.body, plain.body);
}

#[test]
fn client_routes_and_unknown_paths_get_the_shell() {
    let router = router();
    for target in [
        "/",
        "/store",
        "/store/",
        "/store/12",
        "/library/game.v2",
        "/a.b/c",
        "/profile/1.5",
        "/unknown/deep/path",
        "/store?tab=2",
        "/API/x",
        "/apis",
    ] {
        let reply = get(&router, target);
        assert_eq!(reply.status, StatusCode::OK, "{target}");
        assert_eq!(reply.body, INDEX.as_bytes(), "{target}");
        assert_eq!(
            reply.header(header::CONTENT_TYPE),
            "text/html; charset=utf-8"
        );
        assert_eq!(reply.header(header::CACHE_CONTROL), "no-cache");
        assert_eq!(reply.header(header::VARY), "Accept-Encoding");
        assert_eq!(reply.header(header::ETAG), "\"AAAAindex=\"");
        assert_eq!(reply.header(header::X_FRAME_OPTIONS), "DENY");
        assert!(
            reply
                .header(header::CONTENT_SECURITY_POLICY)
                .starts_with(&format!(
                    "default-src 'none'; script-src 'self' '{HASH}'; style-src 'self'"
                )),
            "{target}"
        );
    }
}

#[test]
fn the_browser_pages_under_auth_get_the_shell_with_the_policy() {
    let router = router();
    for target in [
        "/auth/signin",
        "/auth/register",
        "/auth/mfa",
        "/auth/mfa/totp",
        "/auth/mfa/webauthn",
        "/auth",
        "/auth/oidc/x",
        "/auth/callback",
    ] {
        let reply = get(&router, target);
        assert_eq!(reply.status, StatusCode::OK, "{target}");
        assert_eq!(reply.body, INDEX.as_bytes(), "{target}");
        let csp = reply.header(header::CONTENT_SECURITY_POLICY);
        assert!(csp.contains(&format!("'{HASH}'")), "{target}: {csp}");
    }
}

#[test]
fn the_concrete_auth_server_routes_reach_their_route_and_never_the_shell() {
    let router = router();
    for target in [
        "/auth/oidc",
        "/auth/signout",
        "/auth/callback/oidc",
        "/auth/oidc?next=%2Fstore",
    ] {
        let reply = get(&router, target);
        assert_eq!(reply.status, StatusCode::OK, "{target}");
        assert_eq!(reply.body, b"route", "{target}");
        let wrong = send(&router, "POST", target, &[]);
        assert!(wrong.is_error(405, "method_not_allowed"), "{target}");
        assert_eq!(wrong.header(header::ALLOW), "GET, HEAD", "{target}");
    }
    // Spellings that no route matches are still server paths and get the API error.
    for target in [
        "/auth//oidc",
        "/auth/oidc/",
        "/auth/./signout",
        "/auth/callback/oidc/",
    ] {
        let reply = get(&router, target);
        assert!(reply.is_error(404, "not_found"), "{target}");
        let post = send(&router, "POST", target, &[]);
        assert!(post.is_error(404, "not_found"), "{target}");
    }
    // Whether the framework decodes an escape before it matches a route or not, the answer is
    // never the shell.
    let escaped = get(&router, "/%61uth/oidc");
    assert_ne!(escaped.body, INDEX.as_bytes());
}

#[test]
fn api_and_depot_paths_get_the_api_error_and_never_a_page() {
    let router = router();
    for target in [
        "/api",
        "/api/",
        "/api/x",
        "/api/v1/missing",
        "//api/x",
        "/./api/x",
        "/%61pi/x",
        "/depot",
        "/depot/x",
        "/depot/x/y.js",
    ] {
        for method in ["GET", "POST", "HEAD"] {
            let reply = send(&router, method, target, &[]);
            assert_eq!(reply.status, StatusCode::NOT_FOUND, "{method} {target}");
            assert_eq!(reply.header(header::CONTENT_TYPE), "application/json");
            if method != "HEAD" {
                assert_eq!(reply.code(), "not_found", "{target}");
            }
        }
    }
}

#[test]
fn hashed_assets_are_immutable_and_everything_else_is_revalidated() {
    let router = router();
    for target in [SCRIPT_URL, "/_nuxt/entry.D3f9a8b1.js?v=1"] {
        let reply = get(&router, target);
        assert_eq!(reply.status, StatusCode::OK);
        assert_eq!(
            reply.header(header::CACHE_CONTROL),
            "public, max-age=31536000, immutable"
        );
        assert_eq!(
            reply.header(header::CONTENT_TYPE),
            "text/javascript; charset=utf-8"
        );
    }
    for (target, media_type) in [
        ("/_nuxt/app.js", "text/javascript; charset=utf-8"),
        ("/loading.css", "text/css; charset=utf-8"),
        ("/index.html", "text/html; charset=utf-8"),
    ] {
        let reply = get(&router, target);
        assert_eq!(reply.status, StatusCode::OK, "{target}");
        assert_eq!(reply.header(header::CACHE_CONTROL), "no-cache", "{target}");
        assert_eq!(reply.header(header::CONTENT_TYPE), media_type, "{target}");
    }
    // A file that is not a page carries the asset header set, not the shell policy.
    let asset = get(&router, "/loading.css");
    assert!(
        asset
            .header(header::CONTENT_SECURITY_POLICY)
            .starts_with(RESTRICTIVE)
    );
    assert_eq!(asset.header(header::X_CONTENT_TYPE_OPTIONS), "nosniff");
    assert!(!asset.headers.contains_key(header::X_FRAME_OPTIONS));
}

#[test]
fn precompressed_siblings_are_chosen_by_accept_encoding() {
    let router = router();
    let cases: [(Option<&str>, &str, Option<&str>); 13] = [
        (None, "plain", None),
        (Some("gzip"), "gzipped", Some("gzip")),
        (Some("br, gzip"), "brotli", Some("br")),
        (Some("gzip, br;q=0"), "gzipped", Some("gzip")),
        (Some("br;q=0.5, gzip"), "gzipped", Some("gzip")),
        (Some("br;q=0.5, gzip;q=0.5"), "brotli", Some("br")),
        (Some("*"), "brotli", Some("br")),
        (Some("*;q=0, gzip"), "gzipped", Some("gzip")),
        (Some("identity"), "plain", None),
        (Some("br;q=0, gzip;q=0"), "plain", None),
        (Some("BR"), "brotli", Some("br")),
        (Some("deflate"), "plain", None),
        (Some("gzip;q=x"), "plain", None),
    ];
    for (accept, body, encoding) in cases {
        let headers: Vec<(&str, &str)> = accept
            .map(|value| ("accept-encoding", value))
            .into_iter()
            .collect();
        let reply = send(&router, "GET", SCRIPT_URL, &headers);
        assert_eq!(reply.status, StatusCode::OK, "{accept:?}");
        assert_eq!(reply.body, body.as_bytes(), "{accept:?}");
        let sent = reply
            .headers
            .get(header::CONTENT_ENCODING)
            .and_then(|value| value.to_str().ok());
        assert_eq!(sent, encoding, "{accept:?}");
        assert_eq!(reply.header(header::VARY), "Accept-Encoding");
        assert_eq!(
            reply.header(header::CONTENT_TYPE),
            "text/javascript; charset=utf-8"
        );
        assert_eq!(reply.header(header::CONTENT_LENGTH), body.len().to_string());
    }
    // Without siblings the answer is the file itself, and it still varies.
    let alone = send(
        &router,
        "GET",
        "/_nuxt/app.js",
        &[("accept-encoding", "br")],
    );
    assert_eq!(alone.body, b"app");
    assert!(!alone.headers.contains_key(header::CONTENT_ENCODING));
    assert_eq!(alone.header(header::VARY), "Accept-Encoding");
    // The siblings are not files of their own.
    for target in [format!("{SCRIPT_URL}.br"), format!("{SCRIPT_URL}.gz")] {
        assert!(get(&router, &target).is_error(404, "not_found"), "{target}");
    }
}

#[test]
fn the_entity_tag_of_the_sent_bytes_revalidates_to_304() {
    let router = router();
    let shell = get(&router, "/store");
    let tag = shell.header(header::ETAG).to_owned();
    for value in [
        tag.clone(),
        format!("W/{tag}"),
        "*".to_owned(),
        format!("\"other\", {tag}"),
    ] {
        let reply = send(
            &router,
            "GET",
            "/store",
            &[("if-none-match", value.as_str())],
        );
        assert_eq!(reply.status, StatusCode::NOT_MODIFIED, "{value}");
        assert!(reply.body.is_empty());
        assert_eq!(reply.header(header::ETAG), tag);
        assert_eq!(reply.header(header::CACHE_CONTROL), "no-cache");
        assert_eq!(reply.header(header::VARY), "Accept-Encoding");
        // A cache that merges this answer must keep the policy of the page.
        assert!(
            reply
                .header(header::CONTENT_SECURITY_POLICY)
                .contains(&format!("'{HASH}'"))
        );
    }
    let changed = send(&router, "GET", "/store", &[("if-none-match", "\"other\"")]);
    assert_eq!(changed.status, StatusCode::OK);
    // Each representation has its own tag.
    let compressed = send(&router, "GET", SCRIPT_URL, &[("accept-encoding", "br")]);
    let brotli_tag = compressed.header(header::ETAG).to_owned();
    assert_eq!(brotli_tag, "\"AAAAbr=\"");
    let both = [
        ("accept-encoding", "br"),
        ("if-none-match", brotli_tag.as_str()),
    ];
    assert_eq!(
        send(&router, "GET", SCRIPT_URL, &both).status,
        StatusCode::NOT_MODIFIED
    );
    let identity = send(
        &router,
        "GET",
        SCRIPT_URL,
        &[("if-none-match", brotli_tag.as_str())],
    );
    assert_eq!(identity.status, StatusCode::OK);
    assert_eq!(identity.body, b"plain");
}

#[test]
fn html_files_never_use_precompressed_siblings() {
    let router = router();
    for target in ["/store", "/index.html"] {
        let reply = send(&router, "GET", target, &[("accept-encoding", "br, gzip")]);
        assert_eq!(reply.status, StatusCode::OK, "{target}");
        assert_eq!(reply.body, INDEX.as_bytes(), "{target}");
        assert!(!reply.headers.contains_key(header::CONTENT_ENCODING));
        assert_eq!(reply.header(header::ETAG), "\"AAAAindex=\"");
        assert_eq!(reply.header(header::VARY), "Accept-Encoding");
    }
    for name in ["index.html", "a/B.HTML", "c.htm", "d.Htm"] {
        assert!(super::is_html_file(name), "{name}");
    }
    for name in ["index.html.br", "a.js", "html", "a.htmlx"] {
        assert!(!super::is_html_file(name), "{name}");
    }
}

#[test]
fn head_is_answered_without_a_body_and_other_methods_are_refused() {
    let router = router();
    let head = send(&router, "HEAD", "/store", &[]);
    assert_eq!(head.status, StatusCode::OK);
    assert!(head.body.is_empty());
    assert_eq!(head.header(header::CONTENT_LENGTH), INDEX.len().to_string());
    assert_eq!(
        head.header(header::CONTENT_TYPE),
        "text/html; charset=utf-8"
    );
    for (method, target) in [
        ("POST", "/store"),
        ("PUT", "/store"),
        ("DELETE", "/missing.js"),
        ("OPTIONS", "/"),
    ] {
        let reply = send(&router, method, target, &[]);
        assert!(
            reply.is_error(405, "method_not_allowed"),
            "{method} {target}"
        );
        assert_eq!(reply.header(header::ALLOW), "GET, HEAD");
    }
    // An ambiguous path is refused before the method is looked at.
    assert!(send(&router, "POST", "/a%2Fb", &[]).is_error(400, "bad_request"));
}

#[test]
fn the_files_need_a_shell_and_base64_digests() {
    assert_eq!(
        SpaFiles::new(vec![entry("loading.css", "x", "AAAA")]).unwrap_err(),
        SpaFilesError::MissingShell
    );
    assert_eq!(
        SpaFiles::new(vec![
            entry("index.html", "x", "AAAA"),
            entry("index.html", "y", "BBBB"),
        ])
        .unwrap_err(),
        SpaFilesError::DuplicateName("index.html".to_owned())
    );
    for digest in ["", "AA\"AA", "AA AA", "AA\nAA", "AA-AA"] {
        assert_eq!(
            SpaFiles::new(vec![entry("index.html", "x", digest)]).unwrap_err(),
            SpaFilesError::BadDigest("index.html".to_owned()),
            "{digest:?}"
        );
    }
    assert!(SpaFiles::new(vec![entry("index.html", "x", "AAAA+/==")]).is_ok());
}

#[test]
fn quality_values_are_read_in_thousandths() {
    for (text, expected) in [
        ("0", 0),
        ("1", 1000),
        ("0.5", 500),
        ("0.005", 5),
        ("1.000", 1000),
        ("0.", 0),
        ("x", 0),
        ("2", 0),
        ("0.5x", 0),
        ("", 0),
    ] {
        assert_eq!(quality(text), expected, "{text:?}");
    }
    let mut headers = HeaderMap::new();
    headers.append(header::ACCEPT_ENCODING, "gzip".parse().unwrap());
    headers.append(header::ACCEPT_ENCODING, "br;q=0.1".parse().unwrap());
    assert_eq!(
        accepted_encodings(&headers),
        [("gzip", ".gz"), ("br", ".br")]
    );
}
