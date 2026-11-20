// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Tag administration reuses the public scalar tag projection and array.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// An opaque catalog path id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AdminTagParams {
    /// The nonempty record id.
    pub id: String,
}

impl AdminTagParams {
    /// Refuses an empty path id before accessing storage.
    pub fn is_valid(&self) -> bool {
        !self.id.is_empty()
    }
}

/// Creates a tag; the inherited handler checks tags:read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AdminTagCreate {
    /// The tag name, including empty text as accepted by the handler.
    pub name: String,
}
