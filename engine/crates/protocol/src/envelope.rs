// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: 0BSD

//! DSSE and signature authentication through envelope.md §7 step 10.
//!
//! A successful result authenticates the exact payload bytes. The caller must
//! still apply the payload's binary profile, time and semantic checks (steps
//! 11–13) before accepting the document. No payload is decoded here.

use core::marker::PhantomData;
use curve25519_dalek::edwards::CompressedEdwardsY;

use crate::{
    keys::{Content, DepotTicket, Device, Role, SigningKey, TrustedKey, TrustedKeySet},
    proto::impluvium::signed::v1::Signature,
};

pub const MAX_SIGNATURES: usize = 8;
pub const MAX_METADATA: usize = 16_384;
pub const MAX_PAYLOAD_TYPE: usize = 128;
pub const MAX_MANIFEST_PAYLOAD: usize = 536_870_912;

/// Local input reasons retain the exact specification spelling.
#[allow(non_camel_case_types)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Reason {
    malformed_wire,
    wrong_wire_type,
    split_message,
    oneof_conflict,
    duplicate_map_key,
    invalid_json,
    duplicate_json_key,
    json_type_mismatch,
    nesting_too_deep,
    invalid_number,
    integer_out_of_range,
    invalid_base64,
    invalid_timestamp,
    body_too_large,
    response_too_large,
    invalid_utf8,
    duplicate_field,
    element_limit_exceeded,
    byte_limit_exceeded,
    envelope_too_large,
    no_signatures,
    malformed_signature_entry,
    invalid_payload_type,
    duplicate_signature_key_id,
    unknown_payload_type,
    unexpected_payload_type,
    invalid_signature,
    no_trusted_signature,
    missing_field,
    unsupported_manifest_format,
    invalid_uuid,
    missing_header,
    unsupported_hash_algorithm,
    missing_chunking,
    invalid_name,
    unsorted_required_features,
    unknown_required_feature,
    invalid_chunk_id,
    chunk_size_zero,
    chunk_size_too_large,
    unsorted_chunks,
    unsorted_entries,
    path_parent_conflict,
    invalid_entry_type,
    invalid_entry_fields,
    invalid_hash_length,
    invalid_mode,
    empty_file_hash_mismatch,
    chunk_index_out_of_range,
    size_overflow,
    chunk_size_mismatch,
    single_chunk_hash_mismatch,
    total_size_mismatch,
    unreferenced_chunk,
    path_empty,
    path_forbidden_character,
    path_absolute,
    path_drive_prefix,
    path_empty_component,
    path_dot_component,
    path_component_too_long,
    symlink_escape,
    windows_forbidden_character,
    windows_trailing_dot_or_space,
    windows_reserved_name,
    windows_case_collision,
    windows_symlink,
    unsupported_symlink,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Diagnostic {
    pub reason: Reason,
    pub field: &'static str,
}

fn error(reason: Reason, field: &'static str) -> Diagnostic {
    Diagnostic { reason, field }
}

/// A registered expected payload type, bound to its signing role.
#[derive(Debug)]
pub struct PayloadType<R: Role> {
    name: &'static str,
    max_payload: usize,
    max_envelope: usize,
    role: PhantomData<R>,
}

impl<R: Role> Copy for PayloadType<R> {}
impl<R: Role> Clone for PayloadType<R> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<R: Role> PayloadType<R> {
    pub const fn as_str(self) -> &'static str {
        self.name
    }
    pub const fn max_payload(self) -> usize {
        self.max_payload
    }
    pub const fn max_envelope(self) -> usize {
        self.max_envelope
    }
}

pub const MANIFEST: PayloadType<Content> = PayloadType {
    name: "application/vnd.impluvium.manifest.v3+protobuf",
    max_payload: MAX_MANIFEST_PAYLOAD,
    max_envelope: MAX_MANIFEST_PAYLOAD + MAX_METADATA,
    role: PhantomData,
};
pub const RELEASE: PayloadType<Content> = PayloadType {
    name: "application/vnd.impluvium.release.v1+protobuf",
    max_payload: 1_048_576,
    max_envelope: 1_048_576 + MAX_METADATA,
    role: PhantomData,
};
pub const DEPOT_TICKET: PayloadType<DepotTicket> = PayloadType {
    name: "application/vnd.impluvium.depot-ticket.v1+protobuf",
    max_payload: 4_096,
    max_envelope: 4_096,
    role: PhantomData,
};
pub const SESSION_PROOF: PayloadType<Device> = PayloadType {
    name: "application/vnd.impluvium.session-proof.v1+protobuf",
    max_payload: 4_096,
    max_envelope: 4_096,
    role: PhantomData,
};
pub const PAIRING_PROOF: PayloadType<Device> = PayloadType {
    name: "application/vnd.impluvium.pairing-proof.v1+protobuf",
    max_payload: 4_096,
    max_envelope: 4_096,
    role: PhantomData,
};

/// Raw DSSE byte primitive: empty and non-ASCII types are meaningful here.
/// Protocol text syntax is enforced separately by `authenticate`.
pub fn pae(payload_type: &[u8], payload: &[u8]) -> Result<Vec<u8>, Diagnostic> {
    if payload_type.len() > MAX_PAYLOAD_TYPE {
        return Err(error(Reason::byte_limit_exceeded, "Envelope.payload_type"));
    }
    if payload.len() > MAX_MANIFEST_PAYLOAD {
        return Err(error(Reason::byte_limit_exceeded, "Envelope.payload"));
    }
    // Decimal lengths have at most 3 and 9 digits under these fixed bounds.
    let type_len = payload_type.len().to_string();
    let body_len = payload.len().to_string();
    let mut message = Vec::with_capacity(
        10 + type_len.len() + payload_type.len() + body_len.len() + payload.len(),
    );
    message.extend_from_slice(b"DSSEv1 ");
    message.extend_from_slice(type_len.as_bytes());
    message.push(b' ');
    message.extend_from_slice(payload_type);
    message.push(b' ');
    message.extend_from_slice(body_len.as_bytes());
    message.push(b' ');
    message.extend_from_slice(payload);
    Ok(message)
}

/// Sign exact payload bytes with the role required by the registered type.
/// Producers must validate payload semantics before calling this primitive.
///
/// ```compile_fail
/// use impluvium_protocol::{envelope, keys::{Device, SigningKey}};
/// let key = SigningKey::<Device>::from_seed(&[0; 32]);
/// envelope::sign_payload(envelope::MANIFEST, b"payload", &key);
/// ```
pub fn sign_payload<R: Role>(
    expected: PayloadType<R>,
    payload: &[u8],
    key: &SigningKey<R>,
) -> Result<Signature, Diagnostic> {
    if payload.len() > expected.max_payload {
        return Err(error(Reason::byte_limit_exceeded, "Envelope.payload"));
    }
    let message = pae(expected.name.as_bytes(), payload)?;
    Ok(Signature {
        key_id: crate::ids::key_id(&key.public_key()).to_vec(),
        sig: key.sign(&message).to_vec(),
        ..Default::default()
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PointError {
    Length,
    CanonicalY,
    NotOnCurve,
    NegativeZero,
    SmallOrder,
}

impl PointError {
    pub const fn failed_check(self) -> &'static str {
        match self {
            Self::Length => "length",
            Self::CanonicalY => "canonical-y",
            Self::NotOnCurve => "not-on-curve",
            Self::NegativeZero => "negative-zero",
            Self::SmallOrder => "small-order",
        }
    }
}

// Integers in the profile are unsigned little-endian encodings.
fn less_than(a: &[u8; 32], b: &[u8; 32]) -> bool {
    a.iter().rev().cmp(b.iter().rev()).is_lt()
}

/// Explicit point checks in envelope.md §4.2 order, including negative zero.
/// Mixed-order points are allowed when they are not themselves of small order.
pub fn check_point(bytes: &[u8]) -> Result<(), PointError> {
    let encoded: [u8; 32] = bytes.try_into().map_err(|_| PointError::Length)?;
    let mut y = encoded;
    y[31] &= 0x7f;
    let mut p = [0xff; 32];
    p[0] = 0xed;
    p[31] = 0x7f;
    if !less_than(&y, &p) {
        return Err(PointError::CanonicalY);
    }
    let point = CompressedEdwardsY(encoded)
        .decompress()
        .ok_or(PointError::NotOnCurve)?;
    // With y already canonical, re-encoding can differ only for a sign bit on
    // x = 0. The library's permissive decompression alone does not reject it.
    if point.compress().to_bytes() != encoded {
        return Err(PointError::NegativeZero);
    }
    if point.is_small_order() {
        return Err(PointError::SmallOrder);
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Ed25519Error {
    SignatureLength,
    PublicKey(PointError),
    R(PointError),
    SBound,
    Equation,
}

impl Ed25519Error {
    pub const fn failed_check(self) -> &'static str {
        use PointError::*;
        match self {
            Self::SignatureLength => "signature-length",
            Self::PublicKey(Length) => "A-length",
            Self::PublicKey(CanonicalY) => "A-canonical-y",
            Self::PublicKey(NotOnCurve) => "A-not-on-curve",
            Self::PublicKey(NegativeZero) => "A-negative-zero",
            Self::PublicKey(SmallOrder) => "A-small-order",
            Self::R(Length) => "R-length",
            Self::R(CanonicalY) => "R-canonical-y",
            Self::R(NotOnCurve) => "R-not-on-curve",
            Self::R(NegativeZero) => "R-negative-zero",
            Self::R(SmallOrder) => "R-small-order",
            Self::SBound => "S-bound",
            Self::Equation => "cofactorless-equation",
        }
    }
}

/// Encoding and point checks only; this does not establish a valid signature.
pub fn check_ed25519_encoding(public_key: &[u8], signature: &[u8]) -> Result<(), Ed25519Error> {
    if signature.len() != 64 {
        return Err(Ed25519Error::SignatureLength);
    }
    check_point(public_key).map_err(Ed25519Error::PublicKey)?;
    check_point(&signature[..32]).map_err(Ed25519Error::R)?;
    let s: [u8; 32] = signature[32..]
        .try_into()
        .map_err(|_| Ed25519Error::SignatureLength)?;
    const L: [u8; 32] = [
        0xed, 0xd3, 0xf5, 0x5c, 0x1a, 0x63, 0x12, 0x58, 0xd6, 0x9c, 0xf7, 0xa2, 0xde, 0xf9, 0xde,
        0x14, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x10,
    ];
    if !less_than(&s, &L) {
        return Err(Ed25519Error::SBound);
    }
    Ok(())
}

pub fn verify_ed25519(
    public_key: &[u8],
    message: &[u8],
    signature: &[u8],
) -> Result<(), Ed25519Error> {
    // These own checks MUST precede the library's mathematical verification.
    check_ed25519_encoding(public_key, signature)?;
    let a: &[u8; 32] = public_key
        .try_into()
        .map_err(|_| Ed25519Error::PublicKey(PointError::Length))?;
    let key = ed25519_dalek::VerifyingKey::from_bytes(a)
        .map_err(|_| Ed25519Error::PublicKey(PointError::NotOnCurve))?;
    let sig = ed25519_dalek::Signature::from_slice(signature)
        .map_err(|_| Ed25519Error::SignatureLength)?;
    key.verify_strict(message, &sig)
        .map_err(|_| Ed25519Error::Equation)
}

#[derive(Clone, Copy, Default)]
struct SignatureView<'a> {
    id: &'a [u8],
    sig: &'a [u8],
}

/// Exact bytes authenticated under local trust; payload validity is still pending.
#[derive(Debug)]
pub struct AuthenticatedPayload<'bytes, 'trust, R: Role> {
    payload: &'bytes [u8],
    valid_keys: Vec<&'trust TrustedKey<R>>,
}

impl<'bytes, 'trust, R: Role> AuthenticatedPayload<'bytes, 'trust, R> {
    pub fn payload(&self) -> &'bytes [u8] {
        self.payload
    }
    /// Every valid known signature, in envelope order, for later ticket checks.
    pub fn valid_keys(&self) -> &[&'trust TrustedKey<R>] {
        &self.valid_keys
    }
}

/// Authenticate an envelope using a previously validated, role-bound trust set.
/// Configuration is therefore checked before this function can read any input.
/// Unknown fields are skipped, and payload bytes borrow the original input.
///
/// ```compile_fail
/// use impluvium_protocol::{envelope, keys::{Device, TrustedKeySet}};
/// fn wrong_role(trusted: &TrustedKeySet<Device>) {
///     envelope::authenticate(b"", envelope::MANIFEST, trusted);
/// }
/// ```
///
/// ```compile_fail
/// use impluvium_protocol::{envelope, proto::impluvium::signed::v1::Signature};
/// let signatures: Vec<Signature> = Vec::new();
/// envelope::authenticate(b"", envelope::SESSION_PROOF, &signatures);
/// ```
pub fn authenticate<'bytes, 'trust, R: Role>(
    bytes: &'bytes [u8],
    expected: PayloadType<R>,
    trusted: &'trust TrustedKeySet<R>,
) -> Result<AuthenticatedPayload<'bytes, 'trust, R>, Diagnostic> {
    use Reason::*;
    if bytes.len() > expected.max_envelope {
        return Err(error(envelope_too_large, ""));
    }
    crate::decode::scan_binary(
        crate::limits::MessageKind::Envelope,
        bytes,
        crate::decode::InputLimit::envelope(expected),
    )?;
    let mut cursor = Cursor::new(bytes, false);
    let mut payload_type = &b""[..];
    let mut payload = &b""[..];
    let mut seen = [false; 2];
    let mut signatures = [SignatureView::default(); MAX_SIGNATURES];
    let mut count = 0;
    let mut signature_budget = 0;
    while let Some(field) = cursor.next()? {
        match field.number {
            1 | 2 => {
                let index = field.number as usize - 1;
                if seen[index] {
                    return Err(error(duplicate_field, field.name));
                }
                seen[index] = true;
                let limit = if index == 0 {
                    MAX_PAYLOAD_TYPE
                } else {
                    expected.max_payload
                };
                if field.value.len() > limit {
                    return Err(error(byte_limit_exceeded, field.name));
                }
                if index == 0 {
                    core::str::from_utf8(field.value)
                        .map_err(|_| error(invalid_utf8, field.name))?;
                    payload_type = field.value;
                } else {
                    payload = field.value;
                }
            }
            3 => {
                if count == MAX_SIGNATURES {
                    return Err(error(element_limit_exceeded, field.name));
                }
                signature_budget += field.encoded_len;
                if signature_budget > MAX_METADATA {
                    return Err(error(byte_limit_exceeded, field.name));
                }
                signatures[count] = scan_signature(field.value)?;
                count += 1;
            }
            _ => {}
        }
    }
    if bytes.len() - payload.len() > MAX_METADATA {
        return Err(error(byte_limit_exceeded, ""));
    }
    if count == 0 {
        return Err(error(no_signatures, "Envelope.signatures"));
    }
    let signatures = &signatures[..count];
    for signature in signatures {
        if signature.id.len() != 8 || signature.sig.len() != 64 {
            return Err(error(malformed_signature_entry, "Envelope.signatures"));
        }
    }
    if payload_type.is_empty() || !payload_type.iter().all(|b| (0x21..=0x7e).contains(b)) {
        return Err(error(invalid_payload_type, "Envelope.payload_type"));
    }
    for (index, signature) in signatures.iter().enumerate() {
        if signatures[..index]
            .iter()
            .any(|earlier| earlier.id == signature.id)
        {
            return Err(error(duplicate_signature_key_id, "Envelope.signatures"));
        }
    }
    if ![
        MANIFEST.name,
        RELEASE.name,
        DEPOT_TICKET.name,
        SESSION_PROOF.name,
        PAIRING_PROOF.name,
    ]
    .iter()
    .any(|name| name.as_bytes() == payload_type)
    {
        return Err(error(unknown_payload_type, "Envelope.payload_type"));
    }
    if payload_type != expected.name.as_bytes() {
        return Err(error(unexpected_payload_type, "Envelope.payload_type"));
    }
    let message = pae(payload_type, payload)?;
    let mut valid_keys = Vec::with_capacity(trusted.keys().len());
    for signature in signatures {
        if let Some(key) = trusted
            .keys()
            .iter()
            .find(|key| key.id.as_slice() == signature.id)
        {
            verify_ed25519(&key.public_key, &message, signature.sig)
                .map_err(|_| error(invalid_signature, "Signature.sig"))?;
            valid_keys.push(key);
        }
    }
    if valid_keys.is_empty() {
        return Err(error(no_trusted_signature, ""));
    }
    Ok(AuthenticatedPayload {
        payload,
        valid_keys,
    })
}

fn scan_signature(bytes: &[u8]) -> Result<SignatureView<'_>, Diagnostic> {
    let mut cursor = Cursor::new(bytes, true);
    let mut values = [&b""[..]; 2];
    let mut seen = [false; 2];
    while let Some(field) = cursor.next()? {
        if matches!(field.number, 1 | 2) {
            let index = field.number as usize - 1;
            if seen[index] {
                return Err(error(Reason::duplicate_field, field.name));
            }
            seen[index] = true;
            // Fixed-size values have a scan limit of 128, then a semantic length check.
            if field.value.len() > 128 {
                return Err(error(Reason::byte_limit_exceeded, field.name));
            }
            values[index] = field.value;
        }
    }
    Ok(SignatureView {
        id: values[0],
        sig: values[1],
    })
}

struct Field<'a> {
    number: u64,
    name: &'static str,
    value: &'a [u8],
    encoded_len: usize,
}

/// The bounded envelope/Signature subset of the binary profile. It borrows all
/// LEN values and never stores unknown fields or calls a merging decoder.
struct Cursor<'a> {
    bytes: &'a [u8],
    position: usize,
    signature: bool,
}

impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8], signature: bool) -> Self {
        Self {
            bytes,
            position: 0,
            signature,
        }
    }

    fn varint(&mut self, field: &'static str) -> Result<u64, Diagnostic> {
        let mut value = 0;
        for index in 0..10 {
            let byte = *self
                .bytes
                .get(self.position)
                .ok_or(error(Reason::malformed_wire, field))?;
            self.position += 1;
            if index == 9 && byte > 1 {
                return Err(error(Reason::malformed_wire, field));
            }
            value |= u64::from(byte & 0x7f) << (7 * index);
            if byte & 0x80 == 0 {
                return Ok(value);
            }
        }
        Err(error(Reason::malformed_wire, field))
    }

    fn take(&mut self, length: u64, field: &'static str) -> Result<&'a [u8], Diagnostic> {
        if length > (self.bytes.len() - self.position) as u64 {
            return Err(error(Reason::malformed_wire, field));
        }
        let end = self.position + length as usize;
        let value = &self.bytes[self.position..end];
        self.position = end;
        Ok(value)
    }

    fn next(&mut self) -> Result<Option<Field<'a>>, Diagnostic> {
        if self.position == self.bytes.len() {
            return Ok(None);
        }
        let start = self.position;
        let tag = self.varint("")?;
        let number = tag >> 3;
        if number == 0 || number > 536_870_911 {
            return Err(error(Reason::malformed_wire, ""));
        }
        let name = match (self.signature, number) {
            (false, 1) => "Envelope.payload_type",
            (false, 2) => "Envelope.payload",
            (false, 3) => "Envelope.signatures",
            (true, 1) => "Signature.key_id",
            (true, 2) => "Signature.sig",
            _ => "",
        };
        let wire_type = tag & 7;
        if !matches!(wire_type, 0 | 1 | 2 | 5) {
            return Err(error(Reason::malformed_wire, name));
        }
        if !name.is_empty() && wire_type != 2 {
            return Err(error(Reason::wrong_wire_type, name));
        }
        let value = match wire_type {
            0 => {
                self.varint(name)?;
                &b""[..]
            }
            1 => self.take(8, name)?,
            2 => {
                let length = self.varint(name)?;
                self.take(length, name)?
            }
            5 => self.take(4, name)?,
            _ => return Err(error(Reason::malformed_wire, name)),
        };
        Ok(Some(Field {
            number,
            name,
            value,
            encoded_len: self.position - start,
        }))
    }
}
