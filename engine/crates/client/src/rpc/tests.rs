// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: 0BSD

//! These tests invoke the real generated clients. The probe wraps the HTTP
//! body returned to Connect, so it fires before Connect's JSON conversion,
//! response view decode or error-detail decode, including for empty messages.

use std::{
    pin::Pin,
    sync::atomic::{AtomicUsize, Ordering},
    task::{Context, Poll},
};

use bytes::Bytes;
use http_body::{Body, Frame};
use impluvium_protocol::{
    connect::impluvium::v2::{AuthServiceClient, DeviceServiceClient, LibraryServiceClient},
    decode::{self, InputLimit},
    limits::MessageKind,
};

use super::*;

#[derive(Clone)]
struct Fixture {
    status: u16,
    content_type: &'static str,
    bytes: Vec<u8>,
    calls: Arc<AtomicUsize>,
    extra_headers: Vec<(&'static str, &'static str)>,
    measured_head: Option<u64>,
}
impl Fixture {
    fn success(codec: Codec, bytes: Vec<u8>) -> Self {
        Self {
            status: 200,
            content_type: match codec {
                Codec::Binary => "application/proto",
                Codec::Json => "application/json",
            },
            bytes,
            calls: Arc::new(AtomicUsize::new(0)),
            extra_headers: vec![],
            measured_head: None,
        }
    }
    fn error(status: u16, bytes: Vec<u8>) -> Self {
        Self {
            status,
            ..Self::success(Codec::Json, bytes)
        }
    }
}
impl ClientTransport for Fixture {
    type ResponseBody = ClientBody;
    type Error = Error;
    fn send(
        &self,
        _: Request<ClientBody>,
    ) -> BoxFuture<'static, Result<Response<ClientBody>, Error>> {
        let this = self.clone();
        Box::pin(async move {
            this.calls.fetch_add(1, Ordering::SeqCst);
            let mut response = Response::builder()
                .status(this.status)
                .header(header::CONTENT_TYPE, this.content_type)
                .body(full_body(this.bytes.into()))
                .unwrap();
            for (name, value) in this.extra_headers {
                response.headers_mut().append(
                    ::http::header::HeaderName::from_static(name),
                    ::http::HeaderValue::from_static(value),
                );
            }
            if let Some(size) = this.measured_head {
                response.extensions_mut().insert(MeasuredHead(size));
            }
            Ok(response)
        })
    }
}

#[derive(Clone)]
struct DecodeProbe<T> {
    inner: T,
    decodes: Arc<AtomicUsize>,
}
struct ProbeBody {
    inner: ClientBody,
    decodes: Arc<AtomicUsize>,
    entered: bool,
}
impl Body for ProbeBody {
    type Data = Bytes;
    type Error = connectrpc::ConnectError;
    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, Self::Error>>> {
        let this = self.get_mut();
        if !this.entered {
            this.entered = true;
            this.decodes.fetch_add(1, Ordering::SeqCst);
        }
        Pin::new(&mut this.inner).poll_frame(cx)
    }
}
impl<T> ClientTransport for DecodeProbe<T>
where
    T: ClientTransport<ResponseBody = ClientBody, Error = Error>,
{
    type ResponseBody = ProbeBody;
    type Error = Error;
    fn send(
        &self,
        request: Request<ClientBody>,
    ) -> BoxFuture<'static, Result<Response<ProbeBody>, Error>> {
        let inner = self.inner.clone();
        let decodes = self.decodes.clone();
        Box::pin(async move {
            Ok(inner.send(request).await?.map(|inner| ProbeBody {
                inner,
                decodes,
                entered: false,
            }))
        })
    }
}

fn settings() -> Settings {
    Settings::new(BaseUrl::parse("http://127.0.0.1/prefix").unwrap())
}
fn run<T>(future: impl Future<Output = T>) -> T {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(future)
}

async fn invoke<T>(
    transport: T,
    codec: Codec,
    method: Method,
) -> Result<(), connectrpc::ConnectError>
where
    T: ClientTransport,
    <T::ResponseBody as Body>::Error: std::fmt::Display,
{
    let config = settings().client_config(codec).unwrap();
    match method {
        Method::RevokeSelf => AuthServiceClient::new(transport, config)
            .revoke_self(Default::default())
            .await
            .map(|r| {
                let _ = r.into_owned();
            }),
        Method::BeginSession => AuthServiceClient::new(transport, config)
            .begin_session(Default::default())
            .await
            .map(|r| {
                let _ = r.into_owned();
            }),
        Method::CompleteSession => AuthServiceClient::new(transport, config)
            .complete_session(Default::default())
            .await
            .map(|r| {
                let _ = r.into_owned();
            }),
        Method::ListDevices => DeviceServiceClient::new(transport, config)
            .list_devices(Default::default())
            .await
            .map(|r| {
                let _ = r.into_owned();
            }),
        Method::GetGame => LibraryServiceClient::new(transport, config)
            .get_game(Default::default())
            .await
            .map(|r| {
                let _ = r.into_owned();
            }),
        Method::ListLibrary => LibraryServiceClient::new(transport, config)
            .list_library(Default::default())
            .await
            .map(|r| {
                let _ = r.into_owned();
            }),
        _ => unreachable!("test method must select its actual generated client"),
    }
}

fn rejected(fixture: Fixture, codec: Codec, method: Method, expected: Diagnostic) {
    let decodes = Arc::new(AtomicUsize::new(0));
    let transport = DecodeProbe {
        inner: ProtectedTransport::new(fixture, settings(), codec, true),
        decodes: decodes.clone(),
    };
    let result = run(invoke(transport, codec, method));
    assert!(early_rejection(&result, &decodes, expected));
    let error = result.unwrap_err();
    assert_eq!(diagnostic(&error), Some(expected), "{error}");
    assert_eq!(
        decodes.load(Ordering::SeqCst),
        0,
        "decoder entered before rejection"
    );
}
fn early_rejection(
    result: &Result<(), connectrpc::ConnectError>,
    decodes: &AtomicUsize,
    expected: Diagnostic,
) -> bool {
    result.as_ref().err().and_then(|error| diagnostic(error)) == Some(expected)
        && decodes.load(Ordering::SeqCst) == 0
}
fn d(reason: Reason, field: &'static str) -> Diagnostic {
    Diagnostic { reason, field }
}
fn varint(out: &mut Vec<u8>, mut n: usize) {
    loop {
        let byte = (n & 127) as u8;
        n >>= 7;
        out.push(byte | if n == 0 { 0 } else { 128 });
        if n == 0 {
            break;
        }
    }
}
fn len_field(out: &mut Vec<u8>, number: usize, bytes: &[u8]) {
    varint(out, (number << 3) | 2);
    varint(out, bytes.len());
    out.extend_from_slice(bytes);
}

#[test]
fn binary_profiles_precede_the_actual_generated_decoder() {
    let cases = [
        (Method::ListDevices, vec![0], d(Reason::malformed_wire, "")),
        (
            Method::ListDevices,
            vec![0x08, 0],
            d(Reason::wrong_wire_type, "ListDevicesResponse.devices"),
        ),
        (
            Method::ListDevices,
            vec![0x12, 0, 0x12, 0],
            d(
                Reason::duplicate_field,
                "ListDevicesResponse.next_page_token",
            ),
        ),
        (
            Method::GetGame,
            vec![0x0a, 0, 0x0a, 0],
            d(Reason::split_message, "GetGameResponse.game"),
        ),
        (
            Method::BeginSession,
            vec![0x12, 0, 0x12, 0],
            d(Reason::split_message, "BeginSessionResponse.expires_at"),
        ),
        (
            Method::BeginSession,
            vec![0x12, 6, 0x10, 0x80, 0x94, 0xeb, 0xdc, 0x03],
            d(Reason::invalid_timestamp, "BeginSessionResponse.expires_at"),
        ),
    ];
    for (method, bytes, expected) in cases {
        rejected(
            Fixture::success(Codec::Binary, bytes),
            Codec::Binary,
            method,
            expected,
        );
    }
}

#[test]
fn element_floods_inside_the_body_budget_never_reach_connect() {
    rejected(
        Fixture::success(Codec::Binary, [0x0a, 0].repeat(201)),
        Codec::Binary,
        Method::ListDevices,
        d(
            Reason::element_limit_exceeded,
            "ListDevicesResponse.devices",
        ),
    );
    let json = format!("{{\"devices\":[{}]}}", vec!["{}"; 201].join(","));
    rejected(
        Fixture::success(Codec::Json, json.into_bytes()),
        Codec::Json,
        Method::ListDevices,
        d(
            Reason::element_limit_exceeded,
            "ListDevicesResponse.devices",
        ),
    );
    let mut packed = vec![];
    len_field(&mut packed, 1, b"00000000-0000-4000-8000-000000000001");
    len_field(&mut packed, 2, b"Game");
    len_field(&mut packed, 3, &[1; 5]);
    packed.extend_from_slice(&[0x18, 1, 0x18, 1, 0x18, 1, 0x18, 1]);
    let mut response = vec![];
    len_field(&mut response, 1, &packed);
    rejected(
        Fixture::success(Codec::Binary, response),
        Codec::Binary,
        Method::GetGame,
        d(Reason::element_limit_exceeded, "GameSummary.platforms"),
    );
}

#[test]
fn json_names_unknown_subtrees_and_aliases_are_checked_before_serde() {
    let cases = [
        (
            br#"{"nextPageToken":"","next_page_token":""}"#.to_vec(),
            Reason::duplicate_field,
            "ListDevicesResponse.next_page_token",
        ),
        (
            br#"{"nextPageToken":null,"next_page_token":null}"#.to_vec(),
            Reason::duplicate_field,
            "ListDevicesResponse.next_page_token",
        ),
        (
            br#"{"devices":[],"\u0064evices":[]}"#.to_vec(),
            Reason::duplicate_json_key,
            "",
        ),
        (
            br#"{"unknown":{"x":1,"\u0078":2}}"#.to_vec(),
            Reason::duplicate_json_key,
            "",
        ),
        (
            br#"{"devices":[null]}"#.to_vec(),
            Reason::json_type_mismatch,
            "ListDevicesResponse.devices",
        ),
        (
            br#"{"nextPageToken":"\ud800"}"#.to_vec(),
            Reason::invalid_json,
            "ListDevicesResponse.next_page_token",
        ),
        (
            br#"{"devices":true}"#.to_vec(),
            Reason::json_type_mismatch,
            "ListDevicesResponse.devices",
        ),
    ];
    for (bytes, reason, field) in cases {
        rejected(
            Fixture::success(Codec::Json, bytes),
            Codec::Json,
            Method::ListDevices,
            d(reason, field),
        );
    }
    let nested = format!("{{\"ignored\":{}0{}}}", "[".repeat(64), "]".repeat(64));
    rejected(
        Fixture::success(Codec::Json, nested.into_bytes()),
        Codec::Json,
        Method::ListDevices,
        d(Reason::nesting_too_deep, ""),
    );
}

#[test]
fn hostile_errors_and_binary_details_are_rejected_before_connect_error_decoding() {
    let mut cases = vec![
        br#"{"code":"unknown","code":"unauthenticated"}"#.to_vec(),
        br#"{"code":"unknown","details":[{"type":"future.Detail","value":"!"}]}"#.to_vec(),
        // Duplicate ErrorInfo.reason, including an explicitly encoded default.
        br#"{"code":"unauthenticated","details":[{"type":"impluvium.v2.ErrorInfo","value":"CAAIAA=="}]}"#.to_vec(),
        br#"{"code":"unknown","details":[{"type":"impluvium.v2.ErrorInfo","value":""},{"type":"impluvium.v2.ErrorInfo","value":""}]}"#.to_vec(),
        format!("{{\"code\":\"unknown\",\"details\":[{}]}}", vec![r#"{"type":"future.Detail","value":""}"#; 17].join(",")).into_bytes(),
        format!(r#"{{"code":"unknown","message":"{}"}}"#, "x".repeat(4097)).into_bytes(),
        format!(r#"{{"code":"unknown","details":[{{"type":"future.Detail","value":"{}"}}]}}"#, "AAAA".repeat(2731)).into_bytes(),
    ];
    for duplicate_key in [false, true] {
        let mut info = vec![];
        for index in 0..33 {
            let mut entry = vec![];
            let key = if duplicate_key {
                "same".into()
            } else {
                format!("k{index}")
            };
            len_field(&mut entry, 1, key.as_bytes());
            len_field(&mut entry, 2, b"value");
            len_field(&mut info, 2, &entry);
        }
        let base64 = crate::credentials::ticket_header(&info).unwrap();
        cases.push(format!(r#"{{"code":"unauthenticated","details":[{{"type":"impluvium.v2.ErrorInfo","value":"{}"}}]}}"#,
            base64.strip_prefix("Bearer ").unwrap()).into_bytes());
    }
    for codec in [Codec::Binary, Codec::Json] {
        for bytes in &cases {
            rejected(
                Fixture::error(401, bytes.clone()),
                codec,
                Method::RevokeSelf,
                d(Reason::broken_error_body, ""),
            );
        }
        rejected(
            Fixture::error(429, cases[0].clone()),
            codec,
            Method::RevokeSelf,
            d(Reason::rate_limited, ""),
        );
        rejected(
            Fixture::error(503, cases[0].clone()),
            codec,
            Method::RevokeSelf,
            d(Reason::server_unavailable, ""),
        );
    }
}

#[test]
fn http_rejections_have_precedence_and_do_not_enter_any_decoder() {
    for codec in [Codec::Binary, Codec::Json] {
        let mut redirect = Fixture::error(302, vec![]);
        redirect.extra_headers.push(("content-encoding", "gzip"));
        rejected(
            redirect,
            codec,
            Method::RevokeSelf,
            d(Reason::redirect_not_allowed, ""),
        );
        let mut coded = Fixture::error(503, vec![]);
        coded.extra_headers.push(("content-encoding", "gzip"));
        rejected(
            coded,
            codec,
            Method::RevokeSelf,
            d(Reason::unsupported_content_encoding, ""),
        );
        let mut wrong_type = Fixture::error(413, vec![]);
        wrong_type.content_type = "text/html";
        rejected(
            wrong_type,
            codec,
            Method::RevokeSelf,
            d(Reason::request_too_large, ""),
        );
        rejected(
            Fixture::success(codec, vec![0; 65_537]),
            codec,
            Method::RevokeSelf,
            d(Reason::response_too_large, ""),
        );
        rejected(
            Fixture::error(401, vec![b' '; 65_537]),
            codec,
            Method::RevokeSelf,
            d(Reason::broken_error_body, ""),
        );
        let mut huge_head = Fixture::error(503, vec![]);
        huge_head.measured_head = Some(65_537);
        rejected(
            huge_head,
            codec,
            Method::RevokeSelf,
            d(Reason::headers_too_large, ""),
        );
    }
}

#[test]
fn field_byte_limits_and_semantics_precede_the_external_decoder() {
    let mut binary = vec![];
    len_field(&mut binary, 2, &[b'a'; 1025]);
    rejected(
        Fixture::success(Codec::Binary, binary),
        Codec::Binary,
        Method::ListDevices,
        d(
            Reason::byte_limit_exceeded,
            "ListDevicesResponse.next_page_token",
        ),
    );
    rejected(
        Fixture::success(
            Codec::Json,
            format!(r#"{{"nextPageToken":"{}"}}"#, "a".repeat(1025)).into_bytes(),
        ),
        Codec::Json,
        Method::ListDevices,
        d(
            Reason::byte_limit_exceeded,
            "ListDevicesResponse.next_page_token",
        ),
    );
    rejected(Fixture::success(Codec::Json,
        br#"{"devices":[{"deviceId":"00000000-0000-4000-8000-000000000001","name":"bad\u0000name"}]}"#.to_vec()),
        Codec::Json, Method::ListDevices, d(Reason::invalid_text, "Device.name"));
}

#[test]
fn response_identity_is_bound_to_the_actual_generated_request() {
    let decodes = Arc::new(AtomicUsize::new(0));
    let fixture = Fixture::success(
        Codec::Json,
        br#"{"game":{"gameId":"00000000-0000-4000-8000-000000000002","name":"Game"}}"#.to_vec(),
    );
    let transport = DecodeProbe {
        inner: ProtectedTransport::new(fixture, settings(), Codec::Json, true),
        decodes: decodes.clone(),
    };
    let client =
        LibraryServiceClient::new(transport, settings().client_config(Codec::Json).unwrap());
    let error = run(
        client.get_game(impluvium_protocol::proto::impluvium::v2::GetGameRequest {
            game_id: "00000000-0000-4000-8000-000000000001".into(),
            ..Default::default()
        }),
    )
    .unwrap_err();
    assert_eq!(
        diagnostic(&error),
        Some(d(Reason::response_identity_mismatch, "GameSummary.game_id"))
    );
    assert_eq!(decodes.load(Ordering::SeqCst), 0);
}

#[test]
fn valid_boundaries_and_normal_errors_enter_the_real_decoders() {
    for codec in [Codec::Binary, Codec::Json] {
        let mut devices = vec![];
        let bytes = match codec {
            Codec::Binary => {
                let mut device = vec![];
                len_field(&mut device, 1, b"00000000-0000-4000-8000-000000000001");
                len_field(&mut device, 2, b"Device");
                len_field(&mut device, 3, b"linux");
                len_field(&mut device, 4, b"x86_64");
                len_field(&mut device, 5, b"1.0.0");
                len_field(&mut device, 6, &[]);
                len_field(&mut device, 7, &[]);
                for _ in 0..200 { len_field(&mut devices, 1, &device); }
                devices
            }
            Codec::Json => format!(r#"{{"devices":[{}]}}"#, vec![r#"{"deviceId":"00000000-0000-4000-8000-000000000001","name":"Device","platform":"linux","arch":"x86_64","clientVersion":"1.0.0","createdAt":"1970-01-01T00:00:00Z","lastSeenAt":"1970-01-01T00:00:00Z"}"#; 200].join(",")).into_bytes(),
        };
        let decodes = Arc::new(AtomicUsize::new(0));
        let probe = DecodeProbe {
            inner: ProtectedTransport::new(Fixture::success(codec, bytes), settings(), codec, true),
            decodes: decodes.clone(),
        };
        run(invoke(probe, codec, Method::ListDevices)).unwrap();
        assert_eq!(decodes.load(Ordering::SeqCst), 1);

        let normal = br#"{"code":"unauthenticated","message":"expired","details":[{"type":"impluvium.v2.ErrorInfo","value":"CAU="}]}"#.to_vec();
        let probe = DecodeProbe {
            inner: ProtectedTransport::new(Fixture::error(401, normal), settings(), codec, true),
            decodes: decodes.clone(),
        };
        let error = run(invoke(probe, codec, Method::RevokeSelf)).unwrap_err();
        assert_eq!(error.code, connectrpc::ErrorCode::Unauthenticated);
        assert!(diagnostic(&error).is_none());
        assert_eq!(error.details.len(), 1);
        assert_eq!(decodes.load(Ordering::SeqCst), 2);
    }
}

#[test]
fn exact_byte_limit_and_each_json_name_alone_are_accepted() {
    for codec in [Codec::Binary, Codec::Json] {
        let bytes = match codec {
            Codec::Binary => {
                let mut out = vec![];
                let padding = vec![0; 65_531];
                len_field(&mut out, 100, &padding);
                out
            }
            Codec::Json => {
                let mut out = b"{}".to_vec();
                out.resize(65_536, b' ');
                out
            }
        };
        assert_eq!(bytes.len(), 65_536);
        let decodes = Arc::new(AtomicUsize::new(0));
        let mut fixture = Fixture::success(codec, bytes);
        fixture.measured_head = Some(65_536);
        let probe = DecodeProbe {
            inner: ProtectedTransport::new(fixture, settings(), codec, true),
            decodes: decodes.clone(),
        };
        run(invoke(probe, codec, Method::RevokeSelf)).unwrap();
        assert_eq!(decodes.load(Ordering::SeqCst), 1);
    }
    for name in ["nextPageToken", "next_page_token"] {
        let fixture = Fixture::success(
            Codec::Json,
            format!(r#"{{"{name}":"opaque"}}"#).into_bytes(),
        );
        let transport = ProtectedTransport::new(fixture, settings(), Codec::Json, true);
        run(invoke(transport, Codec::Json, Method::ListDevices)).unwrap();
    }
}

#[test]
fn removing_the_guard_or_moving_it_after_decode_is_detected() {
    // Buffa can merge these two empty messages. The raw profile forbids it.
    let hostile = Fixture::success(Codec::Binary, vec![0x0a, 0, 0x0a, 0]);
    rejected(
        hostile.clone(),
        Codec::Binary,
        Method::GetGame,
        d(Reason::split_message, "GetGameResponse.game"),
    );
    for check_after_decode in [false, true] {
        let decodes = Arc::new(AtomicUsize::new(0));
        let probe = DecodeProbe {
            inner: hostile.clone(),
            decodes: decodes.clone(),
        };
        let mut result = run(invoke(probe, Codec::Binary, Method::GetGame));
        let late_rejection = check_after_decode
            && decode::scan_binary(
                MessageKind::GetGameResponse,
                &hostile.bytes,
                InputLimit::response(4_194_304),
            )
            .is_err();
        if late_rejection {
            result = Err(connectrpc::ConnectError::unavailable_from_transport(
                "late response check",
                Error::Rejected(d(Reason::split_message, "GetGameResponse.game")),
            ));
        }
        // A late rejection cannot repair the already crossed decode boundary.
        assert!(!early_rejection(
            &result,
            &decodes,
            d(Reason::split_message, "GetGameResponse.game")
        ));
        assert_eq!(decodes.load(Ordering::SeqCst), 1);
        assert_eq!(late_rejection, check_after_decode);
    }
}

#[test]
fn oneof_conflicts_use_the_same_predecode_profile_for_both_codecs() {
    // RPC responses in this schema have no oneof. Events do; do not invent a
    // oneof on a generated RPC type just to make this test pass through it.
    for bytes in [vec![0x52, 0, 0x5a, 0], vec![0x52, 0, 0x52, 0]] {
        assert_eq!(
            decode::scan_binary(
                MessageKind::ServerEvent,
                &bytes,
                InputLimit::response(65_536)
            ),
            Err(d(Reason::oneof_conflict, ""))
        );
    }
    assert_eq!(
        decode::scan_json(
            MessageKind::ServerEvent,
            br#"{"connected":null,"heartbeat":{}}"#,
            InputLimit::response(65_536)
        ),
        Err(d(Reason::oneof_conflict, ""))
    );
    assert_eq!(
        decode::scan_json(
            MessageKind::ServerEvent,
            br#"{"resyncRequired":null,"resync_required":null}"#,
            InputLimit::response(65_536)
        ),
        Err(d(Reason::oneof_conflict, ""))
    );
}

#[test]
fn preflight_rechecks_the_generated_destination_and_platform_before_io() {
    for (platform_available, uri, reason) in [
        (
            false,
            "http://elsewhere.invalid/api/v2",
            Reason::platform_trust_unavailable,
        ),
        (
            true,
            "http://elsewhere.invalid/api/v2",
            Reason::insecure_scheme_not_allowed,
        ),
        (
            true,
            "https://elsewhere.invalid/api/v2",
            Reason::origin_not_allowed,
        ),
        (
            true,
            "http://127.0.0.1/other/api/v2",
            Reason::origin_not_allowed,
        ),
    ] {
        let fixture = Fixture::success(Codec::Binary, vec![]);
        let calls = fixture.calls.clone();
        let transport =
            ProtectedTransport::new(fixture, settings(), Codec::Binary, platform_available);
        let config = ClientConfig::new(uri.parse().unwrap());
        let client = AuthServiceClient::new(transport, config);
        let error = run(client.revoke_self(Default::default())).unwrap_err();
        assert_eq!(diagnostic(&error), Some(d(reason, "")));
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }
}

#[test]
fn oversized_generated_request_is_rejected_before_io() {
    let fixture = Fixture::success(Codec::Binary, vec![]);
    let calls = fixture.calls.clone();
    let decodes = Arc::new(AtomicUsize::new(0));
    let transport = DecodeProbe {
        inner: ProtectedTransport::new(fixture, settings(), Codec::Binary, true),
        decodes: decodes.clone(),
    };
    let client =
        DeviceServiceClient::new(transport, settings().client_config(Codec::Binary).unwrap());
    let error = run(client.list_devices(
        impluvium_protocol::proto::impluvium::v2::ListDevicesRequest {
            page_token: "x".repeat(65_537),
            ..Default::default()
        },
    ))
    .unwrap_err();
    assert_eq!(diagnostic(&error), Some(d(Reason::body_too_large, "")));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(decodes.load(Ordering::SeqCst), 0);
}
