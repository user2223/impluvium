// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: 0BSD

//! User input normalization and the exact proofs.md §3.2 alphabet.

use crate::decode::{Diagnostic, Reason, error};
use subtle::ConstantTimeEq;

pub const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// Only ASCII space and hyphen are removed; only ASCII letters change case.
/// Normalization alone does not establish that the result is a valid code.
pub fn normalize(input: &str) -> String {
    input
        .chars()
        .filter(|&c| c != '-' && c != ' ')
        .map(|c| c.to_ascii_uppercase())
        .collect()
}

/// A validated eight-character code, deliberately without Debug or ordinary Eq.
#[derive(Clone)]
pub struct UserCode([u8; 8]);
impl UserCode {
    pub fn parse(input: &str) -> Result<Self, Diagnostic> {
        Self::from_normalized(&normalize(input))
    }
    pub fn from_normalized(input: &str) -> Result<Self, Diagnostic> {
        let bytes: [u8; 8] = input
            .as_bytes()
            .try_into()
            .map_err(|_| error(Reason::invalid_user_code, ""))?;
        if !bytes.iter().all(|b| ALPHABET.contains(b)) {
            return Err(error(Reason::invalid_user_code, ""));
        }
        Ok(Self(bytes))
    }
    /// Inject eight independent uniform random bytes from the caller's CSPRNG.
    /// Masking to five bits samples the 32-character alphabet without bias.
    /// Uniqueness among open pairings must be enforced by the caller.
    pub fn from_random(random: &[u8; 8]) -> Self {
        Self(random.map(|b| ALPHABET[usize::from(b & 31)]))
    }
    /// Explicit disclosure for display and wire output; never used by Debug.
    pub fn grouped(&self) -> String {
        let mut out = String::with_capacity(9);
        for (i, byte) in self.0.iter().enumerate() {
            if i == 4 {
                out.push('-');
            }
            out.push(char::from(*byte));
        }
        out
    }
    pub fn matches(&self, other: &Self) -> bool {
        bool::from(self.0.ct_eq(&other.0))
    }
}
