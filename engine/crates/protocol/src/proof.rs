// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: 0BSD

//! Device proofs bound to caller-supplied context. Neither proof uses a clock.
//! The function selected by the caller fixes the expected PAE purpose.

use buffa::Message;
use subtle::ConstantTimeEq;

use crate::{
    decode::{self, Diagnostic, InputLimit, Reason, error},
    envelope::{self, PayloadType},
    keys::{Device, SigningKey, TrustedKeySet},
    manifest::uuid,
    proto::impluvium::{
        signed::v1::Envelope,
        v2::{PairingProof, SessionProof},
    },
};

pub mod pairing;
pub mod session;
pub mod user_code;

pub const MAX_ENVELOPE_BYTES: usize = 4096;

/// Expected session bindings from the request and local instance configuration.
/// Debug is deliberately absent because the challenge is secret.
#[derive(Clone, Copy)]
pub struct SessionContext<'a> {
    pub instance_id: &'a str,
    pub device_id: &'a str,
    pub challenge: &'a [u8; 32],
}

/// Expected pairing bindings from the pending pairing and local configuration.
#[derive(Clone, Copy)]
pub struct PairingContext<'a> {
    pub instance_id: &'a str,
    pub pairing_id: &'a [u8; 16],
}

/// An authenticated and context-bound session proof; secret fields are not Debug.
pub struct VerifiedSession(SessionProof);
impl VerifiedSession {
    pub fn instance_id(&self) -> &str {
        &self.0.instance_id
    }
    pub fn device_id(&self) -> &str {
        &self.0.device_id
    }
}

/// An authenticated and context-bound pairing proof.
pub struct VerifiedPairing(PairingProof);
impl VerifiedPairing {
    pub fn instance_id(&self) -> &str {
        &self.0.instance_id
    }
}

fn session_fields(p: &SessionProof, c: &SessionContext<'_>) -> Result<(), Diagnostic> {
    for (value, field) in [
        (p.instance_id.as_str(), "SessionProof.instance_id"),
        (p.device_id.as_str(), "SessionProof.device_id"),
    ] {
        if !uuid(value) {
            return Err(error(Reason::invalid_uuid, field));
        }
    }
    if p.challenge.len() != 32 {
        return Err(error(
            Reason::invalid_challenge_length,
            "SessionProof.challenge",
        ));
    }
    if p.instance_id != c.instance_id {
        return Err(error(
            Reason::proof_instance_mismatch,
            "SessionProof.instance_id",
        ));
    }
    if p.device_id != c.device_id {
        return Err(error(
            Reason::proof_device_mismatch,
            "SessionProof.device_id",
        ));
    }
    if !bool::from(p.challenge.as_slice().ct_eq(c.challenge.as_slice())) {
        return Err(error(
            Reason::proof_challenge_mismatch,
            "SessionProof.challenge",
        ));
    }
    Ok(())
}
fn pairing_fields(p: &PairingProof, c: &PairingContext<'_>) -> Result<(), Diagnostic> {
    if !uuid(&p.instance_id) {
        return Err(error(Reason::invalid_uuid, "PairingProof.instance_id"));
    }
    if p.pairing_id.len() != 16 {
        return Err(error(
            Reason::invalid_pairing_id_length,
            "PairingProof.pairing_id",
        ));
    }
    if p.instance_id != c.instance_id {
        return Err(error(
            Reason::proof_instance_mismatch,
            "PairingProof.instance_id",
        ));
    }
    if !bool::from(p.pairing_id.as_slice().ct_eq(c.pairing_id.as_slice())) {
        return Err(error(
            Reason::proof_pairing_mismatch,
            "PairingProof.pairing_id",
        ));
    }
    Ok(())
}

/// Envelope checks, bounded payload scan, field rules, then instance/device/challenge.
/// `trusted` contains only the key registered for the expected device.
pub fn verify_session(
    bytes: &[u8],
    trusted: &TrustedKeySet<Device>,
    context: &SessionContext<'_>,
) -> Result<VerifiedSession, Diagnostic> {
    let authenticated = envelope::authenticate(bytes, envelope::SESSION_PROOF, trusted)?;
    let proof = decode::decode_binary(
        authenticated.payload(),
        InputLimit::payload(MAX_ENVELOPE_BYTES),
    )?;
    session_fields(&proof, context)?;
    Ok(VerifiedSession(proof))
}

/// Envelope checks, bounded payload scan, field rules, then instance/pairing.
/// `trusted` contains only the pending pairing's public key.
pub fn verify_pairing(
    bytes: &[u8],
    trusted: &TrustedKeySet<Device>,
    context: &PairingContext<'_>,
) -> Result<VerifiedPairing, Diagnostic> {
    let authenticated = envelope::authenticate(bytes, envelope::PAIRING_PROOF, trusted)?;
    let proof = decode::decode_binary(
        authenticated.payload(),
        InputLimit::payload(MAX_ENVELOPE_BYTES),
    )?;
    pairing_fields(&proof, context)?;
    Ok(VerifiedPairing(proof))
}

fn sign(
    payload: Vec<u8>,
    purpose: PayloadType<Device>,
    key: &SigningKey<Device>,
) -> Result<Vec<u8>, Diagnostic> {
    let signature = envelope::sign_payload(purpose, &payload, key)?;
    let bytes = Envelope {
        payload_type: purpose.as_str().into(),
        payload,
        signatures: vec![signature],
        ..Default::default()
    }
    .encode_to_vec();
    if bytes.len() > MAX_ENVELOPE_BYTES {
        return Err(error(Reason::envelope_too_large, ""));
    }
    Ok(bytes)
}

/// Produce only fields defined by SessionProof, using the device's local key.
pub fn issue_session(
    context: &SessionContext<'_>,
    key: &SigningKey<Device>,
) -> Result<Vec<u8>, Diagnostic> {
    let proof = SessionProof {
        instance_id: context.instance_id.into(),
        device_id: context.device_id.into(),
        challenge: context.challenge.to_vec(),
        ..Default::default()
    };
    session_fields(&proof, context)?;
    sign(proof.encode_to_vec(), envelope::SESSION_PROOF, key)
}

/// Produce only fields defined by PairingProof, using the pending device's key.
pub fn issue_pairing(
    context: &PairingContext<'_>,
    key: &SigningKey<Device>,
) -> Result<Vec<u8>, Diagnostic> {
    let proof = PairingProof {
        instance_id: context.instance_id.into(),
        pairing_id: context.pairing_id.to_vec(),
        ..Default::default()
    };
    pairing_fields(&proof, context)?;
    sign(proof.encode_to_vec(), envelope::PAIRING_PROOF, key)
}
