// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: 0BSD

//! Local transport diagnostics never become wire reasons.

pub use impluvium_protocol::api::errors::{Action, Code, WireError, client_action};
pub use impluvium_protocol::decode::{Diagnostic, Reason};

pub(crate) const fn local(reason: Reason) -> Diagnostic {
    Diagnostic { reason, field: "" }
}

/// Only broken error bodies use this mapping. Head, redirect and coding
/// failures retain their own diagnostics, independently of HTTP status.
pub const fn broken_body(status: u16) -> Diagnostic {
    local(match status {
        413 => Reason::request_too_large,
        429 => Reason::rate_limited,
        502..=504 => Reason::server_unavailable,
        _ => Reason::broken_error_body,
    })
}

pub fn retry_after(value: &str) -> Option<u32> {
    if value.is_empty() || value.len() > 10 || !value.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    value.parse::<u32>().ok().filter(|n| *n <= 86_400)
}
