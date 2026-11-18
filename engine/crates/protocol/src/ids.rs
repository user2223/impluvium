// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: 0BSD

//! Identifiers always hash the original bytes, independently of content algorithms.

use sha2::{Digest, Sha256};

pub type ManifestId = [u8; 32];
pub type KeyId = [u8; 8];

/// Hash the exact received manifest payload, including unknown fields.
pub fn manifest_id(payload: &[u8]) -> ManifestId {
    Sha256::digest(payload).into()
}

/// A key identifier selects a candidate in a trusted set; it does not confer trust.
pub fn key_id(public_key: &[u8; 32]) -> KeyId {
    let digest = Sha256::digest(public_key);
    let mut id = [0; 8];
    id.copy_from_slice(&digest[..8]);
    id
}
