// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: 0BSD

//! Raw profile checks precede projection and all general-purpose decoding.
//! Unknown fields are validated and skipped; projections contain known fields only.
//! Successful decoding establishes the encoding profile, not semantic validity or
//! authentication. Signatures and IDs continue to use the original input bytes.

use std::collections::BTreeSet;

pub use crate::envelope::{Diagnostic, Reason};
use crate::limits::{self, FieldRule, Kind, MessageKind, Occurrence};

mod binary;
mod json;
mod projection;
pub use projection::{Projection, Value};

pub(crate) type Result<T> = core::result::Result<T, Diagnostic>;
pub(crate) fn error(reason: Reason, field: &'static str) -> Diagnostic {
    Diagnostic { reason, field }
}

/// The read boundary is selected from the route or expected payload type,
/// never from an untrusted field or Content-Length header.
#[derive(Clone, Copy, Debug)]
pub struct InputLimit {
    pub bytes: usize,
    pub reason: Reason,
    pub field: &'static str,
    pub payload_bytes: usize,
}

impl InputLimit {
    pub const fn body(bytes: usize) -> Self {
        Self {
            bytes,
            reason: Reason::body_too_large,
            field: "",
            payload_bytes: limits::MANIFEST_BYTES,
        }
    }
    pub const fn response(bytes: usize) -> Self {
        Self {
            reason: Reason::response_too_large,
            ..Self::body(bytes)
        }
    }
    pub const fn payload(bytes: usize) -> Self {
        Self {
            reason: Reason::byte_limit_exceeded,
            ..Self::body(bytes)
        }
    }
    pub fn envelope<R: crate::keys::Role>(expected: crate::envelope::PayloadType<R>) -> Self {
        Self {
            bytes: expected.max_envelope(),
            reason: Reason::envelope_too_large,
            field: "",
            payload_bytes: expected.max_payload(),
        }
    }
    /// Check a received-byte counter before appending the next input block.
    /// Subtraction avoids overflow even for an adversarial declared length.
    pub fn check_add(self, received: u64, additional: u64) -> Result<()> {
        if received > self.bytes as u64 || additional > self.bytes as u64 - received {
            Err(error(self.reason, self.field))
        } else {
            Ok(())
        }
    }
    fn check(self, bytes: &[u8]) -> Result<()> {
        self.check_add(0, bytes.len() as u64)
    }
}

/// Check only the binary profile and pre-decode limits; no semantic checks.
pub fn scan_binary(message: MessageKind, bytes: &[u8], limit: InputLimit) -> Result<()> {
    limit.check(bytes)?;
    binary::scan(message, bytes, limit, false).map(|_| ())
}

/// Check only the raw JSON profile and pre-decode limits.
pub fn scan_json(message: MessageKind, bytes: &[u8], limit: InputLimit) -> Result<()> {
    limit.check(bytes)?;
    json::scan(message, bytes, limit, false).map(|_| ())
}

/// Preserve raw known values for profile inspection (including unknown enum values).
/// Allocation of the projection starts only after the complete pre-check succeeds.
pub fn project_binary(message: MessageKind, bytes: &[u8], limit: InputLimit) -> Result<Projection> {
    scan_binary(message, bytes, limit)?;
    binary::scan(message, bytes, limit, true)
}

pub fn project_json(message: MessageKind, bytes: &[u8], limit: InputLimit) -> Result<Projection> {
    scan_json(message, bytes, limit)?;
    json::scan(message, bytes, limit, true)
}

/// Runtime resource profile. The field limits have already bounded all elements.
/// 2 GiB covers the largest valid manifest's charged message/string footprints;
/// packed integers are separately bounded by the shared chunk-index counter.
/// Unknown fields never reach this decoder, so its unknown-field budget is zero.
pub fn decoder_profile() -> buffa::DecodeOptions {
    buffa::DecodeOptions::new()
        .with_recursion_limit(limits::MAX_DEPTH as u32)
        // Projection can reframe packed integers or implicit map-entry defaults.
        // This runtime ceiling is independent of the smaller raw input budgets.
        .with_max_message_size(2_147_483_647)
        .with_element_memory_limit(2_147_483_647)
        .with_unknown_field_limit(0)
}

/// Bind generated types to their explicit guard inventory.
pub trait ProtectedMessage: buffa::Message + private::Sealed {
    const KIND: MessageKind;
}
mod private {
    pub trait Sealed {}
}

/// Decode a checked projection, applying encoding.md §4's enum semantics.
pub fn decode_binary<M: ProtectedMessage>(bytes: &[u8], limit: InputLimit) -> Result<M> {
    let projection = project_binary(M::KIND, bytes, limit)?;
    decode_projection(&projection)
}

pub fn decode_json<M: ProtectedMessage>(bytes: &[u8], limit: InputLimit) -> Result<M> {
    let projection = project_json(M::KIND, bytes, limit)?;
    decode_projection(&projection)
}

pub(crate) fn decode_projection<M: ProtectedMessage>(projection: &Projection) -> Result<M> {
    let bytes = projection.to_binary();
    decoder_profile()
        .decode_from_slice(&bytes)
        .map_err(|_| error(Reason::malformed_wire, ""))
}

#[derive(Default)]
struct Work {
    chunk_indexes: usize,
}
impl Work {
    fn element(&mut self, rule: &FieldRule, count: &mut usize) -> Result<()> {
        if rule.diagnostic == "Entry.chunk_indexes" {
            if self.chunk_indexes == limits::CHUNK_INDEXES {
                return Err(error(Reason::element_limit_exceeded, ""));
            }
            self.chunk_indexes += 1;
        } else if rule.element_limit != 0 && *count == rule.element_limit {
            return Err(error(Reason::element_limit_exceeded, rule.diagnostic));
        }
        *count += 1;
        Ok(())
    }
}

fn timestamp(seconds: i64, nanos: i32, field: &'static str) -> Result<()> {
    if !(-62_135_596_800..=253_402_300_799).contains(&seconds)
        || !(0..1_000_000_000).contains(&nanos)
    {
        Err(error(Reason::invalid_timestamp, field))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests;
