// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Public web contracts, wire adapters and error codes.
//!
//! Database models do not belong to this interface.
//!
//! The shared wire shapes (64-bit decimals, three-state fields, timestamps, JSON values, pages and
//! string enums) are declared with serde and ts-rs together, so the Rust type and its TypeScript
//! declaration describe the same JSON; [`export`] writes the declarations.

pub mod contracts;
pub mod dec64;
pub mod enums;
pub mod error;
pub mod export;
pub mod json;
pub mod page;
pub mod time;
pub mod tri;

pub use dec64::{Dec64Error, DecI64, DecU64};
pub use enums::{Platform, RuntimeEnum, runtime_enums};
pub use error::{ApiError, CatalogEntry, ErrorCode, FieldError, catalog};
pub use json::{JsonBound, JsonValue};
pub use page::{CursorPage, Page, PageQuery, PageQueryError, PageWindow};
pub use time::{Timestamp, TimestampError};
pub use tri::Tri;
/// The trait that route contracts bound their request and response shapes by.
pub use ts_rs::TS;
