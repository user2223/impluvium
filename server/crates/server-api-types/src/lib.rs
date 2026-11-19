// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Public web contracts, wire adapters and error codes.
//!
//! Database models do not belong to this interface.

pub mod error;

pub use error::{ApiError, CatalogEntry, ErrorCode, FieldError, catalog};
