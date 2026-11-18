// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: 0BSD

//! Response protection is bound before sending. Adapters feed each head before
//! any body bytes, keep duplicate field lines, and never decompress a response.
//! Deadlines and incomplete transfers remain adapter errors, not broken bodies.

use crate::{
    error::{Diagnostic, Reason, broken_body, local},
    transport::{Method, Purpose},
};
use impluvium_protocol::{
    api::{self, Codec, errors::ErrorBody, trust::Pin},
    decode, descriptor, envelope,
    hash::Algorithm,
    ids,
    keys::{Content, TrustedKeySet},
    limits::MessageKind,
    manifest::{self, Manifest, Profile},
    proto::impluvium::v2::{Discovery, PollPairingResponse},
};

pub mod range;
pub const HEAD_LIMIT: usize = 65_536;
pub const ERROR_LIMIT: usize = 65_536;

#[derive(Clone, Debug)]
pub struct Binding {
    purpose: Purpose,
    status: u16,
    limit: usize,
    content_type: String,
    codec: Option<Codec>,
    chunk: Option<Chunk>,
}
impl Binding {
    pub fn discovery() -> Self {
        Self::new(Purpose::Discovery, 200, 65_536, "application/json")
    }
    pub fn rpc(method: Method, codec: Codec) -> Self {
        let mut bound = Self::new(
            Purpose::Rpc(method),
            200,
            method.response_limit(),
            match codec {
                Codec::Binary => "application/proto",
                Codec::Json => "application/json",
            },
        );
        bound.codec = Some(codec);
        bound
    }
    pub fn manifest() -> Self {
        Self::new(
            Purpose::Manifest,
            200,
            536_887_296,
            "application/octet-stream",
        )
    }
    pub fn descriptor() -> Self {
        Self::new(
            Purpose::Descriptor,
            200,
            1_064_960,
            "application/octet-stream",
        )
    }
    pub fn object(limit: usize, content_type: &str) -> Self {
        Self::new(Purpose::Object, 200, limit, content_type)
    }
    pub fn events() -> Self {
        Self::new(Purpose::Events, 200, 0, "text/event-stream")
    }
    pub fn chunk(chunk: Chunk) -> Option<Self> {
        if chunk.size == 0 || chunk.size > 16_777_216 {
            return None;
        }
        let (status, length) = match chunk.range {
            None => (200, chunk.size),
            Some(r) if r.first <= r.last && r.last < chunk.size => (206, r.last - r.first + 1),
            _ => return None,
        };
        let mut bound = Self::new(
            Purpose::Chunk,
            status,
            length as usize,
            "application/octet-stream",
        );
        bound.chunk = Some(chunk);
        Some(bound)
    }
    fn new(purpose: Purpose, status: u16, limit: usize, content_type: &str) -> Self {
        Self {
            purpose,
            status,
            limit,
            content_type: content_type.into(),
            codec: None,
            chunk: None,
        }
    }
    pub fn purpose(&self) -> Purpose {
        self.purpose
    }
    pub fn success_limit(&self) -> usize {
        self.limit
    }
    pub fn success_status(&self) -> u16 {
        self.status
    }
    pub(crate) fn chunk_id(&self) -> Option<&[u8; 32]> {
        self.chunk.as_ref().map(|chunk| &chunk.id)
    }
}

#[derive(Clone, Debug)]
pub struct Chunk {
    pub size: u64,
    pub id: [u8; 32],
    pub algorithm: Algorithm,
    pub range: Option<range::ByteRange>,
}
impl Chunk {
    /// Both the complete candidate and the actual received transfer must agree
    /// with the manifest and request before any target write is permitted.
    pub fn accepts(&self, transfer: &[u8], candidate: &[u8]) -> bool {
        if candidate.len() as u64 != self.size || self.algorithm.digest(candidate) != self.id {
            return false;
        }
        match self.range {
            None => transfer == candidate,
            Some(r) => candidate.get(r.first as usize..=r.last as usize) == Some(transfer),
        }
    }
}

/// The measurement is taken by the HTTP adapter before combination or
/// whitespace normalization. Repeated HTTP/2 fields each carry their own 32.
pub fn check_head_size(size: u64) -> Result<(), Diagnostic> {
    if size > HEAD_LIMIT as u64 {
        Err(local(Reason::headers_too_large))
    } else {
        Ok(())
    }
}
pub fn h2_head_size<'a>(
    fields: impl IntoIterator<Item = (&'a [u8], &'a [u8])>,
) -> Result<u64, Diagnostic> {
    let mut size = 0u64;
    for (name, value) in fields {
        size = add_h2_field(size, name, value);
        check_head_size(size)?;
    }
    Ok(size)
}

/// Measure an already decoded header list for diagnostics, including repeated
/// lines and pseudo-headers. Use `h2_head_size` to enforce the read boundary.
pub fn h2_header_list_size<'a>(fields: impl IntoIterator<Item = (&'a [u8], &'a [u8])>) -> u64 {
    fields
        .into_iter()
        .fold(0, |size, (name, value)| add_h2_field(size, name, value))
}

fn add_h2_field(size: u64, name: &[u8], value: &[u8]) -> u64 {
    size.saturating_add(name.len() as u64)
        .saturating_add(value.len() as u64)
        .saturating_add(32)
}

#[derive(Clone, Copy, Debug)]
pub struct Header<'a> {
    pub name: &'a str,
    pub value: &'a str,
}
fn header<'a>(headers: &[Header<'a>], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|h| h.name.eq_ignore_ascii_case(name))
        .map(|h| h.value.trim_matches([' ', '\t']))
}

pub enum HeadDecision {
    Informational,
    Body(BodyReader),
    Redirect(String),
    /// Hand subsequent bytes to the bounded SSE event receiver. There is no
    /// aggregate stream body buffer; the event module applies per-event limits.
    Events,
}

pub fn head(
    binding: &Binding,
    measured: u64,
    status: u16,
    headers: &[Header<'_>],
) -> Result<HeadDecision, Diagnostic> {
    check_head_size(measured)?;
    if (100..200).contains(&status) {
        return Ok(HeadDecision::Informational);
    }
    let redirect = (300..400).contains(&status);
    if redirect && binding.purpose != Purpose::Chunk {
        return Err(local(Reason::redirect_not_allowed));
    }
    for line in headers
        .iter()
        .filter(|h| h.name.eq_ignore_ascii_case("content-encoding"))
    {
        if !line.value.split(',').all(|coding| {
            coding
                .trim_matches([' ', '\t'])
                .eq_ignore_ascii_case("identity")
        }) {
            return Err(local(Reason::unsupported_content_encoding));
        }
    }
    if redirect {
        // Section 6.3 has no exception for chunk redirects. This is only a
        // candidate; section 7 still checks the count, target and preflight.
        return Ok(HeadDecision::Redirect(
            header(headers, "location").unwrap_or("").into(),
        ));
    }
    let success = status == binding.status;
    let expected = if success {
        binding.content_type.as_str()
    } else {
        "application/json"
    };
    let content_type = header(headers, "content-type")
        .unwrap_or("")
        .split(';')
        .next()
        .unwrap_or("")
        .trim_matches([' ', '\t']);
    if !content_type.eq_ignore_ascii_case(expected) {
        return Err(broken_body(status));
    }
    if success && binding.purpose == Purpose::Events {
        return Ok(HeadDecision::Events);
    }
    Ok(HeadDecision::Body(BodyReader {
        binding: binding.clone(),
        status,
        success,
        bytes: Vec::new(),
        failed: None,
    }))
}

/// Adapters must request at most `remaining()` body bytes. Once it reaches zero,
/// probe for one more byte without buffering it; pass that byte to `push` to
/// abandon an oversized response. Content-Length never changes this budget.
pub struct BodyReader {
    binding: Binding,
    status: u16,
    success: bool,
    bytes: Vec<u8>,
    failed: Option<Diagnostic>,
}
impl BodyReader {
    /// The external Connect runtime consumes only these prechecked bytes.
    /// In particular, the error wrapper and every ErrorInfo detail have passed
    /// the protocol reader before Connect can run serde or a detail decoder.
    pub(crate) fn finish_for_decoder(self) -> Result<Vec<u8>, Diagnostic> {
        if let Some(d) = self.failed {
            return Err(d);
        }
        if self.success {
            let body = CheckedBody {
                binding: self.binding,
                bytes: self.bytes,
            };
            body.precheck_rpc()?;
            Ok(body.bytes)
        } else {
            let checked =
                decode::decode_error_body(&self.bytes).map_err(|_| broken_body(self.status))?;
            if checked.code() == crate::error::Code::unknown {
                // Connect's parser falls back to the HTTP status for unlisted
                // codes. Only after the complete guard succeeds may a general
                // JSON decoder normalize that code, retaining message/details.
                // Raw values also preserve ignored extension values without
                // imposing a floating-point range on their JSON numbers.
                let mut value: std::collections::BTreeMap<
                    String,
                    Box<serde_json::value::RawValue>,
                > = serde_json::from_slice(&self.bytes).map_err(|_| broken_body(self.status))?;
                let code = serde_json::value::RawValue::from_string("\"unknown\"".into())
                    .map_err(|_| broken_body(self.status))?;
                value.insert("code".into(), code);
                serde_json::to_vec(&value).map_err(|_| broken_body(self.status))
            } else {
                Ok(self.bytes)
            }
        }
    }

    fn limit(&self) -> usize {
        if self.success {
            self.binding.limit
        } else {
            ERROR_LIMIT
        }
    }
    pub fn remaining(&self) -> usize {
        if self.failed.is_some() {
            0
        } else {
            self.limit() - self.bytes.len()
        }
    }
    pub fn push(&mut self, bytes: &[u8]) -> Result<(), Diagnostic> {
        if let Some(d) = self.failed {
            return Err(d);
        }
        if bytes.len() > self.remaining() {
            let d = if self.success {
                local(Reason::response_too_large)
            } else {
                broken_body(self.status)
            };
            self.failed = Some(d);
            self.bytes.clear();
            return Err(d);
        }
        if self.bytes.capacity() - self.bytes.len() < bytes.len() {
            self.bytes.reserve_exact(bytes.len());
        }
        self.bytes.extend_from_slice(bytes);
        Ok(())
    }
    /// Call only after a complete transfer. An adapter timeout or framing error
    /// must bypass this operation and retain its transport failure.
    pub fn finish(self) -> Result<Outcome, Diagnostic> {
        if let Some(d) = self.failed {
            return Err(d);
        }
        if self.success {
            Ok(Outcome::Success(CheckedBody {
                binding: self.binding,
                bytes: self.bytes,
            }))
        } else {
            decode::decode_error_body(&self.bytes)
                .map(Outcome::Error)
                .map_err(|_| broken_body(self.status))
        }
    }
}

pub enum Outcome {
    Success(CheckedBody),
    Error(ErrorBody),
    Redirect(String),
    Events,
}

pub struct CheckedBody {
    binding: Binding,
    bytes: Vec<u8>,
}
impl CheckedBody {
    /// The generated Connect decoder must never see an unscanned success body.
    /// Chapter-specific identity and signature checks remain separate operations.
    pub fn precheck_rpc(&self) -> Result<(), Diagnostic> {
        let Purpose::Rpc(method) = self.binding.purpose else {
            return Err(local(Reason::broken_error_body));
        };
        api::precheck_fields(
            response_kind(method),
            self.binding.codec.expect("RPC codec bound before sending"),
            &self.bytes,
            decode::InputLimit::response(self.binding.limit),
        )
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    pub fn binding(&self) -> &Binding {
        &self.binding
    }
    pub fn discovery(&self) -> Result<Discovery, Diagnostic> {
        if self.binding.purpose != Purpose::Discovery {
            return Err(local(Reason::broken_error_body));
        }
        api::decode_discovery(&self.bytes)
    }
    pub fn generic<M: api::GenericMessage>(
        &self,
        context: M::Context<'_>,
    ) -> Result<M, Diagnostic> {
        let Purpose::Rpc(method) = self.binding.purpose else {
            return Err(local(Reason::broken_error_body));
        };
        if response_kind(method) != M::KIND {
            return Err(local(Reason::broken_error_body));
        }
        api::decode_generic(
            self.binding.codec.expect("RPC codec bound before sending"),
            &self.bytes,
            context,
        )
    }
    pub fn pairing(&self, discovery: &Discovery) -> Result<(PollPairingResponse, Pin), Diagnostic> {
        if self.binding.purpose != Purpose::Rpc(Method::PollPairing) {
            return Err(local(Reason::broken_error_body));
        }
        api::trust::decode_pairing_result(
            self.binding.codec.expect("RPC codec"),
            &self.bytes,
            discovery,
        )
    }
    pub fn download(
        &self,
        trusted: &TrustedKeySet<Content>,
        version: &str,
        game: Option<&str>,
    ) -> Result<api::Download, Diagnostic> {
        if self.binding.purpose != Purpose::Rpc(Method::OpenDownload) {
            return Err(local(Reason::broken_error_body));
        }
        api::decode_download(
            self.binding.codec.expect("RPC codec"),
            &self.bytes,
            trusted,
            version,
            game,
        )
    }
    pub fn chunk_accepts(&self, candidate: &[u8]) -> bool {
        self.binding
            .chunk
            .as_ref()
            .is_some_and(|chunk| chunk.accepts(&self.bytes, candidate))
    }
}

fn response_kind(method: Method) -> MessageKind {
    match method {
        Method::StartPairing => MessageKind::StartPairingResponse,
        Method::PollPairing => MessageKind::PollPairingResponse,
        Method::BeginSession => MessageKind::BeginSessionResponse,
        Method::CompleteSession => MessageKind::CompleteSessionResponse,
        Method::RevokeSelf => MessageKind::RevokeSelfResponse,
        Method::ListDevices => MessageKind::ListDevicesResponse,
        Method::RevokeDevice => MessageKind::RevokeDeviceResponse,
        Method::ListLibrary => MessageKind::ListLibraryResponse,
        Method::GetGame => MessageKind::GetGameResponse,
        Method::ListVersions => MessageKind::ListVersionsResponse,
        Method::GetVersion => MessageKind::GetVersionResponse,
        Method::OpenDownload => MessageKind::OpenDownloadResponse,
        Method::RefreshTicket => MessageKind::RefreshTicketResponse,
    }
}

/// Data-only HTTP/1.1 response reader for complete, uncompressed, already
/// transfer-framed fixtures. Live adapters feed `head` and `BodyReader` directly.
/// The head search is bounded even when no terminating empty line is present.
pub fn http1(binding: &Binding, mut raw: &[u8]) -> Result<Outcome, Diagnostic> {
    loop {
        let window = &raw[..raw.len().min(HEAD_LIMIT + 1)];
        let Some(end) = window
            .windows(4)
            .position(|b| b == b"\r\n\r\n")
            .map(|n| n + 4)
        else {
            return Err(if raw.len() > HEAD_LIMIT {
                local(Reason::headers_too_large)
            } else {
                local(Reason::broken_error_body)
            });
        };
        check_head_size(end as u64)?;
        let text = String::from_utf8_lossy(&raw[..end]);
        let mut lines = text.split("\r\n");
        let status = lines
            .next()
            .and_then(|line| line.split_whitespace().nth(1))
            .and_then(|s| s.parse::<u16>().ok())
            .ok_or(local(Reason::broken_error_body))?;
        raw = &raw[end..];
        if (100..200).contains(&status) {
            continue;
        }
        let headers: Vec<_> = lines
            .filter(|s| !s.is_empty())
            .map(|line| {
                let (name, value) = line
                    .split_once(':')
                    .ok_or(local(Reason::broken_error_body))?;
                Ok(Header {
                    name,
                    value: value.trim_matches([' ', '\t']),
                })
            })
            .collect::<Result<_, Diagnostic>>()?;
        return match head(binding, end as u64, status, &headers)? {
            HeadDecision::Informational => unreachable!("informational heads handled above"),
            HeadDecision::Redirect(target) => Ok(Outcome::Redirect(target)),
            HeadDecision::Events => Ok(Outcome::Events),
            HeadDecision::Body(mut reader) => {
                reader.push(raw)?;
                reader.finish()
            }
        };
    }
}

/// Testable decode boundary: early HTTP rejection never invokes `decode`.
/// Production callers use the protected chapter functions on `CheckedBody`.
pub fn decode_with<T>(
    binding: &Binding,
    raw: &[u8],
    decode: impl FnOnce(CheckedBody) -> Result<T, Diagnostic>,
) -> Result<Result<T, ErrorBody>, Diagnostic> {
    match http1(binding, raw)? {
        Outcome::Success(body) => decode(body).map(Ok),
        Outcome::Error(error) => Ok(Err(error)),
        _ => Err(local(Reason::redirect_not_allowed)),
    }
}

impl CheckedBody {
    pub fn manifest(
        &self,
        trusted: &TrustedKeySet<Content>,
        requested_id: &[u8; 32],
        profile: Profile,
    ) -> Result<(Manifest, Vec<Diagnostic>), Diagnostic> {
        if self.binding.purpose != Purpose::Manifest {
            return Err(local(Reason::broken_error_body));
        }
        let payload = envelope::authenticate(&self.bytes, envelope::MANIFEST, trusted)?;
        let result = manifest::decode_payload(payload.payload(), profile)?;
        if ids::manifest_id(payload.payload()) != *requested_id {
            return Err(local(Reason::manifest_id_mismatch));
        }
        Ok(result)
    }
    pub fn descriptor(
        &self,
        trusted: &TrustedKeySet<Content>,
        context: &descriptor::Context<'_>,
    ) -> Result<(descriptor::Descriptor, Vec<Diagnostic>), Diagnostic> {
        if self.binding.purpose != Purpose::Descriptor {
            return Err(local(Reason::broken_error_body));
        }
        descriptor::verify(&self.bytes, trusted, context)
    }
}
