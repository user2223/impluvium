// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! The port that turns a presented credential into a principal.
//!
//! The server only knows this trait. The component that checks sessions and
//! tokens against stored state implements it; tests use synthetic providers.

use std::fmt;
use std::future::Future;
use std::pin::Pin;

use crate::principal::Principal;

/// A boxed future, so the port can be used as a trait object.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// A credential as presented by the client, not yet checked.
#[derive(Clone, PartialEq, Eq)]
pub enum Credential {
    /// The value of the session cookie.
    SessionCookie(String),
    /// The token of an `Authorization: Bearer` header.
    Bearer(String),
}

impl Credential {
    /// The secret value.
    pub fn secret(&self) -> &str {
        match self {
            Self::SessionCookie(value) | Self::Bearer(value) => value,
        }
    }
}

impl fmt::Debug for Credential {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SessionCookie(_) => formatter.write_str("SessionCookie(<redacted>)"),
            Self::Bearer(_) => formatter.write_str("Bearer(<redacted>)"),
        }
    }
}

/// Why a credential does not resolve to a principal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CredentialError {
    /// No such session or token.
    Unknown,
    /// It existed but ran out.
    Expired,
    /// It was revoked.
    Revoked,
}

/// Resolves credentials. An implementation returns a principal only for a
/// credential that is known, unexpired and not revoked; every other case is
/// an error, never an anonymous or partial principal.
pub trait CredentialPort: Send + Sync {
    /// Resolves one credential. A session cookie has to resolve to a session
    /// principal and a bearer token to a token or device principal; the
    /// server treats any other answer as unknown.
    fn resolve<'a>(
        &'a self,
        credential: &'a Credential,
    ) -> BoxFuture<'a, Result<Principal, CredentialError>>;
}

/// A port that knows no credential at all.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoCredentials;

impl CredentialPort for NoCredentials {
    fn resolve<'a>(
        &'a self,
        _credential: &'a Credential,
    ) -> BoxFuture<'a, Result<Principal, CredentialError>> {
        Box::pin(async { Err(CredentialError::Unknown) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_output_hides_the_secret() {
        let text = format!("{:?}", Credential::Bearer("imp_pat_secret".into()));
        assert!(!text.contains("secret"));
        assert_eq!(Credential::SessionCookie("abc".into()).secret(), "abc");
    }
}
