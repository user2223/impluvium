// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Public metadata of the signing keys derived from an instance key.

use core::fmt;
use core::num::NonZeroU32;

fn write_hex(f: &mut fmt::Formatter<'_>, bytes: &[u8]) -> fmt::Result {
    for byte in bytes {
        write!(f, "{byte:02x}")?;
    }
    Ok(())
}

/// A raw Ed25519 public key (32 bytes). Displayed as lowercase hexadecimal.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct PublicKey([u8; 32]);

impl PublicKey {
    pub const fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Display for PublicKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write_hex(f, &self.0)
    }
}

/// The short identifier (8 bytes) of a public key. Displayed as lowercase hexadecimal.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct KeyId([u8; 8]);

impl KeyId {
    pub const fn new(bytes: [u8; 8]) -> Self {
        Self(bytes)
    }

    pub const fn as_bytes(&self) -> &[u8; 8] {
        &self.0
    }
}

impl fmt::Display for KeyId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write_hex(f, &self.0)
    }
}

/// Generation number of a depot ticket signing key. Generations start at 1.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct TicketGeneration(NonZeroU32);

impl TicketGeneration {
    /// Returns `None` for zero, which is not a valid generation.
    pub const fn new(generation: u32) -> Option<Self> {
        match NonZeroU32::new(generation) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }

    pub const fn get(self) -> NonZeroU32 {
        self.0
    }
}

impl fmt::Display for TicketGeneration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// What a derived signing key is used for.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum KeyPurpose {
    /// Signs manifests and release descriptors. There is exactly one.
    Content,
    /// Signs depot tickets. One key exists per generation.
    DepotTicket(TicketGeneration),
}

/// Public description of one derived signing key.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct SigningKeyMetadata {
    pub purpose: KeyPurpose,
    pub public_key: PublicKey,
    pub key_id: KeyId,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifiers_display_as_lowercase_hexadecimal() {
        assert_eq!(
            KeyId::new([0, 1, 0xab, 0xcd, 0xef, 0x10, 0x20, 0xff]).to_string(),
            "0001abcdef1020ff"
        );
        let mut bytes = [0u8; 32];
        bytes[0] = 0x0a;
        bytes[31] = 0xf0;
        let shown = PublicKey::new(bytes).to_string();
        assert_eq!(shown.len(), 64);
        assert!(shown.starts_with("0a") && shown.ends_with("f0"));
    }

    #[test]
    fn ticket_generation_zero_is_rejected() {
        assert!(TicketGeneration::new(0).is_none());
        let generation = TicketGeneration::new(u32::MAX).expect("maximum generation");
        assert_eq!(generation.to_string(), "4294967295");
        assert_eq!(generation.get().get(), u32::MAX);
    }
}
