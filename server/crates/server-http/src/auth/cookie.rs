// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Session cookie names and `Set-Cookie` values.
//!
//! The attributes follow the scheme of the configured external origin and
//! nothing else: over https the cookie is `Secure` and carries the
//! `__Host-` prefix, over plain http (localhost, or the explicit insecure
//! local-network mode) it has neither, because browsers refuse a `__Host-`
//! cookie without `Secure`. Issuing, rotating and revoking sessions happens
//! elsewhere; this module only builds the header values.

use std::error::Error;
use std::fmt;

use super::origin::ExternalOrigin;

/// Cookie name over https.
pub const SECURE_COOKIE_NAME: &str = "__Host-impluvium_session";
/// Cookie name over plain http.
pub const PLAIN_COOKIE_NAME: &str = "impluvium_session";
/// Longest accepted credential value in bytes.
pub(crate) const MAX_VALUE_LEN: usize = 512;

/// The name of the session cookie for the origin.
pub fn session_cookie_name(origin: &ExternalOrigin) -> &'static str {
    if origin.is_https() {
        SECURE_COOKIE_NAME
    } else {
        PLAIN_COOKIE_NAME
    }
}

/// A cookie value that cannot be sent safely.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidCookieValue;

impl fmt::Display for InvalidCookieValue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("the cookie value is empty, too long or has invalid characters")
    }
}

impl Error for InvalidCookieValue {}

/// Whether a credential value is made of URL-safe token characters only.
pub(crate) fn is_cookie_token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_VALUE_LEN
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~'))
}

fn attributes(origin: &ExternalOrigin, max_age_secs: u32) -> String {
    let secure = if origin.is_https() { "; Secure" } else { "" };
    format!("; Max-Age={max_age_secs}; Path=/; HttpOnly; SameSite=Lax{secure}")
}

/// The `Set-Cookie` value that stores a session token.
pub fn set_session_cookie(
    origin: &ExternalOrigin,
    token: &str,
    max_age_secs: u32,
) -> Result<String, InvalidCookieValue> {
    if !is_cookie_token(token) {
        return Err(InvalidCookieValue);
    }
    Ok(format!(
        "{}={token}{}",
        session_cookie_name(origin),
        attributes(origin, max_age_secs)
    ))
}

/// The `Set-Cookie` value that removes the session cookie.
pub fn clear_session_cookie(origin: &ExternalOrigin) -> String {
    format!("{}={}", session_cookie_name(origin), attributes(origin, 0))
}

#[cfg(test)]
mod tests {
    use super::super::origin::Scheme;
    use super::*;

    fn origin(scheme: Scheme, host: &str) -> ExternalOrigin {
        ExternalOrigin::new(scheme, host, None).unwrap()
    }

    #[test]
    fn https_origin_gets_the_host_prefix_and_secure() {
        let https = origin(Scheme::Https, "example.test");
        assert_eq!(session_cookie_name(&https), "__Host-impluvium_session");
        assert_eq!(
            set_session_cookie(&https, "abc-DEF_1.2~3", 3600).unwrap(),
            "__Host-impluvium_session=abc-DEF_1.2~3; Max-Age=3600; Path=/; HttpOnly; SameSite=Lax; Secure"
        );
        assert_eq!(
            clear_session_cookie(&https),
            "__Host-impluvium_session=; Max-Age=0; Path=/; HttpOnly; SameSite=Lax; Secure"
        );
    }

    #[test]
    fn http_origin_gets_the_plain_name_without_secure() {
        // The scheme decides, whatever the host is.
        for host in ["localhost", "192.0.2.10"] {
            let http = origin(Scheme::Http, host);
            assert_eq!(session_cookie_name(&http), "impluvium_session");
            let value = set_session_cookie(&http, "abc", 60).unwrap();
            assert_eq!(
                value,
                "impluvium_session=abc; Max-Age=60; Path=/; HttpOnly; SameSite=Lax"
            );
            assert!(!value.contains("Secure"));
            assert!(!value.contains("Domain"));
        }
    }

    #[test]
    fn unsafe_values_are_refused() {
        let https = origin(Scheme::Https, "example.test");
        for bad in [
            "",
            "a b",
            "a;b",
            "a,b",
            "a\r\nSet-Cookie: x=y",
            "\"a\"",
            "ä",
        ] {
            assert_eq!(
                set_session_cookie(&https, bad, 60),
                Err(InvalidCookieValue),
                "{bad:?}"
            );
        }
        assert!(set_session_cookie(&https, &"a".repeat(MAX_VALUE_LEN + 1), 60).is_err());
    }
}
