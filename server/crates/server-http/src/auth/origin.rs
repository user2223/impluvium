// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! The configured external origin.
//!
//! This is the one authority for cookies, redirect targets and the CSRF
//! check. `Host`, `Forwarded` and `X-Forwarded-Proto` never replace it.

use std::error::Error;
use std::fmt;

/// URL scheme of the external origin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scheme {
    /// Plain `http`.
    Http,
    /// `https`.
    Https,
}

/// The origin clients use to reach the server: scheme, host and port.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalOrigin {
    scheme: Scheme,
    serialized: String,
}

/// Why an origin could not be built.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OriginError {
    /// The host is empty, not lowercase, or contains characters that do not
    /// belong in a host name or bracketed address.
    InvalidHost,
    /// The port is zero.
    InvalidPort,
}

impl fmt::Display for OriginError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidHost => formatter.write_str("the external origin has an invalid host"),
            Self::InvalidPort => formatter.write_str("the external origin has an invalid port"),
        }
    }
}

impl Error for OriginError {}

impl ExternalOrigin {
    /// Builds the origin from a lowercase host (IPv6 addresses in square
    /// brackets) and an optional port. The default port of the scheme is
    /// dropped, as browsers do when they send an `Origin` header.
    pub fn new(scheme: Scheme, host: &str, port: Option<u16>) -> Result<Self, OriginError> {
        let valid = !host.is_empty()
            && host.len() <= 253
            && host.bytes().all(|byte| {
                byte.is_ascii_lowercase()
                    || byte.is_ascii_digit()
                    || matches!(byte, b'.' | b'-' | b'[' | b']' | b':')
            });
        if !valid {
            return Err(OriginError::InvalidHost);
        }
        let port = match (scheme, port) {
            (_, Some(0)) => return Err(OriginError::InvalidPort),
            (Scheme::Http, Some(80)) | (Scheme::Https, Some(443)) => None,
            (_, port) => port,
        };
        let name = match scheme {
            Scheme::Http => "http",
            Scheme::Https => "https",
        };
        let serialized = match port {
            Some(port) => format!("{name}://{host}:{port}"),
            None => format!("{name}://{host}"),
        };
        Ok(Self { scheme, serialized })
    }

    /// The scheme of the origin.
    pub fn scheme(&self) -> Scheme {
        self.scheme
    }

    /// Whether the origin uses `https`.
    pub fn is_https(&self) -> bool {
        self.scheme == Scheme::Https
    }

    /// The origin as a browser serializes it in an `Origin` header.
    pub fn as_str(&self) -> &str {
        &self.serialized
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_like_a_browser_origin_header() {
        let origin = |scheme, host, port| ExternalOrigin::new(scheme, host, port).unwrap();
        assert_eq!(
            origin(Scheme::Https, "example.test", None).as_str(),
            "https://example.test"
        );
        assert_eq!(
            origin(Scheme::Https, "example.test", Some(443)).as_str(),
            "https://example.test"
        );
        assert_eq!(
            origin(Scheme::Http, "192.0.2.10", Some(8080)).as_str(),
            "http://192.0.2.10:8080"
        );
        assert_eq!(
            origin(Scheme::Http, "[2001:db8::1]", Some(80)).as_str(),
            "http://[2001:db8::1]"
        );
    }

    #[test]
    fn rejects_hosts_and_ports_that_cannot_be_an_origin() {
        for host in ["", "Example.test", "a/b", "a b", "user@host", "a?b", "a#b"] {
            assert_eq!(
                ExternalOrigin::new(Scheme::Https, host, None),
                Err(OriginError::InvalidHost),
                "{host:?}"
            );
        }
        assert_eq!(
            ExternalOrigin::new(Scheme::Https, "example.test", Some(0)),
            Err(OriginError::InvalidPort)
        );
    }
}
