// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: 0BSD

//! Role-bound signing keys, validated local trust and RFC 5869 derivation.

use core::{marker::PhantomData, num::NonZeroU32};
use ed25519_dalek::Signer;
use sha2::{Digest, Sha256};

use crate::{
    envelope::check_point,
    ids::{KeyId, key_id},
    proto::impluvium::signed::v1::KeyRole,
};

mod sealed {
    pub trait Sealed {}
}

/// A sealed signing role. Application code cannot introduce additional roles.
pub trait Role: sealed::Sealed {
    const ROLE: KeyRole;
    const MAX_KEYS: usize;
}

#[derive(Debug)]
pub enum Content {}
#[derive(Debug)]
pub enum DepotTicket {}
#[derive(Debug)]
pub enum Device {}

macro_rules! role {
    ($ty:ty, $role:ident, $max:expr) => {
        impl sealed::Sealed for $ty {}
        impl Role for $ty {
            const ROLE: KeyRole = KeyRole::$role;
            const MAX_KEYS: usize = $max;
        }
    };
}
role!(Content, Content, 1);
role!(DepotTicket, DepotTicket, 8);
role!(Device, Device, 1);

/// A private seed bound to one signing role. Debug and serialization are absent.
pub struct SigningKey<R: Role> {
    key: ed25519_dalek::SigningKey,
    role: PhantomData<R>,
}

impl<R: Role> SigningKey<R> {
    pub fn from_seed(seed: &[u8; 32]) -> Self {
        Self {
            key: ed25519_dalek::SigningKey::from_bytes(seed),
            role: PhantomData,
        }
    }

    pub fn public_key(&self) -> [u8; 32] {
        self.key.verifying_key().to_bytes()
    }

    /// Pure Ed25519 over exact message bytes; envelope callers use `sign_payload`.
    pub fn sign(&self, message: &[u8]) -> [u8; 64] {
        self.key.sign(message).to_bytes()
    }
}

/// Configuration reasons are separate from received-input diagnostics.
#[allow(non_camel_case_types)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConfigurationReason {
    trusted_set_size,
    invalid_trusted_key,
    role_mismatch,
    trusted_key_id_collision,
    invalid_ticket_key_set,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConfigurationError {
    pub reason: ConfigurationReason,
    pub field: &'static str,
}

fn config(reason: ConfigurationReason) -> ConfigurationError {
    ConfigurationError { reason, field: "" }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SwitchTime {
    pub seconds: i64,
    pub nanos: u32,
}

/// Missing switch times remain representable so configuration validation can reject them.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TicketStatus {
    Current,
    Retiring { switch_time: Option<SwitchTime> },
}

/// Unvalidated local configuration, never populated from an envelope's signatures.
#[derive(Clone, Copy, Debug)]
pub struct KeyInput<'a> {
    pub public_key: &'a [u8],
    pub role: KeyRole,
    pub ticket_status: Option<TicketStatus>,
}

#[derive(Debug)]
pub struct TrustedKey<R: Role> {
    pub(crate) public_key: [u8; 32],
    pub(crate) id: KeyId,
    pub(crate) ticket_status: Option<TicketStatus>,
    role: PhantomData<R>,
}

impl<R: Role> TrustedKey<R> {
    pub fn public_key(&self) -> &[u8; 32] {
        &self.public_key
    }
    pub fn key_id(&self) -> KeyId {
        self.id
    }
    pub fn ticket_status(&self) -> Option<TicketStatus> {
        self.ticket_status
    }
}

/// A set can only be constructed after all configuration checks, in normative order.
#[derive(Debug)]
pub struct TrustedKeySet<R: Role> {
    keys: Vec<TrustedKey<R>>,
}

impl<R: Role> TrustedKeySet<R> {
    pub fn new(inputs: &[KeyInput<'_>]) -> Result<Self, ConfigurationError> {
        use ConfigurationReason::*;
        if inputs.is_empty() || inputs.len() > R::MAX_KEYS {
            return Err(config(trusted_set_size));
        }
        // Check every point before considering any role or collision.
        for input in inputs {
            if check_point(input.public_key).is_err() {
                return Err(config(invalid_trusted_key));
            }
        }
        if inputs.iter().any(|input| input.role != R::ROLE) {
            return Err(config(role_mismatch));
        }
        let mut keys = Vec::with_capacity(inputs.len());
        for input in inputs {
            let public_key: [u8; 32] = input
                .public_key
                .try_into()
                .map_err(|_| config(invalid_trusted_key))?;
            let id = key_id(&public_key);
            keys.push(TrustedKey {
                public_key,
                id,
                ticket_status: input.ticket_status,
                role: PhantomData,
            });
        }
        check_unique_ids(&keys)?;
        if R::ROLE == KeyRole::DepotTicket {
            let current = keys
                .iter()
                .filter(|key| key.ticket_status == Some(TicketStatus::Current))
                .count();
            if current > 1
                || keys.iter().any(|key| {
                    !matches!(
                        key.ticket_status,
                        Some(
                            TicketStatus::Current
                                | TicketStatus::Retiring {
                                    switch_time: Some(_)
                                }
                        )
                    )
                })
            {
                return Err(config(invalid_ticket_key_set));
            }
        }
        Ok(Self { keys })
    }

    pub fn keys(&self) -> &[TrustedKey<R>] {
        &self.keys
    }
}

fn check_unique_ids<R: Role>(keys: &[TrustedKey<R>]) -> Result<(), ConfigurationError> {
    for (index, key) in keys.iter().enumerate() {
        if keys[..index].iter().any(|earlier| earlier.id == key.id) {
            return Err(config(ConfigurationReason::trusted_key_id_collision));
        }
    }
    Ok(())
}

// HMAC uses fixed-size SHA-256 blocks and streams all message fragments. Neither
// the key length nor the requested HKDF output allocates memory.
fn hmac_sha256(key: &[u8], parts: &[&[u8]]) -> [u8; 32] {
    let mut block = [0u8; 64];
    if key.len() > block.len() {
        block[..32].copy_from_slice(&Sha256::digest(key));
    } else {
        block[..key.len()].copy_from_slice(key);
    }
    let mut inner = Sha256::new();
    for byte in &mut block {
        *byte ^= 0x36;
    }
    inner.update(block);
    for part in parts {
        inner.update(part);
    }
    let digest = inner.finalize();
    for byte in &mut block {
        *byte ^= 0x36 ^ 0x5c;
    }
    let mut outer = Sha256::new();
    outer.update(block);
    outer.update(digest);
    outer.finalize().into()
}

pub fn hkdf_sha256_extract(salt: &[u8], ikm: &[u8]) -> [u8; 32] {
    hmac_sha256(salt, &[ikm])
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HkdfOutputTooLong;

/// RFC 5869 expand, at most 255 SHA-256 blocks; output storage belongs to the caller.
pub fn hkdf_sha256_expand(
    prk: &[u8; 32],
    info: &[u8],
    output: &mut [u8],
) -> Result<(), HkdfOutputTooLong> {
    if output.len() > 255 * 32 {
        return Err(HkdfOutputTooLong);
    }
    let mut previous = [0u8; 32];
    let mut previous_len = 0;
    for (index, chunk) in output.chunks_mut(32).enumerate() {
        previous = hmac_sha256(
            prk,
            &[&previous[..previous_len], info, &[(index + 1) as u8]],
        );
        chunk.copy_from_slice(&previous[..chunk.len()]);
        previous_len = 32;
    }
    Ok(())
}

/// The fixed 32-byte derivation used by every server signing label.
fn derive_seed(instance_key: &[u8; 32], label: &[u8]) -> [u8; 32] {
    let prk = hkdf_sha256_extract(b"", instance_key);
    hmac_sha256(&prk, &[label, &[1]])
}

pub fn derive_content_key(instance_key: &[u8; 32]) -> SigningKey<Content> {
    SigningKey::from_seed(&derive_seed(
        instance_key,
        b"impluvium/content-signing-key/v1",
    ))
}

pub fn derive_ticket_key(
    instance_key: &[u8; 32],
    generation: NonZeroU32,
) -> SigningKey<DepotTicket> {
    let label = format!("impluvium/depot-ticket-signing-key/v1/{generation}");
    SigningKey::from_seed(&derive_seed(instance_key, label.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distinct_public_keys_with_colliding_ids_are_never_selected_arbitrarily() {
        let first = SigningKey::<DepotTicket>::from_seed(&[21; 32]).public_key();
        let second = SigningKey::<DepotTicket>::from_seed(&[22; 32]).public_key();
        assert_ne!(first, second);
        // Inject only at the private uniqueness predicate: producing a real
        // 64-bit SHA-256 prefix collision is not a practical unit-test setup.
        let keys: [TrustedKey<DepotTicket>; 2] = [
            TrustedKey {
                public_key: first,
                id: [3; 8],
                ticket_status: None,
                role: PhantomData,
            },
            TrustedKey {
                public_key: second,
                id: [3; 8],
                ticket_status: None,
                role: PhantomData,
            },
        ];
        assert_eq!(
            check_unique_ids(&keys),
            Err(config(ConfigurationReason::trusted_key_id_collision))
        );
    }
}
