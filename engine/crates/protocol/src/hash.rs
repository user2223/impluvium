// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: 0BSD

//! Manifest content hashes, independent of the SHA-256 manifest identifier.

use crate::decode::{Diagnostic, Reason, error};
use sha2::{Digest, Sha256};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Algorithm {
    Sha256,
    Blake3,
}

impl Algorithm {
    pub fn from_wire(value: i32) -> Result<Self, Diagnostic> {
        match value {
            0 => Err(error(Reason::missing_field, "Header.hash_algorithm")),
            1 => Ok(Self::Sha256),
            2 => Ok(Self::Blake3),
            _ => Err(error(
                Reason::unsupported_hash_algorithm,
                "Header.hash_algorithm",
            )),
        }
    }

    pub fn digest(self, bytes: &[u8]) -> [u8; 32] {
        match self {
            Self::Sha256 => Sha256::digest(bytes).into(),
            Self::Blake3 => *blake3::hash(bytes).as_bytes(),
        }
    }

    pub fn hasher(self) -> ContentHasher {
        match self {
            Self::Sha256 => ContentHasher::Sha256(Sha256::new()),
            Self::Blake3 => ContentHasher::Blake3(Box::new(blake3::Hasher::new())),
        }
    }
}

/// Incremental hashing also permits checking reconstructed files without buffering them.
pub enum ContentHasher {
    Sha256(Sha256),
    Blake3(Box<blake3::Hasher>),
}

impl ContentHasher {
    pub fn update(&mut self, bytes: &[u8]) {
        match self {
            Self::Sha256(state) => state.update(bytes),
            Self::Blake3(state) => {
                state.update(bytes);
            }
        }
    }

    pub fn finish(self) -> [u8; 32] {
        match self {
            Self::Sha256(state) => state.finalize().into(),
            Self::Blake3(state) => *state.finalize().as_bytes(),
        }
    }
}
