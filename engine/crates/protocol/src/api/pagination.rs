// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: 0BSD

//! Page size normalization and opaque-token validation after request field rules.

use crate::{
    decode::{Diagnostic, Reason, error},
    keys::SwitchTime,
};

pub const fn page_size(requested: u32) -> u32 {
    if requested == 0 {
        50
    } else if requested > 200 {
        200
    } else {
        requested
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Method {
    ListDevices,
    ListLibrary,
    ListVersions,
}
impl Method {
    fn field(self) -> &'static str {
        match self {
            Self::ListDevices => "ListDevicesRequest.page_token",
            Self::ListLibrary => "ListLibraryRequest.page_token",
            Self::ListVersions => "ListVersionsRequest.page_token",
        }
    }
}
/// Bind the exact request values, including the unnormalized page_size. In
/// particular, zero and fifty are different 'other request fields'.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Binding<'a> {
    pub method: Method,
    pub page_size: u32,
    pub game_id: Option<&'a str>,
}
/// The token authenticator supplies this observation. No token format or key
/// derivation is prescribed by the protocol or invented at this layer.
pub struct Token<'a> {
    pub binding: Binding<'a>,
    pub issued: SwitchTime,
}
/// Expiry has a wire reason but no catalogued local reason in errors.md §5.
/// Keep it typed rather than inventing an extra local diagnostic spelling.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Failure {
    Invalid(Diagnostic),
    Expired { field: &'static str },
}
impl Failure {
    pub fn wire(self) -> super::errors::WireError {
        use super::errors::{Code, WireError};
        WireError {
            code: match self {
                Self::Invalid(_) => Code::invalid_argument,
                Self::Expired { .. } => Code::failed_precondition,
            },
            reason: match self {
                Self::Invalid(_) => None,
                Self::Expired { .. } => {
                    Some(crate::proto::impluvium::v2::ErrorReason::PageTokenExpired)
                }
            },
        }
    }
}
/// `None` means a malformed/unverifiable nonempty token. Binding checks precede
/// expiry; an empty token starts a listing without consulting stored token data.
pub fn validate(
    token_text: &str,
    token: Option<&Token<'_>>,
    request: &Binding<'_>,
    now: SwitchTime,
) -> Result<(), Failure> {
    if token_text.is_empty() {
        return Ok(());
    }
    let field = request.method.field();
    let token = token
        .filter(|t| t.binding == *request)
        .ok_or(Failure::Invalid(error(Reason::invalid_page_token, field)))?;
    let nanos = |t: SwitchTime| i128::from(t.seconds) * 1_000_000_000 + i128::from(t.nanos);
    if nanos(now) >= nanos(token.issued) + 3_600_000_000_000 {
        Err(Failure::Expired { field })
    } else {
        Ok(())
    }
}
