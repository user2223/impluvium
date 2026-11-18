// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: 0BSD

//! Pure API contracts. These boundaries consume bare schema messages, never
//! Connect wrappers, HTTP responses or authenticated server state.

use crate::{
    decode::{self, Diagnostic, InputLimit, Projection},
    limits::MessageKind,
    proto::impluvium::v2::*,
};

pub mod errors;
pub(crate) mod fields;
pub mod pagination;
pub mod trust;

#[derive(Clone, Copy, Debug)]
pub enum Codec {
    Binary,
    Json,
}

/// Check a bare API message's raw profile, limits, timestamps and field rules
/// before a transport hands it to an external generated decoder. This does not
/// establish request identity, a pairing pin, or signed-document authenticity;
/// those checks still require the chapter-specific context.
pub fn precheck_fields(
    kind: MessageKind,
    codec: Codec,
    bytes: &[u8],
    limit: InputLimit,
) -> Result<(), Diagnostic> {
    fields::validate(&project(kind, codec, bytes, limit)?)
}

#[cfg(feature = "server")]
pub(crate) fn validate_request_fields(p: &Projection) -> Result<(), Diagnostic> {
    fields::validate(p)
}

pub(crate) fn project(
    kind: MessageKind,
    codec: Codec,
    bytes: &[u8],
    limit: InputLimit,
) -> Result<Projection, Diagnostic> {
    match codec {
        Codec::Binary => decode::project_binary(kind, bytes, limit),
        Codec::Json => decode::project_json(kind, bytes, limit),
    }
}

mod private {
    pub trait Sealed {}
}

/// Only messages governed by encoding.md §7.2 implement this trait. Response
/// identities are mandatory arguments for the three context-bound messages.
pub trait GenericMessage: decode::ProtectedMessage + private::Sealed {
    type Context<'a>;
    const LIMIT: InputLimit;
    fn check_context(&self, context: Self::Context<'_>) -> Result<(), Diagnostic>;
}

/// Scan both codecs with the existing bounded reader, validate all fields in
/// schema order, and only then compare response identities with the request.
pub fn decode_generic<M: GenericMessage>(
    codec: Codec,
    bytes: &[u8],
    context: M::Context<'_>,
) -> Result<M, Diagnostic> {
    let p = project(M::KIND, codec, bytes, M::LIMIT)?;
    fields::validate(&p)?;
    let message: M = decode::decode_projection(&p)?;
    message.check_context(context)?;
    Ok(message)
}

macro_rules! generic {
    ($limit:expr; $($name:ident),* $(,)?) => {$ (
        impl private::Sealed for $name {}
        impl GenericMessage for $name {
            type Context<'a> = ();
            const LIMIT: InputLimit = $limit;
            fn check_context(&self, (): ()) -> Result<(), Diagnostic> { Ok(()) }
        }
    )*};
}
generic!(InputLimit::body(65_536);
    RevokeSelfRequest, ListDevicesRequest, RevokeDeviceRequest,
    ListLibraryRequest, GetGameRequest, ListVersionsRequest, GetVersionRequest,
    OpenDownloadRequest, RefreshTicketRequest);
generic!(InputLimit::response(65_536);
    StartPairingResponse, BeginSessionResponse, CompleteSessionResponse, RevokeSelfResponse);
generic!(InputLimit::response(1_048_576); ListDevicesResponse, Device, RevokeDeviceResponse);
generic!(InputLimit::response(4_194_304); ListLibraryResponse, GameSummary, VersionSummary);
generic!(InputLimit::response(2_097_152); RefreshTicketResponse);

macro_rules! identity_response {
    ($name:ident, $nested:ident, $identity:ident, $field:literal) => {
        impl private::Sealed for $name {}
        impl GenericMessage for $name {
            type Context<'a> = &'a str;
            const LIMIT: InputLimit = InputLimit::response(4_194_304);
            fn check_context(&self, expected: &str) -> Result<(), Diagnostic> {
                fields::identity(&self.$nested.$identity, expected, $field)
            }
        }
    };
}
identity_response!(GetGameResponse, game, game_id, "GameSummary.game_id");
identity_response!(
    GetVersionResponse,
    version,
    version_id,
    "VersionSummary.version_id"
);
impl private::Sealed for ListVersionsResponse {}
impl GenericMessage for ListVersionsResponse {
    type Context<'a> = &'a str;
    const LIMIT: InputLimit = InputLimit::response(4_194_304);
    fn check_context(&self, expected: &str) -> Result<(), Diagnostic> {
        for v in &self.versions {
            fields::identity(&v.game_id, expected, "VersionSummary.game_id")?;
        }
        Ok(())
    }
}

/// Identification and field rules only; version and pin checks follow separately.
pub fn decode_discovery(bytes: &[u8]) -> Result<Discovery, Diagnostic> {
    let p = project(
        MessageKind::Discovery,
        Codec::Json,
        bytes,
        InputLimit::response(65_536),
    )?;
    discovery_fields(&p)?;
    decode::decode_projection(&p)
}

pub(super) fn discovery_fields(p: &Projection) -> Result<(), Diagnostic> {
    if fields::number(p, 1) != 1 || fields::string(p, 2) != "impluvium" {
        return Err(decode::error(decode::Reason::not_an_impluvium_server, ""));
    }
    fields::validate(p)?;
    if !p
        .fields
        .iter()
        .any(|(n, v)| *n == 7 && *v == decode::Value::Uint32(3))
    {
        return Err(decode::error(
            decode::Reason::protocol_unsupported,
            "Discovery.manifest_formats",
        ));
    }
    Ok(())
}

/// A download response whose signed descriptor, request binding and manifest set
/// have all passed api.md §6.4.1. The ticket remains opaque to the client.
pub struct Download {
    response: OpenDownloadResponse,
    descriptor: crate::descriptor::Descriptor,
}
impl Download {
    pub fn response(&self) -> &OpenDownloadResponse {
        &self.response
    }
    pub fn descriptor(&self) -> &crate::descriptor::Descriptor {
        &self.descriptor
    }
}
pub fn decode_download(
    codec: Codec,
    bytes: &[u8],
    trusted: &crate::keys::TrustedKeySet<crate::keys::Content>,
    version_id: &str,
    game_id: Option<&str>,
) -> Result<Download, Diagnostic> {
    let p = project(
        MessageKind::OpenDownloadResponse,
        codec,
        bytes,
        InputLimit::response(2_097_152),
    )?;
    fields::validate(&p)?;
    let response: OpenDownloadResponse = decode::decode_projection(&p)?;
    let authenticated =
        crate::envelope::authenticate(&response.descriptor, crate::envelope::RELEASE, trusted)?;
    let descriptor = crate::descriptor::decode_payload(authenticated.payload())?;
    crate::descriptor::validate_identity(&descriptor, version_id, game_id)?;
    if !response
        .manifest_ids
        .contains(&descriptor.model().manifest_id)
    {
        return Err(decode::error(
            decode::Reason::manifest_set_mismatch,
            "OpenDownloadResponse.manifest_ids",
        ));
    }
    for endpoint in &response.depot_endpoints {
        fields::depot_url(&endpoint.base_url, "DepotEndpoint.base_url")?;
    }
    Ok(Download {
        response,
        descriptor,
    })
}

/// Decode the ErrorInfo detail itself; Connect JSON wrapper protection belongs
/// to the transport adapter. Map rules retain the scan's entry order.
pub fn decode_error_info(bytes: &[u8]) -> Result<ErrorInfo, Diagnostic> {
    let p = project(
        MessageKind::ErrorInfo,
        Codec::Binary,
        bytes,
        InputLimit::payload(8_192),
    )?;
    fields::validate(&p)?;
    decode::decode_projection(&p)
}

/// The endpoint field substage, also used when inspecting endpoints separately
/// from a download. Full downloads defer URL syntax until after the descriptor.
pub fn decode_depot_endpoint(codec: Codec, bytes: &[u8]) -> Result<DepotEndpoint, Diagnostic> {
    let p = project(
        MessageKind::DepotEndpoint,
        codec,
        bytes,
        InputLimit::response(2_097_152),
    )?;
    fields::validate(&p)?;
    let endpoint: DepotEndpoint = decode::decode_projection(&p)?;
    fields::depot_url(&endpoint.base_url, "DepotEndpoint.base_url")?;
    Ok(endpoint)
}

/// The complete per-field enum table of api.md §9. Signed and event fields
/// retain their own chapter validators; their classification is recorded here.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EnumUse {
    StateChanging,
    Display,
    ClientDecision,
    Content,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EnumDirection {
    Request,
    Response,
    Discovery,
    ErrorDetail,
    Event,
    SignedBinary,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EnumDefault {
    RejectScope,
    NoPlatform,
    InvalidResponse,
    IgnoreEntry,
    CodeFallback,
    Resynchronize,
    EndStream,
    Chapter,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UnknownEnum {
    RejectScope,
    RemoveElement,
    UnknownKind,
    IgnoreEntry,
    CodeFallback,
    Resynchronize,
    EndStream,
    Chapter,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EnumField {
    pub field: &'static str,
    pub direction: EnumDirection,
    pub container: crate::limits::Occurrence,
    pub usage: EnumUse,
    pub default: EnumDefault,
    pub unknown: UnknownEnum,
}
macro_rules! enum_fields {
    ($(($field:literal, $direction:ident, $container:ident, $usage:ident, $default:ident, $unknown:ident)),* $(,)?) => {
        pub const ENUM_FIELDS: &[EnumField] = &[$(EnumField {
            field: $field,
            direction: EnumDirection::$direction,
            container: crate::limits::Occurrence::$container,
            usage: EnumUse::$usage,
            default: EnumDefault::$default,
            unknown: UnknownEnum::$unknown,
        }),*];
    };
}
enum_fields!(
    (
        "CompleteSessionRequest.scope",
        Request,
        Scalar,
        StateChanging,
        RejectScope,
        RejectScope
    ),
    (
        "CompleteSessionResponse.scope",
        Response,
        Scalar,
        Display,
        RejectScope,
        RejectScope
    ),
    (
        "GameSummary.platforms",
        Response,
        Repeated,
        Display,
        NoPlatform,
        RemoveElement
    ),
    (
        "VersionSummary.platforms",
        Response,
        Repeated,
        Display,
        NoPlatform,
        RemoveElement
    ),
    (
        "VersionSummary.kind",
        Response,
        Scalar,
        Display,
        InvalidResponse,
        UnknownKind
    ),
    (
        "KeyFingerprint.role",
        Discovery,
        Scalar,
        Display,
        IgnoreEntry,
        IgnoreEntry
    ),
    (
        "ErrorInfo.reason",
        ErrorDetail,
        Scalar,
        ClientDecision,
        CodeFallback,
        CodeFallback
    ),
    (
        "ResyncRequired.reason",
        Event,
        Scalar,
        Display,
        Resynchronize,
        Resynchronize
    ),
    (
        "SessionEnded.reason",
        Event,
        Scalar,
        Display,
        EndStream,
        EndStream
    ),
    (
        "Header.hash_algorithm",
        SignedBinary,
        Scalar,
        Content,
        Chapter,
        Chapter
    ),
    (
        "Chunking.algorithm",
        SignedBinary,
        Scalar,
        Content,
        Chapter,
        Chapter
    ),
    (
        "Entry.type",
        SignedBinary,
        Scalar,
        Content,
        Chapter,
        Chapter
    ),
    (
        "ReleaseDescriptor.kind",
        SignedBinary,
        Scalar,
        Content,
        Chapter,
        Chapter
    ),
    (
        "LaunchOption.platform",
        SignedBinary,
        Scalar,
        Content,
        Chapter,
        Chapter
    ),
    (
        "SetupStep.platform",
        SignedBinary,
        Scalar,
        Content,
        Chapter,
        Chapter
    ),
);

/// Shared WHATWG parser for adapters; prevents a second URL implementation or
/// a transport-stack-specific interpretation of credential destinations.
pub mod url {
    pub use ::url::{Host, Url};
}
