// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Request and response bodies of the web API, by area.
//!
//! Every type here is both the serde form that the server reads and writes and, through
//! ts-rs, the TypeScript declaration the web interface uses. Field names are camelCase unless a
//! field says otherwise. A field that can be `null` is declared as `T | null` and always present;
//! a field that may be absent is optional (`name?: T`) and never `null`.

/// Writes a `Debug` that shows `[redacted]` for the secret fields, so no log line or panic message
/// can carry a password, a code or an invitation id.
macro_rules! redacted_debug {
    ($ty:ident, shown: [$($shown:ident),*], hidden: [$($hidden:ident),+]) => {
        impl ::std::fmt::Debug for $ty {
            fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                formatter
                    .debug_struct(stringify!($ty))
                    $(.field(stringify!($shown), &self.$shown))*
                    $(.field(stringify!($hidden), &format_args!("[redacted]")))+
                    .finish()
            }
        }
    };
}

pub mod admin_auth;
pub mod admin_company;
pub mod admin_game;
pub mod admin_library;
pub mod admin_tags;
pub mod admin_users;
pub mod auth;
pub mod catalog;
pub mod collection;
pub mod object;
pub mod screenshot;
pub mod server;
pub mod store;
pub mod token;
pub mod user;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// A missing optional patch field is unchanged; a present field must carry a nonnull value.
/// `default` on the containing field handles absence without calling this deserializer.
fn non_null_option<'de, T, D>(deserializer: D) -> Result<Option<T>, D::Error>
where
    T: Deserialize<'de>,
    D: serde::Deserializer<'de>,
{
    T::deserialize(deserializer).map(Some)
}

/// Parses a paging string using only ASCII decimal digits; empty input and overflow fail.
fn paging_integer(value: &str) -> Result<u32, std::num::ParseIntError> {
    let digits = if value.bytes().all(|byte| byte.is_ascii_digit()) {
        value
    } else {
        ""
    };
    digits.parse()
}

/// The body `{}` of an answer that has nothing to report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[ts(type = "Record<string, never>")]
pub struct EmptyObject {}

#[cfg(test)]
mod admin_catalog_tests;
#[cfg(test)]
mod catalog_tests;
#[cfg(test)]
mod tests;
