// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: 0BSD

//! Real loopback TCP/TLS requests; no injected verification decision or evidence.

#![cfg(target_os = "linux")]

use std::{
    convert::Infallible,
    future::Future,
    io::{Read, Write},
    net::TcpListener,
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

use bytes::Bytes;
use impluvium_client::{
    credentials::{Context, CredentialPurpose, CredentialSource, Identity, PairingContext},
    error::Reason,
    http::Http,
    release::Payload,
    response::{Binding, Chunk, Outcome},
    rpc::{Error, Settings},
    transport::{BaseUrl, DepotTargets, Method, TrustState},
    trust::{Platform, Tls},
};
use impluvium_protocol::{
    api::{
        Codec,
        trust::{ClientVersion, Pin},
    },
    hash::Algorithm,
};
use rustls::{ServerConfig, ServerConnection, StreamOwned, pki_types::PrivatePkcs8KeyDer};
use serde_json::{Value, json};

const INSTANCE: &str = "33333333-3333-3333-3333-333333333333";
const TOKEN: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
const FINGERPRINT: &str = "If4x36FUomFia/hUBG/SJxt77UtqvkWqWId+9H+XIbk=";
const DEADLINE: Duration = Duration::from_secs(5);

fn pin() -> Pin {
    Pin::from_stored(
        INSTANCE.into(),
        [
            0xd7, 0x5a, 0x98, 0x01, 0x82, 0xb1, 0x0a, 0xb7, 0xd5, 0x4b, 0xfe, 0xd3, 0xc9, 0x64,
            0x07, 0x3a, 0x0e, 0xe1, 0x72, 0xf3, 0xda, 0xa6, 0x23, 0x25, 0xaf, 0x02, 0x1a, 0x68,
            0xf7, 0x07, 0x51, 0x1a,
        ],
    )
    .unwrap()
}

fn discovery() -> Value {
    json!({
        "discovery": 1, "product": "impluvium", "serverName": "Loopback",
        "serverVersion": "test", "instanceId": INSTANCE,
        "sourceUrl": "https://source.invalid/repo", "manifestFormats": [3],
        "protocol": { "major": 2, "revision": 1, "minClientRevision": 1 },
        "keyFingerprints": [{ "role": "KEY_ROLE_CONTENT", "sha256": FINGERPRINT }]
    })
}

fn response(status: u16, content_type: &str, body: &[u8]) -> Vec<u8> {
    let mut bytes = format!(
        "HTTP/1.1 {status} Test\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\n\r\n",
        body.len(),
    )
    .into_bytes();
    bytes.extend_from_slice(body);
    bytes
}

fn discovery_response(value: &Value) -> Vec<u8> {
    response(200, "application/json", &serde_json::to_vec(value).unwrap())
}

#[derive(Default)]
struct Source {
    reads: Vec<CredentialPurpose>,
    deletes: usize,
}
impl CredentialSource for Source {
    type Error = Infallible;
    type Credential = String;
    fn read(&mut self, _: Identity<'_>, purpose: CredentialPurpose) -> Result<String, Infallible> {
        self.reads.push(purpose);
        Ok(match purpose {
            CredentialPurpose::AccessToken => TOKEN.into(),
            CredentialPurpose::Proof => "proof-secret".into(),
            _ => panic!("unexpected credential read"),
        })
    }
    fn delete(&mut self, _: &Pin) -> Result<(), Infallible> {
        self.deletes += 1;
        Ok(())
    }
}

fn token(credential: Option<String>) -> Result<Payload, Error> {
    Ok(Payload::AccessToken {
        token: credential.unwrap(),
        body: Bytes::from_static(b"{}"),
    })
}

fn proof(credential: Option<String>) -> Result<Payload, Error> {
    // The proof signer is represented by the source's marker. The real send
    // path must defer this callback just as it defers an actual signing handle.
    let marker = credential.unwrap();
    Ok(Payload::Proof(Bytes::from(format!(
        "{{\"proof\":\"{marker}\"}}"
    ))))
}

fn run<T>(future: impl Future<Output = T>) -> T {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(future)
}

#[derive(Debug)]
struct Record {
    head: String,
    body: Vec<u8>,
}

fn read_request(stream: &mut impl Read) -> Option<Record> {
    let mut head = Vec::new();
    while !head.ends_with(b"\r\n\r\n") {
        let mut byte = [0];
        match stream.read(&mut byte) {
            Ok(0) => {
                assert!(head.is_empty(), "incomplete request");
                return None;
            }
            Ok(_) => head.push(byte[0]),
            Err(e)
                if head.is_empty()
                    && matches!(
                        e.kind(),
                        std::io::ErrorKind::UnexpectedEof | std::io::ErrorKind::ConnectionReset
                    ) =>
            {
                return None;
            }
            Err(e) => panic!("request read failed: {e}"),
        }
        assert!(head.len() < 65_536);
    }
    let head = String::from_utf8(head).unwrap().to_ascii_lowercase();
    assert!(!head.contains("transfer-encoding:"));
    let length = head
        .lines()
        .find_map(|line| line.strip_prefix("content-length:"))
        .map_or(0, |v| v.trim().parse().unwrap());
    let mut body = vec![0; length];
    stream.read_exact(&mut body).unwrap();
    Some(Record { head, body })
}

struct Server {
    base: BaseUrl,
    tls: Tls,
    cert: rustls::pki_types::CertificateDer<'static>,
    thread: thread::JoinHandle<Vec<Vec<Record>>>,
}
impl Server {
    /// Each script belongs to a distinct accepted TCP/TLS connection. Reading
    /// until EOF also detects any unexpected extra authenticated request.
    fn new(scripts: Vec<Vec<Vec<u8>>>) -> Self {
        let certified = rcgen::generate_simple_self_signed(vec!["127.0.0.1".into()]).unwrap();
        let cert = certified.cert.der().clone();
        let tls = Tls::new(Platform::Linux, vec![cert.clone()]).unwrap();
        let key = PrivatePkcs8KeyDer::from(certified.key_pair.serialize_der());
        let config = Arc::new(
            ServerConfig::builder()
                .with_no_client_auth()
                .with_single_cert(vec![cert.clone()], key.into())
                .unwrap(),
        );
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let base = BaseUrl::parse(&format!(
            "https://{}/prefix",
            listener.local_addr().unwrap()
        ))
        .unwrap();
        let thread = thread::spawn(move || {
            let workers: Vec<_> = scripts
                .into_iter()
                .map(|script| {
                    let deadline = Instant::now() + DEADLINE;
                    let socket = loop {
                        match listener.accept() {
                            Ok((socket, _)) => break socket,
                            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                                assert!(
                                    Instant::now() < deadline,
                                    "expected connection was not opened"
                                );
                                thread::sleep(Duration::from_millis(1));
                            }
                            Err(e) => panic!("accept failed: {e}"),
                        }
                    };
                    socket.set_read_timeout(Some(DEADLINE)).unwrap();
                    socket.set_write_timeout(Some(DEADLINE)).unwrap();
                    let config = config.clone();
                    thread::spawn(move || {
                        let mut stream =
                            StreamOwned::new(ServerConnection::new(config).unwrap(), socket);
                        let mut records = Vec::new();
                        while let Some(record) = read_request(&mut stream) {
                            let reply = script
                                .get(records.len())
                                .expect("unexpected request on physical connection");
                            records.push(record);
                            stream.write_all(reply).unwrap();
                            stream.flush().unwrap();
                            if reply
                                .windows(19)
                                .any(|w| w.eq_ignore_ascii_case(b"connection: close\r\n"))
                            {
                                break;
                            }
                        }
                        assert_eq!(records.len(), script.len());
                        records
                    })
                })
                .collect();
            workers
                .into_iter()
                .map(|worker| worker.join().unwrap())
                .collect()
        });
        Self {
            base,
            tls,
            cert,
            thread,
        }
    }
    fn http(&self) -> Http {
        Http::new(&self.tls, "impluvium-test/0.1.0 (linux; x86_64)", DEADLINE).unwrap()
    }
    fn finish(self) -> Vec<Vec<Record>> {
        self.thread.join().unwrap()
    }
}

fn assert_discovery(record: &Record) {
    assert!(record.head.starts_with("get /prefix/api/info "));
    assert!(!record.head.contains("authorization:"));
    assert!(!record.head.contains("cookie:"));
    assert!(record.body.is_empty());
}

#[test]
fn tls_credentials_follow_discovery_on_each_physical_connection() {
    let server = Server::new(vec![
        vec![
            discovery_response(&discovery()),
            response(200, "application/json", b"{}"),
        ],
        vec![
            discovery_response(&discovery()),
            response(200, "application/proto", b""),
        ],
    ]);
    let mut context = Context::new(server.base.clone(), pin());
    let http = server.http();
    let settings = Settings::new(server.base.clone());
    let mut source = Source::default();
    run(async {
        for codec in [Codec::Json, Codec::Binary] {
            let mut connection = http
                .credential_connection(settings.clone(), &mut context)
                .unwrap();
            assert!(matches!(
                connection
                    .rpc(Method::RevokeSelf, Codec::Json, &mut source, |_| panic!(
                        "unverified callback"
                    ))
                    .await,
                Err(Error::CredentialsRequireVerifiedConnection)
            ));
            connection
                .verify(ClientVersion::default(), &[])
                .await
                .unwrap();
            let reply = connection
                .rpc(Method::RevokeSelf, codec, &mut source, |credential| {
                    let body = match codec {
                        Codec::Json => Bytes::from_static(b"{}"),
                        Codec::Binary => Bytes::new(),
                    };
                    Ok(Payload::AccessToken {
                        token: credential.unwrap(),
                        body,
                    })
                })
                .await
                .unwrap();
            assert!(matches!(reply.outcome(), Outcome::Success(_)));
        }
    });
    assert_eq!(source.reads, vec![CredentialPurpose::AccessToken; 2]);
    assert_eq!(source.deletes, 0);
    assert_eq!(context.pin(), &pin());
    for records in server.finish() {
        assert_eq!(records.len(), 2);
        assert_discovery(&records[0]);
        assert!(records[1].head.contains(&format!(
            "authorization: bearer {}\r\n",
            TOKEN.to_ascii_lowercase()
        )));
    }
}

#[test]
fn api_document_get_checks_destination_before_reading_the_token() {
    let server = Server::new(vec![vec![
        discovery_response(&discovery()),
        response(200, "text/plain", b"data"),
    ]]);
    let foreign = TcpListener::bind("127.0.0.1:0").unwrap();
    foreign.set_nonblocking(true).unwrap();
    let foreign_url = format!(
        "https://{}/prefix/api/v2/objects/{INSTANCE}",
        foreign.local_addr().unwrap()
    );
    let url = format!("{}/api/v2/objects/{INSTANCE}", server.base.serialized());
    let http = server.http();
    let mut context = Context::new(server.base.clone(), pin());
    let mut source = Source::default();
    run(async {
        let mut connection = http
            .credential_connection(Settings::new(server.base.clone()), &mut context)
            .unwrap();
        assert!(
            matches!(connection.get(&foreign_url, Binding::object(4, "text/plain"), &mut source).await,
            Err(Error::Rejected(d)) if d.reason == Reason::origin_not_allowed)
        );
        assert!(source.reads.is_empty());
        connection
            .verify(ClientVersion::default(), &[])
            .await
            .unwrap();
        assert!(
            matches!(connection.get(&foreign_url, Binding::object(4, "text/plain"), &mut source).await,
            Err(Error::Rejected(d)) if d.reason == Reason::origin_not_allowed)
        );
        assert!(source.reads.is_empty());
        let reply = connection
            .get(&url, Binding::object(4, "text/plain"), &mut source)
            .await
            .unwrap();
        let Outcome::Success(body) = reply.into_outcome() else {
            panic!("expected object");
        };
        assert_eq!(body.bytes(), b"data");
    });
    assert!(matches!(foreign.accept(), Err(e) if e.kind() == std::io::ErrorKind::WouldBlock));
    assert_eq!(source.reads, vec![CredentialPurpose::AccessToken]);
    let logs = server.finish();
    assert_discovery(&logs[0][0]);
    assert!(
        logs[0][1]
            .head
            .starts_with(&format!("get /prefix/api/v2/objects/{INSTANCE} "))
    );
    assert!(logs[0][1].head.contains("authorization: bearer aaa"));
}

#[test]
fn changed_peer_key_fingerprint_and_major_preserve_pin_and_credentials() {
    let mut different_key = [0x66; 32];
    different_key[0] = 0x58;
    let different_pin = Pin::from_stored(INSTANCE.into(), different_key).unwrap();
    let different_fingerprint = impluvium_client::credentials::ticket_header(
        &Algorithm::Sha256.digest(different_pin.content_public_key()),
    )
    .unwrap();
    for (field, value, expected, stopped) in [
        (
            "instanceId",
            json!("44444444-4444-4444-4444-444444444444"),
            Reason::instance_mismatch,
            true,
        ),
        // A different advertised content key and a damaged fingerprint both
        // fail comparison, without treating a fingerprint as a replacement key.
        (
            "keyFingerprints",
            json!([{ "role": "KEY_ROLE_CONTENT", "sha256": different_fingerprint.strip_prefix("Bearer ").unwrap() }]),
            Reason::content_key_changed,
            true,
        ),
        (
            "keyFingerprints",
            json!([{ "role": "KEY_ROLE_CONTENT", "sha256": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=" }]),
            Reason::content_key_changed,
            true,
        ),
        (
            "protocol",
            json!({ "major": 1, "revision": 1, "minClientRevision": 1 }),
            Reason::protocol_unsupported,
            false,
        ),
    ] {
        let mut changed = discovery();
        changed[field] = value;
        let server = Server::new(vec![
            vec![discovery_response(&discovery())],
            vec![discovery_response(&changed)],
        ]);
        let http = server.http();
        let settings = Settings::new(server.base.clone());
        let mut context = Context::new(server.base.clone(), pin());
        let mut source = Source::default();
        run(async {
            let mut first = http
                .credential_connection(settings.clone(), &mut context)
                .unwrap();
            first.verify(ClientVersion::default(), &[]).await.unwrap();
            drop(first);
            let mut replacement = http
                .credential_connection(settings.clone(), &mut context)
                .unwrap();
            let error = replacement
                .verify(ClientVersion::default(), &[])
                .await
                .unwrap_err();
            assert!(matches!(error, Error::Rejected(d) if d.reason == expected));
            let error = replacement
                .rpc(Method::RevokeSelf, Codec::Json, &mut source, |_| {
                    panic!("rejected callback")
                })
                .await;
            if stopped {
                assert!(matches!(error, Err(Error::Rejected(d)) if d.reason == expected));
            } else {
                assert!(matches!(
                    error,
                    Err(Error::CredentialsRequireVerifiedConnection)
                ));
            }
        });
        assert_eq!(context.pin(), &pin());
        assert_eq!(source.deletes, 0);
        assert!(source.reads.is_empty());
        assert_eq!(context.trust_state() != TrustState::Trusted, stopped);
        for records in server.finish() {
            assert_eq!(records.len(), 1);
            assert_discovery(&records[0]);
        }
    }
}

#[test]
fn session_and_pairing_proofs_use_only_their_verified_connection() {
    let error = response(401, "application/json", br#"{"code":"unauthenticated"}"#);
    let server = Server::new(vec![
        vec![discovery_response(&discovery()), error.clone()],
        vec![discovery_response(&discovery()), error],
    ]);
    let http = server.http();
    let settings = Settings::new(server.base.clone());
    let mut context = Context::new(server.base.clone(), pin());
    let mut pairing = PairingContext::new(server.base.clone());
    let mut source = Source::default();
    run(async {
        let mut session = http
            .credential_connection(settings.clone(), &mut context)
            .unwrap();
        assert!(matches!(
            session
                .rpc(
                    Method::CompleteSession,
                    Codec::Json,
                    &mut source,
                    |_| panic!("early signer")
                )
                .await,
            Err(Error::CredentialsRequireVerifiedConnection)
        ));
        session.verify(ClientVersion::default(), &[]).await.unwrap();
        let reply = session
            .rpc(Method::CompleteSession, Codec::Json, &mut source, proof)
            .await
            .unwrap();
        session.cleanup(&reply, &mut source).unwrap();
        drop(session);
        let mut connection = http.pairing_connection(settings, &mut pairing).unwrap();
        assert!(matches!(
            connection
                .rpc(Method::PollPairing, Codec::Json, &mut source, |_| panic!(
                    "early signer"
                ))
                .await,
            Err(Error::CredentialsRequireVerifiedConnection)
        ));
        connection
            .verify(ClientVersion::default(), &[])
            .await
            .unwrap();
        connection
            .rpc(Method::PollPairing, Codec::Json, &mut source, proof)
            .await
            .unwrap();
        assert!(
            matches!(connection.rpc(Method::RevokeSelf, Codec::Json, &mut source, |_| panic!("pairing token read")).await,
            Err(Error::Rejected(d)) if d.reason == Reason::origin_not_allowed)
        );
    });
    assert_eq!(source.reads, vec![CredentialPurpose::Proof; 2]);
    assert_eq!(source.deletes, 0);
    for records in server.finish() {
        assert_discovery(&records[0]);
        assert!(!records[1].head.contains("authorization:"));
        assert_eq!(records[1].body, br#"{"proof":"proof-secret"}"#);
    }
}

#[test]
fn a_closed_connection_never_reconnects_or_replays_credentials() {
    let mut closing = discovery_response(&discovery());
    let position = closing.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
    drop(closing.splice(position..position, b"\r\nConnection: close".iter().copied()));
    let server = Server::new(vec![vec![closing]]);
    let http = server.http();
    let mut context = Context::new(server.base.clone(), pin());
    let mut source = Source::default();
    run(async {
        let mut connection = http
            .credential_connection(Settings::new(server.base.clone()), &mut context)
            .unwrap();
        connection
            .verify(ClientVersion::default(), &[])
            .await
            .unwrap();
        assert!(
            connection
                .rpc(Method::RevokeSelf, Codec::Json, &mut source, token)
                .await
                .is_err()
        );
        let reads = source.reads.len();
        assert!(matches!(
            connection
                .rpc(Method::RevokeSelf, Codec::Json, &mut source, |_| panic!(
                    "replay"
                ))
                .await,
            Err(Error::CredentialsRequireVerifiedConnection)
        ));
        assert_eq!(source.reads.len(), reads);
        assert!(
            connection
                .verify(ClientVersion::default(), &[])
                .await
                .is_err()
        );
    });
    assert_eq!(source.deletes, 0);
    let logs = server.finish();
    assert_eq!(logs[0].len(), 1);
    assert_discovery(&logs[0][0]);
}

#[test]
fn depot_get_uses_only_proven_ticket_and_rejects_api_purposes() {
    for separate_origin in [false, true] {
        let refresh = br#"{"ticket":"dGlja2V0","manifestIds":["AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="]}"#;
        let depot_error = response(401, "application/json", br#"{"code":"unauthenticated","details":[{"type":"impluvium.v2.ErrorInfo","value":"CAU"}]}"#);
        let mut scripts = vec![vec![
            discovery_response(&discovery()),
            response(200, "application/json", refresh),
        ]];
        if !separate_origin {
            scripts.push(vec![depot_error.clone()]);
        }
        let server = Server::new(scripts);
        let depot = separate_origin.then(|| Server::new(vec![vec![depot_error]]));
        let mut roots = vec![server.cert.clone()];
        if let Some(depot) = &depot {
            roots.push(depot.cert.clone());
        }
        let tls = Tls::new(Platform::Linux, roots).unwrap();
        let http = Http::new(&tls, "impluvium-test/0.1.0 (linux; x86_64)", DEADLINE).unwrap();
        let settings = Settings::new(server.base.clone());
        let depot_base = depot.as_ref().map_or(&server.base, |depot| &depot.base);
        let url = format!(
            "{}/depot/v2/{}/{}",
            depot_base.serialized(),
            "00".repeat(32),
            "00".repeat(32)
        );
        let depots = DepotTargets::from_allowed_urls(&[&url]).unwrap();
        let mut context = Context::new(server.base.clone(), pin());
        let mut source = Source::default();
        run(async {
            let mut connection = http.credential_connection(settings, &mut context).unwrap();
            connection
                .verify(ClientVersion::default(), &[])
                .await
                .unwrap();
            let reply = connection
                .rpc(Method::RefreshTicket, Codec::Json, &mut source, token)
                .await
                .unwrap();
            let (_, ticket) = connection.refresh_response(&reply).unwrap();
            let ticket = ticket.unwrap();
            assert!(
                connection
                    .depot_get(
                        &depots,
                        &url,
                        Binding::rpc(Method::RevokeSelf, Codec::Json),
                        &ticket
                    )
                    .await
                    .is_err()
            );
            let binding = Binding::chunk(Chunk {
                size: 1,
                id: [0; 32],
                algorithm: Algorithm::Sha256,
                range: None,
            })
            .unwrap();
            assert!(
                matches!(connection.depot_get(&DepotTargets::default(), &url, binding.clone(), &ticket).await,
                Err(Error::Rejected(d)) if d.reason == Reason::origin_not_allowed)
            );
            let outcome = connection
                .depot_get(&depots, &url, binding, &ticket)
                .await
                .unwrap();
            assert!(matches!(outcome, Outcome::Error(_)));
            connection.cleanup(&reply, &mut source).unwrap();
        });
        assert_eq!(source.reads, vec![CredentialPurpose::AccessToken]);
        assert_eq!(source.deletes, 0);
        let logs = server.finish();
        assert_discovery(&logs[0][0]);
        assert!(logs[0][1].head.contains("authorization: bearer aaa"));
        let external_logs = depot.map(Server::finish);
        let record = match &external_logs {
            Some(depot_logs) => &depot_logs[0][0],
            None => &logs[1][0],
        };
        assert!(record.head.contains("authorization: bearer dglja2v0\r\n"));
        assert!(!record.head.contains(&TOKEN.to_ascii_lowercase()));
    }
}

#[test]
fn cleanup_requires_a_verified_api_reply_and_the_specified_reason() {
    let replies = ["CAQ", "CAU", "CAY"].map(|value| {
        response(
            401,
            "application/json",
            &serde_json::to_vec(&json!({"code": "unauthenticated", "details": [{
                "type": "impluvium.v2.ErrorInfo", "value": value,
            }]}))
            .unwrap(),
        )
    });
    let mut script = vec![discovery_response(&discovery())];
    script.extend(replies);
    let server = Server::new(vec![script]);
    let http = server.http();
    let mut context = Context::new(server.base.clone(), pin());
    let mut source = Source::default();
    run(async {
        let mut connection = http
            .credential_connection(Settings::new(server.base.clone()), &mut context)
            .unwrap();
        connection
            .verify(ClientVersion::default(), &[])
            .await
            .unwrap();
        for expected in [0, 1, 2] {
            let before = source.deletes;
            let reply = connection
                .rpc(Method::RevokeSelf, Codec::Json, &mut source, token)
                .await
                .unwrap();
            assert_eq!(source.deletes, before);
            connection.cleanup(&reply, &mut source).unwrap();
            assert_eq!(source.deletes, expected);
        }
    });
    assert_eq!(context.pin(), &pin());
    assert_eq!(server.finish()[0].len(), 4);
}

#[test]
fn http2_goaway_never_replaces_the_verified_sender() {
    use http_body_util::{BodyExt, Full};
    use hyper::{Response, service::service_fn};
    use hyper_util::rt::{TokioExecutor, TokioIo};
    use std::{
        sync::{
            Mutex,
            atomic::{AtomicBool, Ordering},
        },
        task::Poll,
    };

    run(async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = BaseUrl::parse(&format!("http://{}", listener.local_addr().unwrap())).unwrap();
        let records = Arc::new(Mutex::new(Vec::new()));
        let done = Arc::new(AtomicBool::new(false));
        let server_records = records.clone();
        let server_done = done.clone();
        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let service = service_fn(move |request: hyper::Request<hyper::body::Incoming>| {
                let records = server_records.clone();
                let done = server_done.clone();
                async move {
                    assert!(!request.headers().contains_key("authorization"));
                    records
                        .lock()
                        .unwrap()
                        .push(request.uri().path().to_owned());
                    assert!(
                        request
                            .into_body()
                            .collect()
                            .await
                            .unwrap()
                            .to_bytes()
                            .is_empty()
                    );
                    done.store(true, Ordering::SeqCst);
                    Ok::<_, Infallible>(
                        Response::builder()
                            .header("content-type", "application/json")
                            .body(Full::new(Bytes::from(
                                serde_json::to_vec(&discovery()).unwrap(),
                            )))
                            .unwrap(),
                    )
                }
            });
            let builder = hyper::server::conn::http2::Builder::new(TokioExecutor::new());
            let mut connection = Box::pin(builder.serve_connection(TokioIo::new(socket), service));
            let mut stopping = false;
            std::future::poll_fn(|cx| {
                if done.load(Ordering::SeqCst) && !stopping {
                    connection.as_mut().graceful_shutdown();
                    stopping = true;
                }
                // An immediate second poll observes a service that just returned
                // discovery and initiates GOAWAY before accepting another stream.
                match connection.as_mut().poll(cx) {
                    Poll::Pending if done.load(Ordering::SeqCst) && !stopping => {
                        cx.waker().wake_by_ref();
                        Poll::Pending
                    }
                    result => result,
                }
            })
            .await
            .unwrap();
            assert!(
                matches!(listener.into_std().unwrap().accept(), Err(e) if e.kind() == std::io::ErrorKind::WouldBlock)
            );
        });
        let tls = Tls::new(Platform::Linux, vec![]).unwrap();
        let http = Http::new(&tls, "impluvium-test/0.1.0 (linux; x86_64)", DEADLINE)
            .unwrap()
            .http2_prior_knowledge();
        let mut context = Context::new(base.clone(), pin());
        let mut source = Source::default();
        let mut connection = http
            .credential_connection(Settings::new(base), &mut context)
            .unwrap();
        connection
            .verify(ClientVersion::default(), &[])
            .await
            .unwrap();
        tokio::time::timeout(DEADLINE, server)
            .await
            .unwrap()
            .unwrap();
        assert!(
            connection
                .rpc(Method::RevokeSelf, Codec::Json, &mut source, token)
                .await
                .is_err()
        );
        assert!(matches!(
            connection
                .rpc(
                    Method::CompleteSession,
                    Codec::Json,
                    &mut source,
                    |_| panic!("GOAWAY proof replay")
                )
                .await,
            Err(Error::CredentialsRequireVerifiedConnection)
        ));
        assert_eq!(*records.lock().unwrap(), vec!["/api/info"]);
        assert_eq!(source.deletes, 0);
    });
}

#[test]
fn canceling_discovery_drops_its_sender_without_releasing_a_credential() {
    use http_body_util::{BodyExt, Full};
    use hyper::{Response, service::service_fn};
    use hyper_util::rt::TokioIo;
    use std::{
        sync::atomic::{AtomicBool, Ordering},
        task::Poll,
    };

    run(async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = BaseUrl::parse(&format!("http://{}", listener.local_addr().unwrap())).unwrap();
        let observed = Arc::new(AtomicBool::new(false));
        let server_observed = observed.clone();
        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let service = service_fn(move |request: hyper::Request<hyper::body::Incoming>| {
                let observed = server_observed.clone();
                async move {
                    assert_eq!(request.uri().path(), "/api/info");
                    assert!(!request.headers().contains_key("authorization"));
                    assert!(
                        request
                            .into_body()
                            .collect()
                            .await
                            .unwrap()
                            .to_bytes()
                            .is_empty()
                    );
                    observed.store(true, Ordering::SeqCst);
                    // Keep discovery incomplete until the client cancels it.
                    std::future::pending::<Result<Response<Full<Bytes>>, Infallible>>().await
                }
            });
            let _ = hyper::server::conn::http1::Builder::new()
                .serve_connection(TokioIo::new(socket), service)
                .await;
        });
        let tls = Tls::new(Platform::Linux, vec![]).unwrap();
        let http = Http::new(&tls, "impluvium-test/0.1.0 (linux; x86_64)", DEADLINE).unwrap();
        let mut context = Context::new(base.clone(), pin());
        let mut source = Source::default();
        let mut connection = http
            .credential_connection(Settings::new(base), &mut context)
            .unwrap();
        let mut verification = Box::pin(connection.verify(ClientVersion::default(), &[]));
        let expires = Instant::now() + DEADLINE;
        loop {
            std::future::poll_fn(|cx| {
                assert!(verification.as_mut().poll(cx).is_pending());
                Poll::Ready(())
            })
            .await;
            if observed.load(Ordering::SeqCst) {
                break;
            }
            assert!(
                Instant::now() < expires,
                "discovery did not reach the server"
            );
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        drop(verification);
        assert!(matches!(
            connection
                .rpc(
                    Method::CompleteSession,
                    Codec::Json,
                    &mut source,
                    |_| panic!("canceled signer")
                )
                .await,
            Err(Error::CredentialsRequireVerifiedConnection)
        ));
        assert!(
            connection
                .verify(ClientVersion::default(), &[])
                .await
                .is_err()
        );
        assert!(source.reads.is_empty());
        assert_eq!(source.deletes, 0);
        server.abort();
    });
}
