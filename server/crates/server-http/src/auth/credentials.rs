// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Reads the credential a request presents.
//!
//! At most one credential counts. A request that presents two kinds, two
//! `Authorization` headers, or two session cookies is rejected: the server
//! never picks "the first one".

use axum::http::{HeaderMap, header};
use server_domain::credential::Credential;

use super::cookie::{is_cookie_token, session_cookie_name};
use super::origin::ExternalOrigin;

/// Why the credentials of a request were rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CredentialsError {
    /// A header is not text, uses another scheme than `Bearer`, or holds an
    /// empty, overlong or malformed value.
    Malformed,
    /// More than one `Authorization` header or session cookie.
    Ambiguous,
    /// A session cookie and a bearer token together.
    Conflicting,
}

/// The credential of the request: `None` when it presents none.
pub fn extract(
    headers: &HeaderMap,
    origin: &ExternalOrigin,
) -> Result<Option<Credential>, CredentialsError> {
    let bearer = bearer(headers)?;
    let cookie = session_cookie(headers, session_cookie_name(origin))?;
    match (cookie, bearer) {
        (Some(_), Some(_)) => Err(CredentialsError::Conflicting),
        (Some(value), None) => Ok(Some(Credential::SessionCookie(value))),
        (None, Some(value)) => Ok(Some(Credential::Bearer(value))),
        (None, None) => Ok(None),
    }
}

fn bearer(headers: &HeaderMap) -> Result<Option<String>, CredentialsError> {
    let mut values = headers.get_all(header::AUTHORIZATION).iter();
    let Some(value) = values.next() else {
        return Ok(None);
    };
    if values.next().is_some() {
        return Err(CredentialsError::Ambiguous);
    }
    let text = value.to_str().map_err(|_| CredentialsError::Malformed)?;
    let (scheme, token) = text.split_once(' ').ok_or(CredentialsError::Malformed)?;
    if !scheme.eq_ignore_ascii_case("bearer") || !is_bearer_token(token) {
        return Err(CredentialsError::Malformed);
    }
    Ok(Some(token.to_owned()))
}

/// Token characters of a bearer value (RFC 6750 `b64token`).
fn is_bearer_token(token: &str) -> bool {
    !token.is_empty()
        && token.len() <= super::cookie::MAX_VALUE_LEN
        && token.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(byte, b'-' | b'.' | b'_' | b'~' | b'+' | b'/' | b'=')
        })
}

fn session_cookie(headers: &HeaderMap, name: &str) -> Result<Option<String>, CredentialsError> {
    let mut found: Option<&str> = None;
    for line in headers.get_all(header::COOKIE) {
        let text = line.to_str().map_err(|_| CredentialsError::Malformed)?;
        for pair in text.split(';') {
            let pair = pair.trim();
            let Some((cookie_name, value)) = pair.split_once('=') else {
                continue;
            };
            if cookie_name.trim() != name {
                continue;
            }
            if found.is_some() {
                return Err(CredentialsError::Ambiguous);
            }
            found = Some(value.trim());
        }
    }
    match found {
        None => Ok(None),
        Some(value) if is_cookie_token(value) => Ok(Some(value.to_owned())),
        Some(_) => Err(CredentialsError::Malformed),
    }
}

#[cfg(test)]
mod tests {
    use axum::http::HeaderValue;

    use super::super::origin::Scheme;
    use super::*;

    fn https() -> ExternalOrigin {
        ExternalOrigin::new(Scheme::Https, "example.test", None).unwrap()
    }

    fn headers(pairs: &[(&'static str, &str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (name, value) in pairs {
            map.append(*name, HeaderValue::from_str(value).unwrap());
        }
        map
    }

    fn read(pairs: &[(&'static str, &str)]) -> Result<Option<Credential>, CredentialsError> {
        extract(&headers(pairs), &https())
    }

    #[test]
    fn no_credential_is_none() {
        assert_eq!(read(&[]), Ok(None));
        assert_eq!(read(&[("cookie", "theme=dark; other=1")]), Ok(None));
    }

    #[test]
    fn session_cookie_is_found_among_other_cookies_and_lines() {
        let expected = Ok(Some(Credential::SessionCookie("tok-1".into())));
        assert_eq!(
            read(&[("cookie", "a=1; __Host-impluvium_session=tok-1; b=2")]),
            expected
        );
        assert_eq!(
            read(&[
                ("cookie", "a=1"),
                ("cookie", "__Host-impluvium_session=tok-1")
            ]),
            expected
        );
    }

    #[test]
    fn only_the_cookie_name_of_the_origin_counts() {
        assert_eq!(read(&[("cookie", "impluvium_session=tok")]), Ok(None));
        let plain = ExternalOrigin::new(Scheme::Http, "localhost", None).unwrap();
        assert_eq!(
            extract(
                &headers(&[("cookie", "__Host-impluvium_session=tok")]),
                &plain
            ),
            Ok(None)
        );
    }

    #[test]
    fn repeated_session_cookies_are_ambiguous_even_when_equal() {
        for pairs in [
            &[(
                "cookie",
                "__Host-impluvium_session=a; __Host-impluvium_session=b",
            )][..],
            &[(
                "cookie",
                "__Host-impluvium_session=a; __Host-impluvium_session=a",
            )][..],
            &[
                ("cookie", "__Host-impluvium_session=a"),
                ("cookie", "__Host-impluvium_session=b"),
            ][..],
        ] {
            assert_eq!(read(pairs), Err(CredentialsError::Ambiguous));
        }
    }

    #[test]
    fn empty_or_malformed_session_values_are_rejected() {
        for value in ["", " ", "\"quoted\"", "a b", "a,b"] {
            let cookie = format!("__Host-impluvium_session={value}");
            assert_eq!(
                read(&[("cookie", &cookie)]),
                Err(CredentialsError::Malformed),
                "{value:?}"
            );
        }
    }

    #[test]
    fn bearer_header_is_read_once() {
        assert_eq!(
            read(&[("authorization", "Bearer imp_pat_abc")]),
            Ok(Some(Credential::Bearer("imp_pat_abc".into())))
        );
        assert_eq!(
            read(&[("authorization", "bearer abc")]),
            Ok(Some(Credential::Bearer("abc".into())))
        );
        assert_eq!(
            read(&[("authorization", "Bearer a"), ("authorization", "Bearer b")]),
            Err(CredentialsError::Ambiguous)
        );
        for bad in [
            "Bearer",
            "Bearer ",
            "Bearer  abc",
            "Basic abc",
            "abc",
            "Bearer a b",
        ] {
            assert_eq!(
                read(&[("authorization", bad)]),
                Err(CredentialsError::Malformed),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn cookie_and_bearer_together_conflict() {
        assert_eq!(
            read(&[
                ("cookie", "__Host-impluvium_session=tok"),
                ("authorization", "Bearer abc"),
            ]),
            Err(CredentialsError::Conflicting)
        );
    }

    #[test]
    fn non_text_headers_are_rejected() {
        let mut map = HeaderMap::new();
        map.insert(header::COOKIE, HeaderValue::from_bytes(b"a=\xff").unwrap());
        assert_eq!(extract(&map, &https()), Err(CredentialsError::Malformed));
    }
}
