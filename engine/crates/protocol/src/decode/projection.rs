// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: 0BSD

use super::*;

/// A bounded projection of known fields. Message presence and wire order are retained;
/// unknown fields have no representation here. Missing scalar defaults are implicit.
#[derive(Clone, Debug, PartialEq)]
pub struct Projection {
    pub message: MessageKind,
    pub fields: Vec<(u32, Value)>,
}
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Uint32(u32),
    Uint64(u64),
    Int32(i32),
    Int64(i64),
    Bool(bool),
    String(String),
    Bytes(Vec<u8>),
    Enum(i32),
    UnknownEnumName(String),
    Message(Projection),
    MapEntry(String, String),
}

pub(super) fn preserve_enum(rule: &FieldRule) -> bool {
    matches!(
        rule.diagnostic,
        "Header.hash_algorithm"
            | "Chunking.algorithm"
            | "Entry.type"
            | "ReleaseDescriptor.kind"
            | "LaunchOption.platform"
            | "SetupStep.platform"
            | "VersionSummary.kind"
    )
}

impl Projection {
    pub(crate) fn to_binary(&self) -> Vec<u8> {
        let mut out = Vec::new();
        self.write(&mut out);
        out
    }
    fn write(&self, out: &mut Vec<u8>) {
        for (number, value) in &self.fields {
            let Some(rule) = self.message.fields().iter().find(|r| r.number == *number) else {
                continue;
            };
            if let Kind::Enum(values) = rule.kind {
                let unknown = match value {
                    Value::Enum(n) => !values.iter().any(|(_, v)| v == n),
                    Value::UnknownEnumName(_) => true,
                    _ => false,
                };
                if unknown {
                    if rule.occurrence == Occurrence::Repeated {
                        continue;
                    }
                    if !preserve_enum(rule) {
                        scalar(out, *number, 0);
                        continue;
                    }
                    if matches!(value, Value::UnknownEnumName(_)) {
                        // Generated enums cannot retain JSON names. Keep an
                        // unlisted numeric value for chapter-specific unknown
                        // handling; the original name remains in Projection.
                        let mut sentinel = i32::MIN;
                        while values.iter().any(|(_, n)| *n == sentinel) {
                            sentinel += 1;
                        }
                        scalar(out, *number, sentinel as i64 as u64);
                        continue;
                    }
                }
            }
            match value {
                Value::Uint32(n) => scalar(out, *number, u64::from(*n)),
                Value::Uint64(n) => scalar(out, *number, *n),
                Value::Int32(n) | Value::Enum(n) => scalar(out, *number, *n as i64 as u64),
                Value::Int64(n) => scalar(out, *number, *n as u64),
                Value::Bool(b) => scalar(out, *number, u64::from(*b)),
                Value::String(s) => len(out, *number, s.as_bytes()),
                Value::Bytes(b) => len(out, *number, b),
                Value::Message(m) => {
                    // A fixed ten-byte placeholder avoids allocating a temporary
                    // child buffer. Moving the suffix is bounded by depth 64.
                    varint(out, (u64::from(*number) << 3) | 2);
                    let prefix = out.len();
                    out.extend_from_slice(&[0; 10]);
                    m.write(out);
                    finish_length(out, prefix);
                }
                Value::MapEntry(k, v) => {
                    varint(out, (u64::from(*number) << 3) | 2);
                    let prefix = out.len();
                    out.extend_from_slice(&[0; 10]);
                    len(out, 1, k.as_bytes());
                    len(out, 2, v.as_bytes());
                    finish_length(out, prefix);
                }
                Value::UnknownEnumName(_) => {}
            }
        }
    }
}
fn finish_length(out: &mut Vec<u8>, prefix: usize) {
    let mut length = (out.len() - prefix - 10) as u64;
    let mut encoded = [0; 10];
    let mut size = 0;
    loop {
        encoded[size] = (length & 127) as u8;
        length >>= 7;
        if length != 0 {
            encoded[size] |= 128;
        }
        size += 1;
        if length == 0 {
            break;
        }
    }
    out[prefix..prefix + size].copy_from_slice(&encoded[..size]);
    out.copy_within(prefix + 10.., prefix + size);
    out.truncate(out.len() - (10 - size));
}
fn varint(out: &mut Vec<u8>, mut n: u64) {
    while n >= 128 {
        out.push(n as u8 | 128);
        n >>= 7;
    }
    out.push(n as u8);
}
fn scalar(out: &mut Vec<u8>, field: u32, n: u64) {
    varint(out, u64::from(field) << 3);
    varint(out, n);
}
fn len(out: &mut Vec<u8>, field: u32, bytes: &[u8]) {
    varint(out, (u64::from(field) << 3) | 2);
    varint(out, bytes.len() as u64);
    out.extend_from_slice(bytes);
}

macro_rules! protected {
    ($($name:ident),* $(,)?) => {
        $(impl private::Sealed for $name {}
        impl ProtectedMessage for $name { const KIND: MessageKind = MessageKind::$name; })*
    };
}

use crate::proto::impluvium::v2::{
    BeginSessionRequest, BeginSessionResponse, CompleteSessionRequest, CompleteSessionResponse,
    Connected, DepotEndpoint, DepotTicketClaims, Device, Discovery, ErrorInfo, GameSummary,
    GetGameRequest, GetGameResponse, GetVersionRequest, GetVersionResponse, Heartbeat,
    KeyFingerprint, LibraryChanged, ListDevicesRequest, ListDevicesResponse, ListLibraryRequest,
    ListLibraryResponse, ListVersionsRequest, ListVersionsResponse, OpenDownloadRequest,
    OpenDownloadResponse, PairingProof, PollPairingRequest, PollPairingResponse, ProtocolVersion,
    RefreshTicketRequest, RefreshTicketResponse, ResyncRequired, RevokeDeviceRequest,
    RevokeDeviceResponse, RevokeSelfRequest, RevokeSelfResponse, ServerEvent, SessionEnded,
    SessionProof, StartPairingRequest, StartPairingResponse, Superseded, VersionSummary,
};
protected!(
    StartPairingRequest,
    StartPairingResponse,
    PollPairingRequest,
    PollPairingResponse,
    BeginSessionRequest,
    BeginSessionResponse,
    CompleteSessionRequest,
    CompleteSessionResponse,
    RevokeSelfRequest,
    RevokeSelfResponse,
    ListDevicesRequest,
    ListDevicesResponse,
    Device,
    RevokeDeviceRequest,
    RevokeDeviceResponse,
    Discovery,
    ProtocolVersion,
    KeyFingerprint,
    OpenDownloadRequest,
    OpenDownloadResponse,
    DepotEndpoint,
    RefreshTicketRequest,
    RefreshTicketResponse,
    ErrorInfo,
    ServerEvent,
    Connected,
    Heartbeat,
    ResyncRequired,
    SessionEnded,
    Superseded,
    LibraryChanged,
    ListLibraryRequest,
    ListLibraryResponse,
    GameSummary,
    GetGameRequest,
    GetGameResponse,
    ListVersionsRequest,
    ListVersionsResponse,
    VersionSummary,
    GetVersionRequest,
    GetVersionResponse,
    SessionProof,
    PairingProof,
    DepotTicketClaims
);
use crate::proto::impluvium::manifest::v3::{
    Chunk, Chunking, Command, EmulatorRef, Entry, Header, LaunchOption, Manifest,
    ReleaseDescriptor, SetupStep,
};
protected!(
    ReleaseDescriptor,
    LaunchOption,
    Command,
    EmulatorRef,
    SetupStep,
    Manifest,
    Header,
    Chunking,
    Entry,
    Chunk
);
use crate::proto::impluvium::signed::v1::{Envelope, Signature};
protected!(Envelope, Signature);
use buffa_types::Timestamp;
protected!(Timestamp);
