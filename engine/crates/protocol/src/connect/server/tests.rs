// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: 0BSD

//! Loopback-only transport fixtures. No test service is compiled into a product.

use super::*;
use crate::connect::impluvium::v2::*;
use buffa::Message;
use std::{
    net::SocketAddr,
    sync::atomic::{AtomicUsize, Ordering},
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[allow(dead_code)]
#[path = "../../../tests/support/corpus.rs"]
mod corpus;

const DEVICE: &str = "33333333-3333-3333-3333-333333333333";

fn run(future: impl std::future::Future<Output = ()>) {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(future);
}

#[derive(Default)]
struct TestPolicy {
    required: Vec<String>,
    calls: AtomicUsize,
    tokens: AtomicUsize,
    deny_start: bool,
    deny_token: bool,
    deny_validation: bool,
}

#[connectrpc::async_trait]
impl Policy for TestPolicy {
    async fn start_pairing(&self, _: &HeaderMap, _: &Extensions) -> Result<(), ConnectError> {
        if self.deny_start {
            Err(wire_error(WireError {
                code: Code::resource_exhausted,
                reason: Some(ErrorReason::RateLimited),
            }))
        } else {
            Ok(())
        }
    }
    async fn access_token(
        &self,
        _: Method,
        _: &HeaderMap,
        _: &Extensions,
    ) -> Result<(), ConnectError> {
        self.tokens.fetch_add(1, Ordering::SeqCst);
        if self.deny_token {
            Err(ConnectError::unauthenticated("test token rejection"))
        } else {
            Ok(())
        }
    }
    fn required_client_capabilities(&self) -> &[String] {
        &self.required
    }
    async fn validate(&self, _: &RequestContext, request: Request<'_>) -> Result<(), ConnectError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let key = crate::keys::SigningKey::<crate::keys::Device>::from_seed(&[19; 32]);
        let trusted = crate::keys::TrustedKeySet::new(&[crate::keys::KeyInput {
            public_key: &key.public_key(),
            role: crate::proto::impluvium::signed::v1::KeyRole::Device,
            ticket_status: None,
        }])
        .unwrap();
        match request {
            Request::CompleteSession(r) => {
                let challenge: &[u8; 32] = r.challenge.as_slice().try_into().unwrap();
                crate::proof::verify_session(
                    &r.proof,
                    &trusted,
                    &crate::proof::SessionContext {
                        instance_id: DEVICE,
                        device_id: &r.device_id,
                        challenge,
                    },
                )
                .map_err(|d| rpc_error(d, Context::Proof))?;
                let accepted = api::trust::accepted_capabilities(
                    &r.client_capabilities,
                    &["events.v1".into()],
                );
                assert!(accepted.iter().all(|c| c == "events.v1"));
            }
            Request::PollPairing(r) => {
                let pairing_id: &[u8; 16] = r.pairing_id.as_slice().try_into().unwrap();
                crate::proof::verify_pairing(
                    &r.proof,
                    &trusted,
                    &crate::proof::PairingContext {
                        instance_id: DEVICE,
                        pairing_id,
                    },
                )
                .map_err(|d| rpc_error(d, Context::Proof))?;
            }
            _ => {}
        }
        if self.deny_validation {
            Err(rpc_error(
                Diagnostic {
                    reason: Reason::invalid_signature,
                    field: "Signature.sig",
                },
                Context::Proof,
            ))
        } else {
            Ok(())
        }
    }
}

struct TestService {
    handlers: Arc<AtomicUsize>,
    seconds: i64,
    nanos: i32,
}

trait Reply: Default {
    fn reply(_: &TestService) -> Self {
        Self::default()
    }
}
macro_rules! replies {
    ($($response:ty),* $(,)?) => { $(impl Reply for $response {})* };
}
replies!(
    StartPairingResponse,
    PollPairingResponse,
    CompleteSessionResponse,
    RevokeSelfResponse,
    ListDevicesResponse,
    RevokeDeviceResponse,
    ListLibraryResponse,
    GetGameResponse,
    ListVersionsResponse,
    GetVersionResponse,
    OpenDownloadResponse,
    RefreshTicketResponse
);
impl Reply for BeginSessionResponse {
    fn reply(service: &TestService) -> Self {
        let mut reply = Self {
            challenge: vec![1; 32],
            ..Default::default()
        };
        reply.expires_at.modify(|t| {
            t.seconds = service.seconds;
            t.nanos = service.nanos;
        });
        reply
    }
}

macro_rules! service {
    ($service:ident; $(($method:ident, $req:ty, $res:ty)),* $(,)?) => {
        impl $service for TestService {
            $(fn $method<'a>(&'a self, _: RequestContext, _: connectrpc::ServiceRequest<'_, $req>)
                -> impl std::future::Future<Output = connectrpc::ServiceResult<impl connectrpc::Encodable<$res> + Send + use<'a>>> + Send
            {
                self.handlers.fetch_add(1, Ordering::SeqCst);
                let reply = <$res as Reply>::reply(self);
                async move { connectrpc::Response::ok(reply) }
            })*
        }
    };
}
service!(AuthService;
    (start_pairing, StartPairingRequest, StartPairingResponse),
    (poll_pairing, PollPairingRequest, PollPairingResponse),
    (begin_session, BeginSessionRequest, BeginSessionResponse),
    (complete_session, CompleteSessionRequest, CompleteSessionResponse),
    (revoke_self, RevokeSelfRequest, RevokeSelfResponse));
service!(DeviceService;
    (list_devices, ListDevicesRequest, ListDevicesResponse),
    (revoke_device, RevokeDeviceRequest, RevokeDeviceResponse));
service!(LibraryService;
    (list_library, ListLibraryRequest, ListLibraryResponse),
    (get_game, GetGameRequest, GetGameResponse),
    (list_versions, ListVersionsRequest, ListVersionsResponse),
    (get_version, GetVersionRequest, GetVersionResponse));
service!(DownloadService;
    (open_download, OpenDownloadRequest, OpenDownloadResponse),
    (refresh_ticket, RefreshTicketRequest, RefreshTicketResponse));

fn services(service: Arc<TestService>) -> connectrpc::Router {
    connectrpc::Router::new()
        .add_service::<_, AuthServiceRegisterMarker>(service.clone())
        .add_service::<_, DeviceServiceRegisterMarker>(service.clone())
        .add_service::<_, LibraryServiceRegisterMarker>(service.clone())
        .add_service::<_, DownloadServiceRegisterMarker>(service)
}

struct Running {
    addr: SocketAddr,
    task: tokio::task::JoinHandle<()>,
    decoded: Arc<AtomicUsize>,
    handlers: Arc<AtomicUsize>,
}
impl Drop for Running {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Running {
    async fn app(app: axum::Router, decoded: Arc<AtomicUsize>, handlers: Arc<AtomicUsize>) -> Self {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let addr = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        Self {
            addr,
            task,
            decoded,
            handlers,
        }
    }
    async fn new(policy: TestPolicy) -> Self {
        Self::timestamp(policy, 0, 0).await
    }
    async fn timestamp(policy: TestPolicy, seconds: i64, nanos: i32) -> Self {
        let handlers = Arc::new(AtomicUsize::new(0));
        let router = services(Arc::new(TestService {
            handlers: handlers.clone(),
            seconds,
            nanos,
        }));
        let server = Server::new(router, policy);
        let decoded = server.decoded.clone();
        Self::app(
            axum::Router::new().nest("/api/v2", server.into_axum_router()),
            decoded,
            handlers,
        )
        .await
    }
    fn counts(&self) -> (usize, usize) {
        (
            self.decoded.load(Ordering::SeqCst),
            self.handlers.load(Ordering::SeqCst),
        )
    }
    async fn call(&self, method: Method, json: bool, bytes: &[u8]) -> ReplyBody {
        exchange(
            self.addr,
            method.limit().path,
            "POST",
            if json {
                "application/json"
            } else {
                "application/proto"
            },
            "",
            bytes,
            false,
        )
        .await
    }
}

struct ReplyBody {
    status: u16,
    head: String,
    body: Vec<u8>,
}
impl ReplyBody {
    fn error(&self, code: Code, reason: Option<ErrorReason>) {
        assert_eq!(self.status, code.http_status(), "{}", self.head);
        assert!(
            self.head
                .to_ascii_lowercase()
                .contains("content-type: application/json")
        );
        let body = decode::decode_error_body(&self.body).unwrap();
        assert_eq!(body.code(), code);
        assert_eq!(body.wire().reason, reason);
    }
}

async fn exchange(
    addr: SocketAddr,
    path: &str,
    method: &str,
    content_type: &str,
    headers: &str,
    body: &[u8],
    chunked: bool,
) -> ReplyBody {
    tokio::time::timeout(Duration::from_secs(10), async {
        let mut socket = tokio::net::TcpStream::connect(addr).await.unwrap();
        let framing = if chunked { "Transfer-Encoding: chunked\r\n".into() } else { format!("Content-Length: {}\r\n", body.len()) };
        let head = format!("{method} /api/v2{path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\nContent-Type: {content_type}\r\nConnect-Protocol-Version: 1\r\n{framing}{headers}\r\n");
        socket.write_all(head.as_bytes()).await.unwrap();
        if chunked {
            for chunk in body.chunks(997) {
                socket.write_all(format!("{:x}\r\n", chunk.len()).as_bytes()).await.unwrap();
                socket.write_all(chunk).await.unwrap();
                socket.write_all(b"\r\n").await.unwrap();
            }
            socket.write_all(b"0\r\n\r\n").await.unwrap();
        } else { socket.write_all(body).await.unwrap(); }
        let mut response = Vec::new();
        socket.read_to_end(&mut response).await.unwrap();
        parse_reply(&response)
    }).await.unwrap()
}

fn parse_reply(response: &[u8]) -> ReplyBody {
    let split = response.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
    let head = String::from_utf8(response[..split].to_vec()).unwrap();
    let status = head.split_whitespace().nth(1).unwrap().parse().unwrap();
    let mut body = response[split + 4..].to_vec();
    if head
        .to_ascii_lowercase()
        .contains("transfer-encoding: chunked")
    {
        let mut decoded = Vec::new();
        let mut at = 0;
        loop {
            let end = body[at..].windows(2).position(|w| w == b"\r\n").unwrap() + at;
            let size =
                usize::from_str_radix(std::str::from_utf8(&body[at..end]).unwrap(), 16).unwrap();
            at = end + 2;
            if size == 0 {
                break;
            }
            decoded.extend_from_slice(&body[at..at + size]);
            at += size + 2;
        }
        body = decoded;
    }
    ReplyBody { status, head, body }
}

fn begin() -> Vec<u8> {
    BeginSessionRequest {
        device_id: DEVICE.into(),
        ..Default::default()
    }
    .encode_to_vec()
}
fn complete(scope: i32, capabilities: Vec<String>) -> CompleteSessionRequest {
    let key = crate::keys::SigningKey::<crate::keys::Device>::from_seed(&[19; 32]);
    let proof = crate::proof::issue_session(
        &crate::proof::SessionContext {
            instance_id: DEVICE,
            device_id: DEVICE,
            challenge: &[1; 32],
        },
        &key,
    )
    .unwrap();
    CompleteSessionRequest {
        device_id: DEVICE.into(),
        challenge: vec![1; 32],
        proof,
        scope: scope.into(),
        client_capabilities: capabilities,
        ..Default::default()
    }
}

#[test]
fn all_generated_methods_have_exact_contracts() {
    let service = Arc::new(TestService {
        handlers: Arc::new(AtomicUsize::new(0)),
        seconds: 0,
        nanos: 0,
    });
    let router = services(service);
    let methods: Vec<_> = router.methods().collect();
    assert_eq!(methods.len(), METHOD_LIMITS.len());
    assert_eq!(methods.len(), 13);
    for m in METHOD_LIMITS {
        assert!(methods.contains(&m.path.trim_start_matches('/')));
        assert_eq!(m.request_bytes, 65_536);
        assert_eq!(m.request.name(), format!("{:?}Request", m.method));
        assert_eq!(m.response.name(), format!("{:?}Response", m.method));
    }
}

#[test]
fn unregistered_table_methods_return_501_without_decode_or_handler() {
    run(async {
        let handlers = Arc::new(AtomicUsize::new(0));
        let service = Arc::new(TestService {
            handlers: handlers.clone(),
            seconds: 0,
            nanos: 0,
        });
        let router =
            connectrpc::Router::new().add_service::<_, DeviceServiceRegisterMarker>(service);
        let protected = Server::new(router, TestPolicy::default());
        let decoded = protected.decoded.clone();
        let server = Running::app(
            axum::Router::new().nest("/api/v2", protected.into_axum_router()),
            decoded,
            handlers,
        )
        .await;
        for json in [false, true] {
            let before = server.counts();
            let raw = if json {
                format!(r#"{{"deviceId":"{DEVICE}"}}"#).into_bytes()
            } else {
                begin()
            };
            server
                .call(Method::BeginSession, json, &raw)
                .await
                .error(Code::unimplemented, None);
            assert_eq!(server.counts(), before);
        }
        // The partial router still serves its registered methods normally.
        assert_eq!(
            server.call(Method::ListDevices, true, b"{}").await.status,
            200
        );
        assert_eq!(server.counts(), (1, 1));
    });
}

#[test]
fn every_service_uses_the_protected_path_in_both_codecs() {
    run(async {
        let server = Running::new(TestPolicy::default()).await;
        let key = crate::keys::SigningKey::<crate::keys::Device>::from_seed(&[19; 32]);
        macro_rules! call {
            ($method:ident, $request:expr) => {{
                let request = $request;
                let before = server.counts();
                assert_eq!(
                    server
                        .call(Method::$method, false, &request.encode_to_vec())
                        .await
                        .status,
                    200
                );
                assert_eq!(
                    server
                        .call(
                            Method::$method,
                            true,
                            &serde_json::to_vec(&request).unwrap()
                        )
                        .await
                        .status,
                    200
                );
                assert_eq!(server.counts(), (before.0 + 2, before.1 + 2));
            }};
        }
        call!(
            StartPairing,
            StartPairingRequest {
                device_name: "Test device".into(),
                platform: "linux".into(),
                arch: "x86_64".into(),
                public_key: key.public_key().to_vec(),
                client_version: "0.1.0".into(),
                ..Default::default()
            }
        );
        let pairing_proof = crate::proof::issue_pairing(
            &crate::proof::PairingContext {
                instance_id: DEVICE,
                pairing_id: &[1; 16],
            },
            &key,
        )
        .unwrap();
        call!(
            PollPairing,
            PollPairingRequest {
                pairing_id: vec![1; 16],
                proof: pairing_proof,
                ..Default::default()
            }
        );
        call!(
            BeginSession,
            BeginSessionRequest {
                device_id: DEVICE.into(),
                ..Default::default()
            }
        );
        call!(CompleteSession, complete(1, vec![]));
        call!(RevokeSelf, RevokeSelfRequest::default());
        call!(ListDevices, ListDevicesRequest::default());
        call!(
            RevokeDevice,
            RevokeDeviceRequest {
                device_id: DEVICE.into(),
                ..Default::default()
            }
        );
        call!(ListLibrary, ListLibraryRequest::default());
        call!(
            GetGame,
            GetGameRequest {
                game_id: DEVICE.into(),
                ..Default::default()
            }
        );
        call!(
            ListVersions,
            ListVersionsRequest {
                game_id: DEVICE.into(),
                ..Default::default()
            }
        );
        call!(
            GetVersion,
            GetVersionRequest {
                version_id: DEVICE.into(),
                ..Default::default()
            }
        );
        call!(
            OpenDownload,
            OpenDownloadRequest {
                version_id: DEVICE.into(),
                ..Default::default()
            }
        );
        call!(
            RefreshTicket,
            RefreshTicketRequest {
                version_id: DEVICE.into(),
                ..Default::default()
            }
        );
        assert_eq!(server.counts(), (26, 26));
    });
}

#[test]
fn unknown_enum_numbers_and_names_never_reach_decode_or_handler() {
    run(async {
        let server = Running::new(TestPolicy::default()).await;
        for scope in [0, 2, 99, -1, i32::MIN] {
            server
                .call(
                    Method::CompleteSession,
                    false,
                    &complete(scope, vec![]).encode_to_vec(),
                )
                .await
                .error(Code::invalid_argument, None);
        }
        for scope in ["0", "99", "-1", "\"SESSION_SCOPE_FUTURE\"", "null"] {
            let mut value = serde_json::to_value(complete(1, vec![])).unwrap();
            value["scope"] = serde_json::from_str(scope).unwrap();
            server
                .call(
                    Method::CompleteSession,
                    true,
                    &serde_json::to_vec(&value).unwrap(),
                )
                .await
                .error(Code::invalid_argument, None);
        }
        assert_eq!(server.counts(), (0, 0));
        let good = server
            .call(
                Method::CompleteSession,
                false,
                &complete(1, vec![]).encode_to_vec(),
            )
            .await;
        assert_eq!(good.status, 200);
        assert_eq!(server.counts(), (1, 1));
    });
}

#[test]
fn concurrent_and_subsequent_requests_keep_the_strict_profile() {
    run(async {
        let server = Running::new(TestPolicy::default()).await;
        let mut tasks = tokio::task::JoinSet::new();
        for i in 0..48 {
            let addr = server.addr;
            tasks.spawn(async move {
                let raw = if i % 2 == 0 {
                    format!(r#"{{"deviceId":"{DEVICE}"}}"#)
                } else {
                    format!(r#"{{"deviceId":"{DEVICE}","device_id":"{DEVICE}"}}"#)
                };
                let reply = exchange(
                    addr,
                    Method::BeginSession.limit().path,
                    "POST",
                    "application/json",
                    "",
                    raw.as_bytes(),
                    false,
                )
                .await;
                if i % 2 == 0 {
                    assert_eq!(reply.status, 200);
                } else {
                    reply.error(Code::invalid_argument, None);
                }
            });
        }
        while let Some(result) = tasks.join_next().await {
            result.unwrap();
        }
        assert_eq!(server.counts(), (24, 24));
        let raw = format!(r#"{{"deviceId":"{DEVICE}","device_id":"{DEVICE}"}}"#);
        server
            .call(Method::BeginSession, true, raw.as_bytes())
            .await
            .error(Code::invalid_argument, None);
        let response = server
            .call(
                Method::BeginSession,
                true,
                format!(r#"{{"deviceId":"{DEVICE}","futureScope":"UNKNOWN"}}"#).as_bytes(),
            )
            .await;
        assert_eq!(response.status, 200);
        assert!(
            response
                .head
                .to_ascii_lowercase()
                .contains("content-type: application/json")
        );
        api::decode_generic::<BeginSessionResponse>(Codec::Json, &response.body, ()).unwrap();
        assert_eq!(server.counts(), (25, 25));
    });
}

#[test]
fn byte_limits_count_chunked_json_wire_bytes_and_unknown_fields() {
    run(async {
        let server = Running::new(TestPolicy::default()).await;
        for chunked in [false, true] {
            for length in [65_535, 65_536, 65_537] {
                let mut raw = b"{}".to_vec();
                raw.resize(length, b' ');
                let before = server.counts();
                let response = exchange(
                    server.addr,
                    Method::RevokeSelf.limit().path,
                    "POST",
                    "application/json",
                    "",
                    &raw,
                    chunked,
                )
                .await;
                if length <= 65_536 {
                    assert_eq!(response.status, 200);
                    assert_eq!(server.counts(), (before.0 + 1, before.1 + 1));
                } else {
                    response.error(Code::resource_exhausted, None);
                    assert_eq!(server.counts(), before);
                }
            }
        }
        let flood = format!(
            r#"{{"future":{{{}}}}}"#,
            (0..5000)
                .map(|_| "\"same\":0")
                .collect::<Vec<_>>()
                .join(",")
        );
        let before = server.counts();
        server
            .call(Method::RevokeSelf, true, flood.as_bytes())
            .await
            .error(Code::invalid_argument, None);
        assert_eq!(server.counts(), before);
        let depth = format!("{{\"future\":{}0{}}}", "[".repeat(65), "]".repeat(65));
        server
            .call(Method::RevokeSelf, true, depth.as_bytes())
            .await
            .error(Code::resource_exhausted, None);
        assert_eq!(server.counts(), before);
        let escaped = format!(r#"{{"future":"{}"}}"#, "\\u0061".repeat(11_000));
        assert!(escaped.len() > 65_536);
        server
            .call(Method::RevokeSelf, true, escaped.as_bytes())
            .await
            .error(Code::resource_exhausted, None);
        assert_eq!(server.counts(), before);
        let plain = format!(r#"{{"future":"{}"}}"#, "a".repeat(11_000));
        assert_eq!(
            server
                .call(Method::RevokeSelf, true, plain.as_bytes())
                .await
                .status,
            200
        );
        assert_eq!(server.counts(), (before.0 + 1, before.1 + 1));
    });
}

#[test]
fn oversized_chunked_body_is_rejected_while_the_request_remains_open() {
    run(async {
        let server = Running::new(TestPolicy::default()).await;
        let before = server.counts();
        let response = tokio::time::timeout(Duration::from_secs(2), async {
            let mut socket = tokio::net::TcpStream::connect(server.addr).await.unwrap();
            let path = Method::RevokeSelf.limit().path;
            let head = format!(
                "POST /api/v2{path} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\nContent-Type: application/json\r\nConnect-Protocol-Version: 1\r\nTransfer-Encoding: chunked\r\n\r\n",
                server.addr
            );
            socket.write_all(head.as_bytes()).await.unwrap();
            let mut body = b"{}".to_vec();
            body.resize(65_536, b' ');
            socket.write_all(b"10000\r\n").await.unwrap();
            socket.write_all(&body).await.unwrap();
            socket.write_all(b"\r\n1\r\n \r\n").await.unwrap();
            // Exactly 65,537 body bytes have been sent. Do not send a zero
            // chunk or close the write half: waiting for request completion
            // must time out here, before the server's 30-second body timeout.
            let mut response = Vec::new();
            socket.read_to_end(&mut response).await.unwrap();
            parse_reply(&response)
        })
        .await
        .expect("oversized open request must be rejected within two seconds");
        response.error(Code::resource_exhausted, None);
        assert_eq!(server.counts(), before);
    });
}

#[test]
fn dispatch_size_token_profile_and_policy_order_precedes_handlers() {
    run(async {
        let server = Running::new(TestPolicy {
            deny_start: true,
            deny_token: true,
            ..Default::default()
        })
        .await;
        let oversized = vec![0; 65_537];
        exchange(
            server.addr,
            Method::StartPairing.limit().path,
            "GET",
            "application/proto",
            "Content-Encoding: gzip\r\n",
            &[],
            false,
        )
        .await
        .error(Code::unimplemented, None);
        exchange(
            server.addr,
            Method::StartPairing.limit().path,
            "POST",
            "application/proto",
            "Content-Encoding: gzip\r\n",
            &[],
            false,
        )
        .await
        .error(Code::unimplemented, None);
        server
            .call(Method::StartPairing, false, &oversized)
            .await
            .error(Code::resource_exhausted, Some(ErrorReason::RateLimited));
        server
            .call(Method::RevokeSelf, false, &oversized)
            .await
            .error(Code::resource_exhausted, None);
        server
            .call(Method::RevokeSelf, true, b"{")
            .await
            .error(Code::unauthenticated, None);
        assert_eq!(server.counts(), (0, 0));
        let server = Running::new(TestPolicy {
            deny_validation: true,
            ..Default::default()
        })
        .await;
        server
            .call(Method::BeginSession, false, &begin())
            .await
            .error(Code::unauthenticated, Some(ErrorReason::ChallengeInvalid));
        assert_eq!(server.counts(), (1, 0));
        let server = Running::new(TestPolicy::default()).await;
        let mut request = complete(1, vec![]);
        let last = request.proof.len() - 1;
        request.proof[last] ^= 1;
        server
            .call(Method::CompleteSession, false, &request.encode_to_vec())
            .await
            .error(Code::unauthenticated, Some(ErrorReason::ChallengeInvalid));
        assert_eq!(server.counts(), (1, 0));
    });
}

#[test]
fn required_capabilities_reject_missing_and_ignore_unknown_offers() {
    run(async {
        let server = Running::new(TestPolicy {
            required: vec!["events.v1".into()],
            ..Default::default()
        })
        .await;
        server
            .call(
                Method::CompleteSession,
                false,
                &complete(1, vec!["future.v99".into()]).encode_to_vec(),
            )
            .await
            .error(Code::failed_precondition, Some(ErrorReason::ClientTooOld));
        for json in [false, true] {
            let request = complete(1, vec!["future.v99".into(), "events.v1".into()]);
            let raw = if json {
                serde_json::to_vec(&request).unwrap()
            } else {
                request.encode_to_vec()
            };
            assert_eq!(
                server
                    .call(Method::CompleteSession, json, &raw)
                    .await
                    .status,
                200
            );
        }
        assert_eq!(server.counts(), (2, 2));
        assert_eq!(
            api::trust::accepted_capabilities(
                &["future.v99".into(), "events.v1".into()],
                &["events.v1".into()]
            ),
            vec!["events.v1"]
        );
    });
}

#[test]
fn timestamp_boundaries_are_checked_on_binary_and_json_transport_responses() {
    run(async {
        for (seconds, nanos, valid) in [
            (-62_135_596_800, 0, true),
            (253_402_300_799, 999_999_999, true),
            (-62_135_596_801, 0, false),
            (253_402_300_800, 0, false),
            (0, -1, false),
            (0, 1_000_000_000, false),
        ] {
            let server = Running::timestamp(TestPolicy::default(), seconds, nanos).await;
            for json in [false, true] {
                let raw = if json {
                    format!(r#"{{"deviceId":"{DEVICE}"}}"#).into_bytes()
                } else {
                    begin()
                };
                let response = server.call(Method::BeginSession, json, &raw).await;
                if valid {
                    assert_eq!(response.status, 200);
                    api::decode_generic::<BeginSessionResponse>(
                        if json { Codec::Json } else { Codec::Binary },
                        &response.body,
                        (),
                    )
                    .unwrap();
                } else {
                    response.error(Code::internal, None);
                }
            }
        }
    });
}

#[test]
fn bodyless_infrastructure_errors_are_preserved_outside_connect() {
    run(async {
        for status in [413, 429, 502] {
            let handlers = Arc::new(AtomicUsize::new(0));
            let router = services(Arc::new(TestService {
                handlers: handlers.clone(),
                seconds: 0,
                nanos: 0,
            }));
            let protected = Server::new(router, TestPolicy::default());
            let decoded = protected.decoded.clone();
            // Simulate a hosting HTTP layer before the protected Connect
            // entry, without involving a proxy or any external connection.
            let app = protected.into_axum_router().layer(middleware::from_fn(
                move |_: HttpRequest, _: HttpNext| async move {
                    (
                        axum::http::StatusCode::from_u16(status).unwrap(),
                        Body::empty(),
                    )
                        .into_response()
                },
            ));
            let server = Running::app(app, decoded, handlers).await;
            let response = server.call(Method::RevokeSelf, true, b"{}").await;
            assert_eq!(response.status, status);
            assert!(response.body.is_empty());
            assert!(
                !response
                    .head
                    .to_ascii_lowercase()
                    .contains("content-type: application/json")
            );
            assert_eq!(server.counts(), (0, 0));
        }
    });
}

#[test]
fn missing_guard_and_a_decoder_placed_before_it_fail_the_early_rejection_assertion() {
    run(async {
        for premature_decode in [false, true] {
            let decoded = Arc::new(AtomicUsize::new(0));
            let handlers = Arc::new(AtomicUsize::new(0));
            let policy = Arc::new(TestPolicy::default());
            let router = services(Arc::new(TestService {
                handlers: handlers.clone(),
                seconds: 0,
                nanos: 0,
            }));
            let mut service = ConnectRpcService::new(router);
            if premature_decode {
                let counter = decoded.clone();
                service =
                    service.with_interceptor(connectrpc::unary_interceptor(move |req, next| {
                        let counter = counter.clone();
                        Box::pin(async move {
                            counter.fetch_add(1, Ordering::SeqCst);
                            req.payload.message::<BeginSessionRequest>()?;
                            // Deliberately misplaced: the real raw parser now
                            // runs after a general-purpose binary decode.
                            api::project(
                                MessageKind::BeginSessionRequest,
                                Codec::Binary,
                                req.payload.bytes(),
                                InputLimit::body(65_536),
                            )
                            .map_err(|d| rpc_error(d, Context::BeginSession))?;
                            next.run(req).await
                        })
                    }));
            }
            service = service.with_interceptor(Semantic {
                policy,
                decoded: decoded.clone(),
            });
            let server = Running::app(
                axum::Router::new().nest("/api/v2", axum::Router::new().fallback_service(service)),
                decoded,
                handlers,
            )
            .await;
            // Binary runtimes normally merge duplicate scalar occurrences. The
            // profile must reject them before any such decode is attempted.
            let duplicate = [begin(), begin()].concat();
            let response = server.call(Method::BeginSession, false, &duplicate).await;
            response.error(
                if premature_decode {
                    Code::invalid_argument
                } else {
                    Code::internal
                },
                None,
            );
            let acceptance_contract = response.status == 400 && server.counts() == (0, 0);
            assert!(
                !acceptance_contract,
                "the negative control must be detected"
            );
            assert_eq!(server.counts(), (usize::from(premature_decode), 0));
        }
        let server = Running::new(TestPolicy::default()).await;
        server
            .call(Method::BeginSession, false, &[begin(), begin()].concat())
            .await
            .error(Code::invalid_argument, None);
        assert_eq!(server.counts(), (0, 0));
    });
}

#[test]
fn pinned_rpc_parser_vectors_run_through_the_complete_connect_path() {
    run(async {
        use corpus::{Corpus, array, expected, text};
        let corpus = Corpus::new();
        let server = Running::new(TestPolicy::default()).await;
        let mut rejected = 0;
        let mut flood = 0;
        for file in corpus.groups().into_iter().filter(|f| {
            f.starts_with("profile-")
                || f == "diagnostic-profile.json"
                || f == "large-limits-000.json"
        }) {
            let shard = corpus.pinned(&file);
            for case in array(&shard["cases"]) {
                let operation = text(&case["operation"]);
                let input = &case["input"];
                let Some(name) = input["message"].as_str() else {
                    continue;
                };
                let Some(method) = METHOD_LIMITS.iter().find(|m| m.request.name() == name) else {
                    continue;
                };
                let json = match operation {
                    "scan-json" | "json-precheck" => true,
                    "scan-binary" | "binary-precheck" => false,
                    _ => continue,
                };
                let fixed = expected(&corpus, &shard, case);
                let Some(reason) = fixed["reason"].as_str() else {
                    continue;
                };
                // A corpus-only artificial limit is not a route contract.
                if input["limit"]
                    .as_u64()
                    .is_some_and(|n| n != method.request_bytes as u64)
                {
                    continue;
                }
                let raw = corpus.bytes(&input[if json { "rawJsonHex" } else { "wireHex" }]);
                let before = server.counts();
                let response = server.call(method.method, json, &raw).await;
                let code = if raw.len() > method.request_bytes
                    || matches!(
                        reason,
                        "nesting_too_deep"
                            | "element_limit_exceeded"
                            | "byte_limit_exceeded"
                            | "body_too_large"
                    ) {
                    Code::resource_exhausted
                } else {
                    Code::invalid_argument
                };
                response.error(code, None);
                assert_eq!(server.counts(), before, "{}", text(&case["id"]));
                rejected += 1;
                if reason == "element_limit_exceeded" {
                    flood += 1;
                }
            }
        }
        assert!(
            rejected > 100,
            "RPC raw-wire coverage disappeared: {rejected}"
        );
        assert!(flood > 0, "RPC flood vectors must reach the guarded path");
    });
}

#[test]
fn oneof_null_vector_uses_the_same_preconversion_parser() {
    let corpus = corpus::Corpus::new();
    let shard = corpus.pinned("profile-009.json");
    let case = corpus::array(&shard["cases"])
        .iter()
        .find(|c| c["id"] == "json.oneof.null-member")
        .unwrap();
    let bytes = corpus.bytes(&case["input"]["rawJsonHex"]);
    // No unary request schema contains a oneof. Keep this probe on the shared
    // parser, rather than adding a fictitious RPC or widening a method schema.
    assert_eq!(
        api::project(
            MessageKind::ServerEvent,
            Codec::Json,
            &bytes,
            InputLimit::body(65_536)
        )
        .unwrap_err(),
        Diagnostic {
            reason: Reason::oneof_conflict,
            field: ""
        }
    );
}

#[test]
fn version_comparison_and_wire_catalog_reuse_part_one_contracts() {
    use api::trust::{ClientVersion, check_version};
    for (major, revision, minimum, expected) in [
        (2, 1, 1, None),
        (2, 99, 1, None),
        (3, 99, 99, Some(Reason::protocol_unsupported)),
        (1, 1, 1, Some(Reason::protocol_unsupported)),
        (2, 1, 2, Some(Reason::client_too_old)),
    ] {
        let server = ProtocolVersion {
            major,
            revision,
            min_client_revision: minimum,
            ..Default::default()
        };
        assert_eq!(
            check_version(&server, ClientVersion::default())
                .err()
                .map(|d| d.reason),
            expected
        );
    }
    let server = ProtocolVersion {
        major: 2,
        revision: 1,
        min_client_revision: 1,
        ..Default::default()
    };
    assert_eq!(
        check_version(
            &server,
            ClientVersion {
                min_server_revision: 2,
                ..Default::default()
            }
        )
        .unwrap_err()
        .reason,
        Reason::server_too_old
    );
    for reason in 1..=21 {
        let reason_enum = <ErrorReason as buffa::Enumeration>::from_i32(reason).unwrap();
        if let Some(errors::WireStatus::Active(code, _)) = errors::wire_catalog(reason) {
            let error = wire_error(WireError {
                code,
                reason: Some(reason_enum),
            });
            assert_eq!(error.http_status().as_u16(), code.http_status());
            let body = decode::decode_error_body(&error.to_json()).unwrap();
            assert_eq!(
                body.wire(),
                WireError {
                    code,
                    reason: Some(reason_enum)
                }
            );
        } else {
            assert_eq!(
                wire_error(WireError {
                    code: Code::failed_precondition,
                    reason: Some(reason_enum)
                })
                .http_status()
                .as_u16(),
                500
            );
        }
    }
    let error = rpc_error(
        Diagnostic {
            reason: Reason::invalid_uuid,
            field: "secret-field",
        },
        Context::General,
    );
    assert!(
        !std::str::from_utf8(&error.to_json())
            .unwrap()
            .contains("secret-field")
    );
}
