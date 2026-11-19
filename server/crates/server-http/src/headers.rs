// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! The protective response headers.
//!
//! One layer applies the header contract to every response of the router.
//! It replaces a header instead of appending a second value, never touches
//! the status, the body or `Content-Length`, and does not buffer anything,
//! so streamed bodies keep streaming.
//!
//! The contract by response class:
//!
//! - every response: `X-Content-Type-Options: nosniff` and
//!   `Referrer-Policy: no-referrer`;
//! - HTML responses and error responses (status 400 and above):
//!   `X-Frame-Options: DENY`;
//! - HTML responses: `Cross-Origin-Opener-Policy: same-origin` and a
//!   `Permissions-Policy` that switches camera, microphone and geolocation off;
//! - the single-page application shell, marked with [`SpaShell`]: a
//!   `Content-Security-Policy` that allows only same-origin scripts plus the
//!   configured inline script hashes;
//! - every other response: a `Content-Security-Policy` that allows nothing.
//!
//! Cross-origin resource sharing headers, `Strict-Transport-Security`,
//! `Cross-Origin-Embedder-Policy`, `Cross-Origin-Resource-Policy` and
//! `X-XSS-Protection` are never sent, and the layer removes them when a
//! handler set them. The script policy cannot be loosened.

use std::fmt;
use std::sync::Arc;

use axum::extract::{Request, State};
use axum::http::{HeaderMap, HeaderName, HeaderValue, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

const NOSNIFF: &str = "nosniff";
const NO_REFERRER: &str = "no-referrer";
const FRAME_DENY: &str = "DENY";
const OPENER_POLICY: &str = "same-origin";
const PERMISSIONS: &str = "camera=(), microphone=(), geolocation=()";
const RESTRICTIVE_CSP: &str = "default-src 'none'; base-uri 'none'; form-action 'none'; \
frame-ancestors 'none'; object-src 'none'";
const SPA_CSP_BEFORE_HASHES: &str = "default-src 'none'; script-src 'self'";
const SPA_CSP_AFTER_HASHES: &str = "; style-src 'self'; img-src 'self' data: \
https://www.giantbomb.com https://images.pcgamingwiki.com https://images.igdb.com \
https://*.steamstatic.com; font-src 'self'; connect-src 'self'; manifest-src 'self'; \
base-uri 'none'; form-action 'self'; frame-ancestors 'none'; object-src 'none'";

const OPENER: HeaderName = HeaderName::from_static("cross-origin-opener-policy");
const PERMISSIONS_POLICY: HeaderName = HeaderName::from_static("permissions-policy");

/// Headers the server never sends.
const FORBIDDEN: [&str; 4] = [
    "strict-transport-security",
    "cross-origin-embedder-policy",
    "cross-origin-resource-policy",
    "x-xss-protection",
];

/// Marks a response as the single-page application shell. The handler that
/// serves the shell wraps its response; the layer never guesses from the
/// content.
///
/// A shell served with an error status must also be wrapped in
/// [`PreformattedError`](crate::error::PreformattedError); otherwise the
/// error layer replaces it with the JSON error shape.
#[derive(Debug, Clone)]
pub struct SpaShell<R>(pub R);

#[derive(Debug, Clone, Copy)]
struct SpaMarker;

impl<R: IntoResponse> IntoResponse for SpaShell<R> {
    fn into_response(self) -> Response {
        let mut response = self.0.into_response();
        response.extensions_mut().insert(SpaMarker);
        response
    }
}

/// A script hash for the content security policy of the shell, in the form
/// `sha256-` followed by the base64 digest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptHash(String);

/// The text is not a SHA-256 script hash.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidScriptHash;

impl fmt::Display for InvalidScriptHash {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a script hash is `sha256-` followed by 44 base64 characters")
    }
}

impl std::error::Error for InvalidScriptHash {}

impl ScriptHash {
    /// Accepts `sha256-` followed by the 44 characters of a base64 SHA-256 digest.
    pub fn parse(text: &str) -> Result<Self, InvalidScriptHash> {
        let digest = text.strip_prefix("sha256-").ok_or(InvalidScriptHash)?;
        let bytes = digest.as_bytes();
        let valid = bytes.len() == 44
            && bytes.last() == Some(&b'=')
            && bytes[..43]
                .iter()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(*byte, b'+' | b'/'));
        if valid {
            Ok(Self(text.to_owned()))
        } else {
            Err(InvalidScriptHash)
        }
    }
}

/// The header values the layer applies.
#[derive(Debug, Clone)]
pub struct HeaderPolicy {
    spa_csp: HeaderValue,
}

impl HeaderPolicy {
    /// Builds the policy; the hashes name the inline scripts of the shell.
    pub fn new(hashes: &[ScriptHash]) -> Self {
        let mut csp = String::from(SPA_CSP_BEFORE_HASHES);
        for hash in hashes {
            csp.push_str(" '");
            csp.push_str(&hash.0);
            csp.push('\'');
        }
        csp.push_str(SPA_CSP_AFTER_HASHES);
        // The text consists of validated base64 and fixed ASCII.
        let spa_csp = HeaderValue::from_str(&csp)
            .unwrap_or_else(|_| HeaderValue::from_static(RESTRICTIVE_CSP));
        Self { spa_csp }
    }
}

impl Default for HeaderPolicy {
    fn default() -> Self {
        Self::new(&[])
    }
}

fn is_html(headers: &HeaderMap) -> bool {
    headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::trim_start)
        .is_some_and(|value| {
            let value = value.to_ascii_lowercase();
            value.starts_with("text/html") || value.starts_with("application/xhtml+xml")
        })
}

fn set(headers: &mut HeaderMap, name: &HeaderName, value: Option<HeaderValue>) {
    match value {
        Some(value) => {
            headers.insert(name.clone(), value);
        }
        None => {
            headers.remove(name);
        }
    }
}

/// Applies the header contract to one response.
pub(crate) fn apply_to(policy: &HeaderPolicy, response: &mut Response) {
    let spa = response.extensions().get::<SpaMarker>().is_some();
    let status = response.status();
    let error = status.is_client_error() || status.is_server_error();
    let html = spa || is_html(response.headers());
    let headers = response.headers_mut();
    let names: Vec<HeaderName> = headers
        .keys()
        .filter(|name| {
            name.as_str().starts_with("access-control-") || FORBIDDEN.contains(&name.as_str())
        })
        .cloned()
        .collect();
    for name in names {
        headers.remove(name);
    }
    let when =
        |condition: bool, value: &'static str| condition.then(|| HeaderValue::from_static(value));
    let fixed = |value: &'static str| Some(HeaderValue::from_static(value));
    set(headers, &header::X_CONTENT_TYPE_OPTIONS, fixed(NOSNIFF));
    set(headers, &header::REFERRER_POLICY, fixed(NO_REFERRER));
    set(
        headers,
        &header::X_FRAME_OPTIONS,
        when(html || error, FRAME_DENY),
    );
    set(headers, &OPENER, when(html, OPENER_POLICY));
    set(headers, &PERMISSIONS_POLICY, when(html, PERMISSIONS));
    let csp = if spa {
        policy.spa_csp.clone()
    } else {
        HeaderValue::from_static(RESTRICTIVE_CSP)
    };
    set(headers, &header::CONTENT_SECURITY_POLICY, Some(csp));
}

/// Layer function applying [`apply_to`] to the response of the rest of the stack.
pub(crate) async fn apply(
    State(policy): State<Arc<HeaderPolicy>>,
    request: Request,
    next: Next,
) -> Response {
    let mut response = next.run(request).await;
    apply_to(&policy, &mut response);
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    const DIGEST: &str = "sha256-47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU=";

    #[test]
    fn script_hashes_are_checked() {
        assert!(ScriptHash::parse(DIGEST).is_ok());
        for bad in [
            "",
            "sha256-",
            "sha384-47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU=",
            "sha256-47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU",
            "sha256-47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuF'=",
            "sha256-47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuF =",
            "'sha256-47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU='",
        ] {
            assert!(ScriptHash::parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn shell_policy_lists_the_hashes_after_self() {
        let policy = HeaderPolicy::new(&[ScriptHash::parse(DIGEST).unwrap()]);
        let text = policy.spa_csp.to_str().unwrap();
        assert!(text.starts_with(&format!(
            "default-src 'none'; script-src 'self' '{DIGEST}'; style-src 'self'; "
        )));
        let empty = HeaderPolicy::default();
        assert!(
            empty
                .spa_csp
                .to_str()
                .unwrap()
                .starts_with("default-src 'none'; script-src 'self'; style-src 'self'; ")
        );
    }
}
