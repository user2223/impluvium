// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: 0BSD

use std::{sync::Arc, time::Duration};

use axum::{
    body::{Body, Bytes},
    extract::{Request as HttpRequest, State},
    http::{Extensions, HeaderMap, HeaderValue, Method as HttpMethod, header},
    middleware::{self, Next as HttpNext},
    response::{IntoResponse, Response as HttpResponse},
};
use connectrpc::{
    AnyMessage, CodecFormat, ConnectError, ConnectRpcService, ErrorCode, ErrorDetail, Interceptor,
    Next, RequestContext,
    interceptor::{UnaryRequest, UnaryResponse},
};
use http_body_util::BodyExt;

use crate::{
    api::{
        self, Codec,
        errors::{self, Code, Context, WireError},
    },
    decode::{self, Diagnostic, InputLimit, Projection, Reason},
    limits::MessageKind,
    proto::impluvium::v2::*,
};

/// One entry per RPC, with no fallback schema or size allowance. Limits count
/// received wire bytes, including JSON escapes, whitespace and base64 growth.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MethodLimit {
    pub method: Method,
    pub path: &'static str,
    pub request: MessageKind,
    pub response: MessageKind,
    pub request_bytes: usize,
    pub response_bytes: usize,
    pub access_token: bool,
}

macro_rules! methods {
    ($(($name:ident, $service:literal, $req:ident, $res:ident, $limit:literal, $token:literal)),* $(,)?) => {
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum Method { $($name),* }

        pub const METHOD_LIMITS: &[MethodLimit] = &[$(MethodLimit {
            method: Method::$name,
            path: concat!("/impluvium.v2.", $service, "/", stringify!($name)),
            request: MessageKind::$req,
            response: MessageKind::$res,
            request_bytes: 65_536,
            response_bytes: $limit,
            access_token: $token,
        }),*];

        /// Borrowed, decoded request presented only after the raw guard passed.
        /// Policy implementations must verify proofs and contextual constraints
        /// before returning success; this enum is not authentication evidence.
        pub enum Request<'a> { $($name(&'a $req)),* }

        fn request(method: Method, payload: &connectrpc::Payload) -> Result<Request<'_>, ConnectError> {
            Ok(match method { $(Method::$name => Request::$name(payload.message::<$req>()?)),* })
        }

        fn response_json(method: Method, bytes: &[u8]) -> Result<Bytes, ConnectError> {
            match method { $(Method::$name => {
                let model: $res = decode::decoder_profile().decode_from_slice(bytes)
                    .map_err(|_| boundary_error())?;
                model.encode(CodecFormat::Json).map_err(|_| boundary_error())
            }),* }
        }
    };
}
methods!(
    (
        StartPairing,
        "AuthService",
        StartPairingRequest,
        StartPairingResponse,
        65_536,
        false
    ),
    (
        PollPairing,
        "AuthService",
        PollPairingRequest,
        PollPairingResponse,
        65_536,
        false
    ),
    (
        BeginSession,
        "AuthService",
        BeginSessionRequest,
        BeginSessionResponse,
        65_536,
        false
    ),
    (
        CompleteSession,
        "AuthService",
        CompleteSessionRequest,
        CompleteSessionResponse,
        65_536,
        false
    ),
    (
        RevokeSelf,
        "AuthService",
        RevokeSelfRequest,
        RevokeSelfResponse,
        65_536,
        true
    ),
    (
        ListDevices,
        "DeviceService",
        ListDevicesRequest,
        ListDevicesResponse,
        1_048_576,
        true
    ),
    (
        RevokeDevice,
        "DeviceService",
        RevokeDeviceRequest,
        RevokeDeviceResponse,
        1_048_576,
        true
    ),
    (
        ListLibrary,
        "LibraryService",
        ListLibraryRequest,
        ListLibraryResponse,
        4_194_304,
        true
    ),
    (
        GetGame,
        "LibraryService",
        GetGameRequest,
        GetGameResponse,
        4_194_304,
        true
    ),
    (
        ListVersions,
        "LibraryService",
        ListVersionsRequest,
        ListVersionsResponse,
        4_194_304,
        true
    ),
    (
        GetVersion,
        "LibraryService",
        GetVersionRequest,
        GetVersionResponse,
        4_194_304,
        true
    ),
    (
        OpenDownload,
        "DownloadService",
        OpenDownloadRequest,
        OpenDownloadResponse,
        2_097_152,
        true
    ),
    (
        RefreshTicket,
        "DownloadService",
        RefreshTicketRequest,
        RefreshTicketResponse,
        2_097_152,
        true
    ),
);

impl Method {
    pub fn limit(self) -> &'static MethodLimit {
        METHOD_LIMITS
            .iter()
            .find(|m| m.method == self)
            .expect("complete method table")
    }
    fn context(self) -> Context {
        match self {
            Self::StartPairing | Self::PollPairing => Context::Pairing,
            Self::BeginSession => Context::BeginSession,
            Self::CompleteSession => Context::CompleteSession,
            _ => Context::General,
        }
    }
}

/// Application-supplied checks in the chapter-defined order. There is no
/// permissive default policy. These checks may read state; state transitions
/// belong to the service handler, after validation, with atomic rechecks where
/// proofs.md requires them. Headers/extensions include the host's peer identity.
#[connectrpc::async_trait]
pub trait Policy: Send + Sync + 'static {
    /// StartPairing's address limit, before the body is read (proofs.md §1.3).
    async fn start_pairing(
        &self,
        headers: &HeaderMap,
        extensions: &Extensions,
    ) -> Result<(), ConnectError>;
    /// Access-token checks, after the byte limit and before profile/field checks.
    async fn access_token(
        &self,
        method: Method,
        headers: &HeaderMap,
        extensions: &Extensions,
    ) -> Result<(), ConnectError>;
    /// Required capabilities are checked after CompleteSession's field rules.
    fn required_client_capabilities(&self) -> &[String];
    /// Proof, device/pairing state, page-token and access checks appropriate to
    /// the method. Use the existing proof verifiers, never model observations
    /// as authentication evidence. Runs before all user interceptors/handlers.
    async fn validate(
        &self,
        ctx: &RequestContext,
        request: Request<'_>,
    ) -> Result<(), ConnectError>;
}

/// Build once, then mount the resulting router at `/api/v2` (with any base
/// prefix outside it). The internal HTTP adapter and first interceptor cannot
/// be reordered or removed through this API. Generated service registration is
/// unchanged; only known unary methods may be registered here.
pub struct Server<P> {
    policy: Arc<P>,
    service: ConnectRpcService,
    registered: Vec<&'static MethodLimit>,
    body_timeout: Duration,
    #[cfg(test)]
    decoded: Arc<std::sync::atomic::AtomicUsize>,
}

impl<P: Policy> Server<P> {
    pub fn new(router: connectrpc::Router, policy: P) -> Self {
        assert!(
            router.methods().all(|path| METHOD_LIMITS
                .iter()
                .any(|m| m.path.strip_prefix('/') == Some(path))),
            "protected server requires an explicit method contract"
        );
        let registered = METHOD_LIMITS
            .iter()
            .filter(|method| router.has_method(method.path))
            .collect();
        let policy = Arc::new(policy);
        #[cfg(test)]
        let decoded = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let semantic = Semantic {
            policy: policy.clone(),
            #[cfg(test)]
            decoded: decoded.clone(),
        };
        let service = ConnectRpcService::new(router)
            .with_limits(
                connectrpc::Limits::default()
                    .with_max_request_body_size(65_536)
                    .with_max_message_size(65_536),
            )
            .with_interceptor(semantic);
        Self {
            policy,
            service,
            registered,
            body_timeout: Duration::from_secs(30),
            #[cfg(test)]
            decoded,
        }
    }

    /// Appended inside the mandatory semantic validator. It sees only requests
    /// whose profile and semantic checks passed. Replacing messages here makes
    /// the application responsible for validating its replacement.
    pub fn with_interceptor(mut self, interceptor: impl Interceptor) -> Self {
        self.service = self.service.with_interceptor(interceptor);
        self
    }

    /// Bound the raw body read independently of an optional Connect deadline.
    pub fn with_body_timeout(mut self, timeout: Duration) -> Self {
        self.body_timeout = timeout;
        self
    }

    pub fn into_axum_router(self) -> axum::Router {
        let guard = Arc::new(Guard {
            policy: self.policy,
            registered: self.registered,
            body_timeout: self.body_timeout,
        });
        axum::Router::new()
            .fallback_service(self.service)
            .layer(middleware::from_fn_with_state(guard, protect::<P>))
    }
}

struct Guard<P> {
    policy: Arc<P>,
    registered: Vec<&'static MethodLimit>,
    body_timeout: Duration,
}

// Unforgeable outside this module. Both path and canonical bytes are checked:
// a stale extension cannot bless a changed payload or another method.
#[derive(Clone)]
struct Checked {
    method: Method,
    bytes: Bytes,
    projection: Arc<Projection>,
}

struct Semantic<P> {
    policy: Arc<P>,
    #[cfg(test)]
    decoded: Arc<std::sync::atomic::AtomicUsize>,
}

#[connectrpc::async_trait]
impl<P: Policy> Interceptor for Semantic<P> {
    async fn intercept_unary(
        &self,
        req: UnaryRequest,
        next: Next<'_>,
    ) -> Result<UnaryResponse, ConnectError> {
        let checked = req
            .ctx
            .extensions()
            .get::<Checked>()
            .ok_or_else(boundary_error)?;
        if req.ctx.path() != Some(checked.method.limit().path)
            || req.payload.format() != CodecFormat::Proto
            || req.payload.bytes() != &checked.bytes
        {
            return Err(boundary_error());
        }
        // Field checks use the original projection, preserving unknown enum
        // names/numbers and explicit presence before normalization loses them.
        validate_fields(
            &checked.projection,
            self.policy.required_client_capabilities(),
        )
        .map_err(|d| rpc_error(d, checked.method.context()))?;
        #[cfg(test)]
        self.decoded
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let request = request(checked.method, &req.payload)?;
        self.policy.validate(&req.ctx, request).await?;
        next.run(req).await
    }
}

async fn protect<P: Policy>(
    State(guard): State<Arc<Guard<P>>>,
    req: HttpRequest,
    next: HttpNext,
) -> HttpResponse {
    match protect_inner(&guard, req, next).await {
        Ok(response) => response,
        Err(error) => error.into_response(),
    }
}

async fn protect_inner<P: Policy>(
    guard: &Guard<P>,
    req: HttpRequest,
    next: HttpNext,
) -> Result<HttpResponse, ConnectError> {
    // The two dispatch checks precede body reads, credentials and schema lookup.
    if req.method() != HttpMethod::POST {
        return Err(ConnectError::unimplemented("POST required"));
    }
    if req
        .headers()
        .get_all(header::CONTENT_ENCODING)
        .iter()
        .any(|v| v.as_bytes() != b"identity")
    {
        return Err(ConnectError::unimplemented("content coding not supported"));
    }
    let method = guard
        .registered
        .iter()
        .copied()
        .find(|m| m.path == req.uri().path())
        .ok_or_else(|| ConnectError::unimplemented("unknown method"))?;
    if method.method == Method::StartPairing {
        guard
            .policy
            .start_pairing(req.headers(), req.extensions())
            .await?;
    }
    let (mut parts, body) = req.into_parts();
    let bytes = tokio::time::timeout(guard.body_timeout, read_body(body, method.request_bytes))
        .await
        .map_err(|_| ConnectError::deadline_exceeded("body read timed out"))??;
    if method.access_token {
        guard
            .policy
            .access_token(method.method, &parts.headers, &parts.extensions)
            .await?;
    }
    let codec = content_type(&parts.headers)?;
    let projection = api::project(
        method.request,
        codec,
        &bytes,
        InputLimit::body(method.request_bytes),
    )
    .map_err(|d| rpc_error(d, method.method.context()))?;
    // This is the entire request JSON adapter. Framework JSON conversion is
    // never invoked: only checked, known fields enter the binary decoder.
    let canonical = Bytes::from(projection.to_binary());
    parts.extensions.insert(Checked {
        method: method.method,
        bytes: canonical.clone(),
        projection: Arc::new(projection),
    });
    parts.headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/proto"),
    );
    parts.headers.remove(header::CONTENT_LENGTH);
    let response = next
        .run(HttpRequest::from_parts(parts, Body::from(canonical)))
        .await;
    if response.status() != axum::http::StatusCode::OK {
        return Ok(response);
    }
    let (mut parts, body) = response.into_parts();
    let bytes = read_body(body, method.response_bytes)
        .await
        .map_err(|_| boundary_error())?;
    decode::scan_binary(
        method.response,
        &bytes,
        InputLimit::response(method.response_bytes),
    )
    .map_err(|_| boundary_error())?;
    let bytes = match codec {
        Codec::Binary => bytes,
        Codec::Json => response_json(method.method, &bytes)?,
    };
    if bytes.len() > method.response_bytes {
        return Err(boundary_error());
    }
    parts.headers.remove(header::CONTENT_LENGTH);
    parts.headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(match codec {
            Codec::Binary => "application/proto",
            Codec::Json => "application/json",
        }),
    );
    Ok(HttpResponse::from_parts(parts, Body::from(bytes)))
}

fn content_type(headers: &HeaderMap) -> Result<Codec, ConnectError> {
    if headers.get_all(header::CONTENT_TYPE).iter().count() != 1 {
        return Err(ConnectError::invalid_argument("one content type required"));
    }
    let value = headers[header::CONTENT_TYPE]
        .to_str()
        .map_err(|_| ConnectError::invalid_argument("invalid content type"))?;
    match value.split(';').next().unwrap_or("").trim() {
        v if v.eq_ignore_ascii_case("application/proto") => Ok(Codec::Binary),
        v if v.eq_ignore_ascii_case("application/json") => Ok(Codec::Json),
        _ => Err(ConnectError::unimplemented("content type not supported")),
    }
}

async fn read_body(mut body: Body, limit: usize) -> Result<Bytes, ConnectError> {
    let mut bytes = Vec::new();
    while let Some(frame) = body.frame().await {
        let frame = frame.map_err(|_| ConnectError::invalid_argument("incomplete body"))?;
        if let Ok(data) = frame.into_data() {
            InputLimit::body(limit)
                .check_add(bytes.len() as u64, data.len() as u64)
                .map_err(|d| rpc_error(d, Context::General))?;
            bytes.extend_from_slice(&data);
        }
    }
    Ok(Bytes::from(bytes))
}

/// Map only catalogued local reasons; diagnostics and their fields never leak
/// into human text or ErrorInfo. Unsendable local reasons fail closed.
pub fn rpc_error(diagnostic: Diagnostic, context: Context) -> ConnectError {
    errors::to_wire(diagnostic.reason, context).map_or_else(boundary_error, wire_error)
}

/// Construct a catalogued code/reason combination. Local-only, reserved and
/// mismatched wire reasons cannot be emitted through this function.
pub fn wire_error(wire: WireError) -> ConnectError {
    let code = match wire.code {
        Code::canceled => ErrorCode::Canceled,
        Code::unknown => ErrorCode::Unknown,
        Code::invalid_argument => ErrorCode::InvalidArgument,
        Code::deadline_exceeded => ErrorCode::DeadlineExceeded,
        Code::not_found => ErrorCode::NotFound,
        Code::already_exists => ErrorCode::AlreadyExists,
        Code::permission_denied => ErrorCode::PermissionDenied,
        Code::resource_exhausted => ErrorCode::ResourceExhausted,
        Code::failed_precondition => ErrorCode::FailedPrecondition,
        Code::aborted => ErrorCode::Aborted,
        Code::out_of_range => ErrorCode::OutOfRange,
        Code::unimplemented => ErrorCode::Unimplemented,
        Code::internal => return boundary_error(),
        Code::unavailable => ErrorCode::Unavailable,
        Code::data_loss => ErrorCode::DataLoss,
        Code::unauthenticated => ErrorCode::Unauthenticated,
    };
    let error = ConnectError::new(code, "request rejected");
    let Some(reason) = wire.reason else {
        return error;
    };
    if !matches!(errors::wire_catalog(reason as i32), Some(errors::WireStatus::Active(expected, _)) if expected == wire.code)
    {
        return boundary_error();
    }
    error.with_detail(ErrorDetail::from_message(
        "impluvium.v2.ErrorInfo",
        &ErrorInfo {
            reason: reason.into(),
            ..Default::default()
        },
    ))
}

fn boundary_error() -> ConnectError {
    // Assign an opaque process-local request identifier. The hosting HTTP
    // logger can record it from ErrorInfo without exposing request contents.
    static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let id = NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let mut info = ErrorInfo::default();
    info.metadata
        .insert("request_id".into(), format!("connect-{id}"));
    ConnectError::internal("request could not be processed")
        .with_detail(ErrorDetail::from_message("impluvium.v2.ErrorInfo", &info))
}

fn validate_fields(p: &Projection, required: &[String]) -> Result<(), Diagnostic> {
    use decode::{Value, error};
    let bytes = |n| {
        p.fields
            .iter()
            .find(|(field, _)| *field == n)
            .and_then(|(_, v)| {
                if let Value::Bytes(b) = v {
                    Some(b.as_slice())
                } else {
                    None
                }
            })
            .unwrap_or(&[])
    };
    let string = |n| {
        p.fields
            .iter()
            .find(|(field, _)| *field == n)
            .and_then(|(_, v)| {
                if let Value::String(s) = v {
                    Some(s.as_str())
                } else {
                    None
                }
            })
            .unwrap_or("")
    };
    match p.message {
        MessageKind::StartPairingRequest => {
            for (n, field) in [
                (1, "StartPairingRequest.device_name"),
                (2, "StartPairingRequest.platform"),
                (3, "StartPairingRequest.arch"),
            ] {
                display(string(n), field)?;
            }
            crate::envelope::check_point(bytes(4))
                .map_err(|_| error(Reason::invalid_public_key, "StartPairingRequest.public_key"))?;
            display(string(5), "StartPairingRequest.client_version")
        }
        MessageKind::BeginSessionRequest => uuid(string(1), "BeginSessionRequest.device_id"),
        MessageKind::CompleteSessionRequest => {
            uuid(string(1), "CompleteSessionRequest.device_id")?;
            if bytes(2).len() != 32 {
                return Err(error(
                    Reason::invalid_challenge_length,
                    "CompleteSessionRequest.challenge",
                ));
            }
            if bytes(3).is_empty() {
                return Err(error(Reason::missing_field, "CompleteSessionRequest.proof"));
            }
            if !p
                .fields
                .iter()
                .any(|(n, v)| *n == 5 && *v == Value::Enum(1))
            {
                return Err(error(
                    Reason::unsupported_session_scope,
                    "CompleteSessionRequest.scope",
                ));
            }
            let mut offered = Vec::new();
            for (_, v) in p.fields.iter().filter(|(n, _)| *n == 4) {
                if let Value::String(s) = v {
                    if !crate::manifest::feature_name(s) {
                        return Err(error(
                            Reason::invalid_name,
                            "CompleteSessionRequest.client_capabilities",
                        ));
                    }
                    if offered.contains(s) {
                        return Err(error(
                            Reason::duplicate_capability,
                            "CompleteSessionRequest.client_capabilities",
                        ));
                    }
                    offered.push(s.clone());
                }
            }
            api::trust::check_capabilities(required, &offered).map_err(|_| {
                error(
                    Reason::client_too_old,
                    "CompleteSessionRequest.client_capabilities",
                )
            })?;
            proof_size(bytes(3), "CompleteSessionRequest.proof")
        }
        MessageKind::PollPairingRequest => {
            if bytes(1).len() != 16 {
                return Err(error(
                    Reason::invalid_pairing_id_length,
                    "PollPairingRequest.pairing_id",
                ));
            }
            if bytes(2).is_empty() {
                return Err(error(Reason::missing_field, "PollPairingRequest.proof"));
            }
            proof_size(bytes(2), "PollPairingRequest.proof")
        }
        _ => api::validate_request_fields(p),
    }
}

fn uuid(value: &str, field: &'static str) -> Result<(), Diagnostic> {
    if crate::manifest::uuid(value) {
        Ok(())
    } else {
        Err(decode::error(Reason::invalid_uuid, field))
    }
}
fn display(value: &str, field: &'static str) -> Result<(), Diagnostic> {
    if value.is_empty()
        || value
            .chars()
            .any(|c| c <= '\u{1f}' || ('\u{7f}'..='\u{9f}').contains(&c))
    {
        return Err(decode::error(Reason::invalid_text, field));
    }
    Ok(())
}
fn proof_size(bytes: &[u8], field: &'static str) -> Result<(), Diagnostic> {
    if bytes.len() > crate::proof::MAX_ENVELOPE_BYTES {
        Err(decode::error(Reason::envelope_too_large, field))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests;
