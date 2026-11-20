// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Object references and non-JSON body descriptors. The body types describe raw HTTP bytes;
//! handlers must never send them through a JSON extractor or JSON response wrapper.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// An image or file reference carried by catalog and screenshot records, not file contents.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ObjectReference(pub String);

/// The object id in a path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ObjectParams {
    /// The object reference, validated before storage access.
    pub id: String,
}

impl ObjectParams {
    /// Refuses unsafe filename components. Object ids are opaque and are
    /// not required to be UUIDs (profile pictures and imported images also use these routes).
    pub fn is_valid(&self) -> bool {
        let id = &self.id;
        let stem = id
            .split('.')
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase();
        let reserved = matches!(stem.as_str(), "con" | "prn" | "aux" | "nul")
            || ["com", "lpt"].iter().any(|prefix| {
                stem.strip_prefix(*prefix).is_some_and(|suffix| {
                    suffix.len() == 1 && suffix.bytes().all(|byte| byte.is_ascii_digit())
                })
            });
        !reserved
            && !id.is_empty()
            && id.len() <= 255
            && id != "."
            && id != ".."
            && !id.ends_with(['.', ' '])
            && !id
                .chars()
                .any(|c| c.is_control() || r#"/\<>:"|?*"#.contains(c))
    }
}

/// Raw bytes accepted by both upload routes, never a multipart form or JSON document.
#[derive(Clone, PartialEq, Eq, TS)]
#[ts(type = "Blob | ArrayBuffer | Uint8Array<ArrayBuffer>")]
pub struct BinaryUpload {
    /// The bounded request bytes. Never log them.
    pub bytes: Vec<u8>,
}

redacted_debug!(BinaryUpload, shown: [], hidden: [bytes]);

/// A raw object body on GET 200; no body on 304 or HEAD. This is a response descriptor,
/// not a JSON envelope; content type comes from object metadata.
#[derive(Debug, Clone, PartialEq, Eq, TS)]
#[ts(type = "Blob | undefined")]
pub struct ObjectDownload;

/// The result of an object write or delete, including denied/missing objects.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ObjectMutation {
    /// Whether the operation succeeded under the stored object permissions.
    pub success: bool,
}
