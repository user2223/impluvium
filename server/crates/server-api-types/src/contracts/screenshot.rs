// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Screenshot metadata. File contents are reached through the object reference.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::object::{ObjectParams, ObjectReference};
use crate::time::Timestamp;

/// The screenshot id for reading or deleting metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ScreenshotParams {
    /// The screenshot id, checked as an object-style filename.
    pub id: String,
}

impl ScreenshotParams {
    /// Validates the name before a lookup; do not silently sanitize it into another id.
    pub fn is_valid(&self) -> bool {
        ObjectParams {
            id: self.id.clone(),
        }
        .is_valid()
    }
}

/// A screenshot record owned by the requesting account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Screenshot {
    /// The screenshot id.
    pub id: String,
    /// The game id.
    pub game_id: String,
    /// The account id.
    pub user_id: String,
    /// The object reference.
    pub object_id: ObjectReference,
    /// The stored flag; it does not bypass ownership checks.
    pub private: bool,
    /// When the screenshot was uploaded.
    pub created_at: Timestamp,
}

/// Both screenshot list routes return a bare array without pagination.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ScreenshotList(pub Vec<Screenshot>);
