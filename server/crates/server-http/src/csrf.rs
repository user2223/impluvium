// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Cross-site request forgery check for state-changing requests.
//!
//! A request passes only with exactly one `Origin` header equal to the
//! configured external origin. `Sec-Fetch-Site` may be absent, `same-origin`
//! or `none`; every other value, and any repeated header, is rejected.
//! `Referer` plays no part, and `Sec-Fetch-Site: same-origin` never replaces
//! the `Origin` header. A request without any of these headers fails.

use axum::http::HeaderMap;

use crate::auth::ExternalOrigin;

/// Why a request failed the check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CsrfError {
    /// No `Origin` header.
    MissingOrigin,
    /// More than one `Origin` header.
    AmbiguousOrigin,
    /// The `Origin` is `null`, malformed, or another origin than the server's.
    ForeignOrigin,
    /// More than one `Sec-Fetch-Site` header.
    AmbiguousFetchSite,
    /// `Sec-Fetch-Site` is something other than `same-origin` or `none`.
    ForeignFetchSite,
}

/// Checks the request headers against the external origin.
pub fn check(headers: &HeaderMap, origin: &ExternalOrigin) -> Result<(), CsrfError> {
    let mut origins = headers.get_all("origin").iter();
    let presented = origins.next().ok_or(CsrfError::MissingOrigin)?;
    if origins.next().is_some() {
        return Err(CsrfError::AmbiguousOrigin);
    }
    if presented.as_bytes() != origin.as_str().as_bytes() {
        return Err(CsrfError::ForeignOrigin);
    }
    let mut sites = headers.get_all("sec-fetch-site").iter();
    match (sites.next(), sites.next()) {
        (None, _) => Ok(()),
        (Some(_), Some(_)) => Err(CsrfError::AmbiguousFetchSite),
        (Some(site), None) => match site.as_bytes() {
            b"same-origin" | b"none" => Ok(()),
            _ => Err(CsrfError::ForeignFetchSite),
        },
    }
}

#[cfg(test)]
mod tests {
    use axum::http::HeaderValue;

    use super::*;
    use crate::auth::Scheme;

    fn origin() -> ExternalOrigin {
        ExternalOrigin::new(Scheme::Https, "app.example.test", None).unwrap()
    }

    fn verdict(pairs: &[(&'static str, &str)]) -> Result<(), CsrfError> {
        let mut headers = HeaderMap::new();
        for (name, value) in pairs {
            headers.append(*name, HeaderValue::from_str(value).unwrap());
        }
        check(&headers, &origin())
    }

    const GOOD: &str = "https://app.example.test";

    #[test]
    fn same_origin_write_passes_with_or_without_fetch_metadata() {
        assert_eq!(verdict(&[("origin", GOOD)]), Ok(()));
        assert_eq!(
            verdict(&[("origin", GOOD), ("sec-fetch-site", "same-origin")]),
            Ok(())
        );
        assert_eq!(
            verdict(&[("origin", GOOD), ("sec-fetch-site", "none")]),
            Ok(())
        );
    }

    #[test]
    fn a_request_without_any_headers_fails() {
        assert_eq!(verdict(&[]), Err(CsrfError::MissingOrigin));
    }

    #[test]
    fn fetch_metadata_never_replaces_the_origin() {
        assert_eq!(
            verdict(&[("sec-fetch-site", "same-origin")]),
            Err(CsrfError::MissingOrigin)
        );
        assert_eq!(
            verdict(&[("referer", "https://app.example.test/page")]),
            Err(CsrfError::MissingOrigin)
        );
    }

    #[test]
    fn foreign_neighbour_and_opaque_origins_fail() {
        for foreign in [
            "https://evil.test",
            "https://other.example.test",
            "https://example.test",
            "https://app.example.test:8443",
            "http://app.example.test",
            "https://app.example.test/",
            "https://APP.example.test",
            "null",
            "",
        ] {
            assert_eq!(
                verdict(&[("origin", foreign)]),
                Err(CsrfError::ForeignOrigin),
                "{foreign:?}"
            );
        }
    }

    #[test]
    fn neighbouring_subdomain_with_same_site_fails() {
        assert_eq!(
            verdict(&[
                ("origin", "https://neighbour.example.test"),
                ("sec-fetch-site", "same-site"),
            ]),
            Err(CsrfError::ForeignOrigin)
        );
        // Even with the right origin, same-site is not accepted.
        assert_eq!(
            verdict(&[("origin", GOOD), ("sec-fetch-site", "same-site")]),
            Err(CsrfError::ForeignFetchSite)
        );
    }

    #[test]
    fn cross_site_unknown_and_odd_fetch_values_fail() {
        for site in [
            "cross-site",
            "same-site",
            "",
            "unknown",
            "Same-Origin",
            "same-origin, none",
        ] {
            assert_eq!(
                verdict(&[("origin", GOOD), ("sec-fetch-site", site)]),
                Err(CsrfError::ForeignFetchSite),
                "{site:?}"
            );
        }
    }

    #[test]
    fn repeated_headers_are_rejected() {
        assert_eq!(
            verdict(&[("origin", GOOD), ("origin", GOOD)]),
            Err(CsrfError::AmbiguousOrigin)
        );
        assert_eq!(
            verdict(&[
                ("origin", GOOD),
                ("sec-fetch-site", "same-origin"),
                ("sec-fetch-site", "same-origin"),
            ]),
            Err(CsrfError::AmbiguousFetchSite)
        );
    }
}
