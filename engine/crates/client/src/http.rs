// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: 0BSD

//! Direct network ingress using the shared OS TLS configuration. Hyper's
//! HTTP/1 parser bounds every raw head; HTTP/2 has its own header-list limit.
//! No proxy, redirect, retry or decompression service is installed.

use std::{
    error::Error as StdError,
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};

use ::http::{Request, Response, header};
use bytes::Bytes;
use connectrpc::client::{BoxFuture, ClientBody, ClientTransport, full_body};
use http_body::{Body, Frame};
use http_body_util::BodyExt;
use hyper_rustls::{HttpsConnector, HttpsConnectorBuilder};
use hyper_util::{
    client::legacy::connect::{Connection, HttpConnector},
    rt::TokioExecutor,
};
use impluvium_protocol::api::Codec;
use tokio::time::{Instant, Sleep};
use tower_service::Service;

use crate::{
    credentials::CredentialPurpose,
    error::{Reason, local},
    response::{self, Binding, HeadDecision, Outcome},
    rpc::{Error, ProtectedTransport, Settings},
    transport::{self, Method, Origin, Policy},
    trust::Tls,
};

#[derive(Clone)]
pub struct Http {
    connector: HttpsConnector<HttpConnector>,
    user_agent: ::http::HeaderValue,
    pub(crate) deadline: Duration,
    http2_prior_knowledge: bool,
}
impl Http {
    /// The same total deadline covers DNS, TCP, TLS, response heads and body.
    /// Connections are direct and never carry credentials in this adapter.
    pub fn new(tls: &Tls, user_agent: &str, deadline: Duration) -> Result<Self, Error> {
        if deadline.is_zero() {
            return Err(Error::UnsupportedRequest);
        }
        let user_agent = user_agent
            .parse::<::http::HeaderValue>()
            .map_err(Error::transport)?;
        let mut config = tls.config().as_ref().clone();
        // hyper-rustls installs the enabled protocols itself, retaining the
        // same provider, verifier, protocol versions and certificate roots.
        config.alpn_protocols.clear();
        let connector = HttpsConnectorBuilder::new()
            .with_tls_config(config)
            .https_or_http()
            .enable_http1()
            .enable_http2()
            .build();
        Ok(Self {
            connector,
            user_agent,
            deadline,
            http2_prior_knowledge: false,
        })
    }

    /// Select HTTP/2 prior knowledge for cleartext servers that support h2c.
    /// HTTPS continues to select its protocol through the shared TLS ALPN.
    pub fn http2_prior_knowledge(mut self) -> Self {
        self.http2_prior_knowledge = true;
        self
    }

    pub fn rpc(&self, settings: Settings, codec: Codec) -> ProtectedTransport<ReqwestTransport> {
        ProtectedTransport::new(
            ReqwestTransport { http: self.clone() },
            settings,
            codec,
            true,
        )
    }

    /// Open one physical connection, without sending an HTTP request. The
    /// caller must complete public preflight before invoking this operation.
    pub(crate) async fn open_physical(&self, uri: ::http::Uri) -> Result<Physical, Error> {
        let mut connector = self.connector.clone();
        std::future::poll_fn(|cx| connector.poll_ready(cx))
            .await
            .map_err(|error| Error::Transport(error.into()))?;
        let io = connector
            .call(uri.clone())
            .await
            .map_err(|error| Error::Transport(error.into()))?;
        let http2 = io.connected().is_negotiated_h2()
            || (uri.scheme_str() == Some("http") && self.http2_prior_knowledge);
        let origin = Origin::of(&transport::request_url(&uri.to_string())?);
        let (sender, task) = if http2 {
            let mut builder = hyper::client::conn::http2::Builder::new(TokioExecutor::new());
            // h2 rejects >= its configured maximum; the local inclusive
            // 65536-byte check is independent of the advertised setting.
            builder.max_header_list_size(response::HEAD_LIMIT as u32 + 1);
            let (sender, connection) = builder.handshake(io).await.map_err(network_error)?;
            let task = ConnectionTask(tokio::spawn(async move {
                let _ = connection.await;
            }));
            (Sender::Http2(sender), task)
        } else {
            let mut builder = hyper::client::conn::http1::Builder::new();
            builder
                .max_header_size(response::HEAD_LIMIT)
                .max_buf_size(response::HEAD_LIMIT + 1)
                .max_headers(response::HEAD_LIMIT / 4);
            let (sender, connection) = builder.handshake(io).await.map_err(network_error)?;
            let task = ConnectionTask(tokio::spawn(async move {
                let _ = connection.await;
            }));
            (Sender::Http1(sender), task)
        };
        Ok(Physical {
            sender,
            origin,
            _task: task,
        })
    }

    /// Credential-free GET ingress. Redirect targets pass fresh preflight.
    /// SSE needs a separate receiver with a budget for each event.
    pub async fn get(
        &self,
        policy: &Policy<'_>,
        url: &str,
        binding: Binding,
    ) -> Result<Outcome, Error> {
        let original = transport::preflight(policy, url, binding)?;
        if matches!(
            original.purpose(),
            transport::Purpose::Rpc(_) | transport::Purpose::Events
        ) {
            return Err(Error::UnsupportedRequest);
        }
        let mut prepared = original.clone();
        let mut followed = 0;
        loop {
            let request = Request::builder()
                .method(::http::Method::GET)
                .uri(prepared.url().as_str())
                .body(full_body(Bytes::new()))
                .map_err(Error::transport)?;
            let mut incoming = self.send(request).await?;
            let fields = crate::rpc::headers(incoming.headers())?;
            let measured = crate::rpc::parsed_head_size(incoming.version(), incoming.headers());
            match response::head(
                prepared.binding(),
                measured,
                incoming.status().as_u16(),
                &fields,
            )? {
                HeadDecision::Redirect(location) => {
                    prepared = transport::redirect(
                        policy,
                        &original,
                        prepared.url(),
                        followed,
                        &location,
                    )?;
                    followed += 1;
                }
                HeadDecision::Body(mut reader) => {
                    while let Some(frame) = incoming.body_mut().frame().await {
                        let frame = frame?;
                        if let Ok(chunk) = frame.into_data() {
                            reader.push(&chunk)?;
                        }
                    }
                    return reader.finish().map_err(Into::into);
                }
                HeadDecision::Events | HeadDecision::Informational => {
                    return Err(Error::UnsupportedRequest);
                }
            }
        }
    }

    async fn send(&self, request: Request<ClientBody>) -> Result<Response<ReqwestBody>, Error> {
        // This gate precedes DNS, connection establishment and request-body
        // polling. TrustState::Trusted is not physical-connection evidence.
        credential_free(&request)?;
        let expires = Instant::now() + self.deadline;
        tokio::time::timeout_at(expires, async {
            let mut physical = self.open_physical(request.uri().clone()).await?;
            let (parts, body) = request.into_parts();
            let bytes = http_body_util::Limited::new(body, 65_536)
                .collect()
                .await
                .map_err(|_| Error::Rejected(local(Reason::body_too_large)))?
                .to_bytes();
            let incoming = physical
                .send(self, Request::from_parts(parts, full_body(bytes)))
                .await?;
            Ok(incoming.map(|inner| ReqwestBody {
                inner,
                task: Some(physical._task),
                timer: Box::pin(tokio::time::sleep_until(expires)),
                finished: false,
            }))
        })
        .await
        .map_err(Error::transport)?
    }
}

/// Preserve the public transport type name while using a bounded Hyper parser
/// beneath the shared response guard. Construct only through Http::rpc.
#[derive(Clone)]
pub struct ReqwestTransport {
    http: Http,
}
impl ClientTransport for ReqwestTransport {
    type ResponseBody = ReqwestBody;
    type Error = Error;

    fn send(
        &self,
        request: Request<ClientBody>,
    ) -> BoxFuture<'static, Result<Response<ReqwestBody>, Error>> {
        let http = self.http.clone();
        Box::pin(async move { http.send(request).await })
    }
}

pub(crate) fn credential_free(request: &Request<ClientBody>) -> Result<(), Error> {
    // HeaderMap compares names case-insensitively. Presence rejects empty
    // values and repeated credential fields without interpreting their values.
    let credential_header = [
        header::AUTHORIZATION,
        header::PROXY_AUTHORIZATION,
        header::COOKIE,
    ]
    .iter()
    .any(|name| request.headers().contains_key(name));
    let proof_method = [Method::PollPairing, Method::CompleteSession]
        .iter()
        .any(|method| {
            debug_assert_eq!(method.credential(), CredentialPurpose::Proof);
            request
                .uri()
                .path()
                .ends_with(&format!("/{}", method.route()))
        });
    if credential_header || proof_method {
        Err(Error::CredentialsRequireVerifiedConnection)
    } else {
        Ok(())
    }
}

enum Sender {
    Http1(hyper::client::conn::http1::SendRequest<ClientBody>),
    Http2(hyper::client::conn::http2::SendRequest<ClientBody>),
}

/// This sender has no connector, pool, retry or replacement operation. Keeping
/// its driver alive preserves exactly the TCP/TLS session that carried discovery.
pub(crate) struct Physical {
    sender: Sender,
    origin: Origin,
    _task: ConnectionTask,
}
impl Physical {
    pub(crate) fn is_closed(&self) -> bool {
        match &self.sender {
            Sender::Http1(sender) => sender.is_closed(),
            Sender::Http2(sender) => sender.is_closed(),
        }
    }

    pub(crate) async fn send(
        &mut self,
        http: &Http,
        mut request: Request<ClientBody>,
    ) -> Result<Response<hyper::body::Incoming>, Error> {
        let uri = request.uri().clone();
        if Origin::of(&transport::request_url(&uri.to_string())?) != self.origin {
            return Err(local(Reason::origin_not_allowed).into());
        }
        request
            .headers_mut()
            .entry(header::USER_AGENT)
            .or_insert(http.user_agent.clone());
        request.headers_mut().insert(
            header::ACCEPT_ENCODING,
            ::http::HeaderValue::from_static("identity"),
        );
        request.headers_mut().insert(
            header::HOST,
            uri.authority()
                .ok_or(Error::UnsupportedRequest)?
                .as_str()
                .parse()
                .map_err(Error::transport)?,
        );
        let incoming = match &mut self.sender {
            Sender::Http1(sender) => {
                sender.ready().await.map_err(network_error)?;
                *request.version_mut() = ::http::Version::HTTP_11;
                *request.uri_mut() = uri
                    .path_and_query()
                    .map_or("/", |p| p.as_str())
                    .parse()
                    .map_err(Error::transport)?;
                sender.send_request(request).await.map_err(network_error)?
            }
            Sender::Http2(sender) => {
                sender.ready().await.map_err(network_error)?;
                *request.version_mut() = ::http::Version::HTTP_2;
                sender.send_request(request).await.map_err(network_error)?
            }
        };
        response::check_head_size(crate::rpc::parsed_head_size(
            incoming.version(),
            incoming.headers(),
        ))?;
        Ok(incoming)
    }
}

fn network_error(error: hyper::Error) -> Error {
    let mut cause: Option<&(dyn StdError + 'static)> = Some(&error);
    while let Some(current) = cause {
        if current
            .downcast_ref::<hyper::Error>()
            .is_some_and(hyper::Error::is_parse_too_large)
        {
            return local(Reason::headers_too_large).into();
        }
        cause = current.source();
    }
    Error::transport(error)
}

struct ConnectionTask(tokio::task::JoinHandle<()>);
impl Drop for ConnectionTask {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// A streaming body retaining both its connection driver and total deadline.
pub struct ReqwestBody {
    inner: hyper::body::Incoming,
    task: Option<ConnectionTask>,
    timer: Pin<Box<Sleep>>,
    finished: bool,
}
impl Body for ReqwestBody {
    type Data = Bytes;
    type Error = Error;

    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, Self::Error>>> {
        let this = self.get_mut();
        if this.finished {
            return Poll::Ready(None);
        }
        if this.timer.as_mut().poll(cx).is_ready() {
            this.finished = true;
            this.task = None;
            return Poll::Ready(Some(Err(Error::transport(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "HTTP response deadline exceeded",
            )))));
        }
        match std::task::ready!(Pin::new(&mut this.inner).poll_frame(cx)) {
            Some(Ok(frame)) => Poll::Ready(Some(Ok(frame))),
            Some(Err(error)) => {
                this.finished = true;
                this.task = None;
                Poll::Ready(Some(Err(network_error(error))))
            }
            None => {
                this.finished = true;
                this.task = None;
                Poll::Ready(None)
            }
        }
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use std::{
        io::{Read, Write},
        net::TcpListener,
        thread,
        time::Instant,
    };

    use impluvium_protocol::connect::impluvium::v2::AuthServiceClient;

    use super::*;
    use crate::{rpc, transport::BaseUrl, trust::Platform};

    fn server(response: Vec<u8>) -> (BaseUrl, thread::JoinHandle<Vec<u8>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base =
            BaseUrl::parse(&format!("http://{}/prefix", listener.local_addr().unwrap())).unwrap();
        listener.set_nonblocking(true).unwrap();
        let server = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(5);
            let mut socket = loop {
                match listener.accept() {
                    Ok((socket, _)) => break socket,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(Instant::now() < deadline, "client did not connect");
                        thread::sleep(Duration::from_millis(1));
                    }
                    Err(error) => panic!("accept failed: {error}"),
                }
            };
            socket
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut request = vec![];
            let mut buffer = [0; 1024];
            loop {
                let n = socket.read(&mut buffer).unwrap();
                assert_ne!(n, 0);
                request.extend_from_slice(&buffer[..n]);
                if let Some(end) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                    let head = String::from_utf8_lossy(&request[..end]).to_ascii_lowercase();
                    let length = head
                        .lines()
                        .find_map(|line| line.strip_prefix("content-length:"))
                        .map_or(0, |s| s.trim().parse::<usize>().unwrap());
                    if request.len() >= end + 4 + length {
                        break;
                    }
                }
            }
            // An oversized response may be abandoned before the final write.
            let _ = socket.write_all(&response);
            request
        });
        (base, server)
    }

    fn call(raw: Vec<u8>, codec: Codec) -> (Result<(), connectrpc::ConnectError>, Vec<u8>) {
        let tls = Tls::new(Platform::Linux, vec![]).unwrap();
        let http = Http::new(
            &tls,
            "impluvium-test/0.1.0 (linux; x86_64)",
            Duration::from_secs(5),
        )
        .unwrap();
        let (base, server) = server(raw);
        let transport = http.rpc(Settings::new(base), codec);
        let config = transport.client_config().unwrap();
        let client = AuthServiceClient::new(transport, config);
        let result = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(client.revoke_self(Default::default()))
            .map(|r| {
                let _ = r.into_owned();
            });
        (result, server.join().unwrap())
    }

    #[test]
    fn reqwest_streams_chunked_bodies_through_the_generated_client_guard() {
        // Isolate proxy environment variables in a child test process. Rust
        // 2024 deliberately makes mutation of the shared process environment
        // unsafe; concurrent tests must never observe temporary proxy settings.
        if std::env::var_os("IMPLUVIUM_DIRECT_TEST_CHILD").is_none() {
            let mut child = std::process::Command::new(std::env::current_exe().unwrap());
            child.arg("--exact")
                .arg("http::tests::reqwest_streams_chunked_bodies_through_the_generated_client_guard")
                .arg("--nocapture")
                .env("IMPLUVIUM_DIRECT_TEST_CHILD", "1");
            for name in [
                "HTTP_PROXY",
                "HTTPS_PROXY",
                "ALL_PROXY",
                "http_proxy",
                "https_proxy",
                "all_proxy",
            ] {
                child.env(name, "http://127.0.0.1:9");
            }
            child.env("NO_PROXY", "").env("no_proxy", "");
            let output = child.output().unwrap();
            assert!(
                output.status.success(),
                "{}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
            return;
        }
        let raw = b"HTTP/1.1 200 OK\r\nContent-Type: application/proto\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n1\r\n\0\r\n0\r\n\r\n".to_vec();
        let (result, request) = call(raw, Codec::Binary);
        assert_eq!(
            rpc::diagnostic(&result.unwrap_err()),
            Some(local(Reason::malformed_wire))
        );
        let request = String::from_utf8_lossy(&request).to_ascii_lowercase();
        assert!(
            request.starts_with("post /prefix/api/v2/impluvium.v2.authservice/revokeself http/1.1")
        );
        assert!(request.contains("accept-encoding: identity"));
        assert!(!request.contains("authorization:"));

        let (result, _) = call(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n1\r\n{\r\n1\r\n}\r\n0\r\n\r\n".to_vec(), Codec::Json);
        result.unwrap();
    }

    #[test]
    fn reqwest_does_not_follow_redirects_or_decompress() {
        for (raw, reason) in [
            (b"HTTP/1.1 302 Found\r\nLocation: /elsewhere\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec(), Reason::redirect_not_allowed),
            (b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Encoding: gzip\r\nContent-Length: 3\r\nConnection: close\r\n\r\nbad".to_vec(), Reason::unsupported_content_encoding),
        ] {
            let (result, _) = call(raw, Codec::Json);
            assert_eq!(rpc::diagnostic(&result.unwrap_err()), Some(local(reason)));
        }
    }

    #[test]
    fn reqwest_aborts_a_success_flood_at_the_route_byte_limit() {
        let mut raw = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n10001\r\n".to_vec();
        raw.resize(raw.len() + 65_537, b' ');
        raw.extend_from_slice(b"\r\n0\r\n\r\n");
        let (result, _) = call(raw, Codec::Json);
        assert_eq!(
            rpc::diagnostic(&result.unwrap_err()),
            Some(local(Reason::response_too_large))
        );
    }

    fn http() -> Http {
        let tls = Tls::new(Platform::Linux, vec![]).unwrap();
        Http::new(
            &tls,
            "impluvium-test/0.1.0 (linux; x86_64)",
            Duration::from_secs(5),
        )
        .unwrap()
    }

    fn run<T>(future: impl Future<Output = T>) -> T {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(future)
    }

    #[test]
    fn network_credentials_are_typed_and_rejected_before_connecting() {
        for codec in [Codec::Binary, Codec::Json] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            listener.set_nonblocking(true).unwrap();
            let base = BaseUrl::parse(&format!("http://{}/prefix", listener.local_addr().unwrap()))
                .unwrap();
            for authorization in ["Bearer must-not-leave-this-process", ""] {
                let transport = http().rpc(Settings::new(base.clone()), codec);
                let config = transport
                    .client_config()
                    .unwrap()
                    .with_default_header("Authorization", authorization);
                let client = AuthServiceClient::new(transport, config);
                let error = run(client.revoke_self(Default::default())).unwrap_err();
                assert_credentials_pending(&error);
                assert_eq!(
                    listener.accept().unwrap_err().kind(),
                    std::io::ErrorKind::WouldBlock
                );
            }
            for method in [Method::CompleteSession, Method::PollPairing] {
                let transport = http().rpc(Settings::new(base.clone()), codec);
                let config = transport.client_config().unwrap();
                let client = AuthServiceClient::new(transport, config);
                let error = match method {
                    Method::CompleteSession => run(client.complete_session(
                        impluvium_protocol::proto::impluvium::v2::CompleteSessionRequest {
                            proof: b"session-proof-must-not-leave".to_vec(),
                            ..Default::default()
                        },
                    ))
                    .unwrap_err(),
                    Method::PollPairing => run(client.poll_pairing(
                        impluvium_protocol::proto::impluvium::v2::PollPairingRequest {
                            proof: b"pairing-proof-must-not-leave".to_vec(),
                            ..Default::default()
                        },
                    ))
                    .unwrap_err(),
                    _ => unreachable!(),
                };
                assert_credentials_pending(&error);
                // No accepted socket means no request head or proof body could
                // reach this real loopback endpoint, including with Trusted state.
                assert_eq!(
                    listener.accept().unwrap_err().kind(),
                    std::io::ErrorKind::WouldBlock
                );
            }
        }
    }

    fn assert_credentials_pending(error: &(dyn StdError + 'static)) {
        let mut cause = Some(error);
        while let Some(error) = cause {
            if matches!(
                error.downcast_ref::<Error>(),
                Some(Error::CredentialsRequireVerifiedConnection)
            ) {
                return;
            }
            cause = error.source();
        }
        panic!("missing typed physical-connection prerequisite: {error}");
    }

    fn assert_no_credential_connection<T, E: StdError + 'static>(
        result: Result<T, E>,
        listener: &TcpListener,
    ) {
        match result {
            Err(error) => assert_credentials_pending(&error),
            Ok(_) => panic!("credential-bearing request was accepted"),
        }
        assert_eq!(
            listener.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
    }

    fn check_credential_header(names: &[&'static str], secret: &'static str) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let base =
            BaseUrl::parse(&format!("http://{}/prefix", listener.local_addr().unwrap())).unwrap();
        let http = http();
        for &name in names {
            for value in [secret, ""] {
                for count in [1, 2] {
                    for codec in [Codec::Binary, Codec::Json] {
                        let transport = http.rpc(Settings::new(base.clone()), codec);
                        let mut config = transport.client_config().unwrap();
                        for _ in 0..count {
                            config = config.with_default_header(name, value);
                        }
                        let client = AuthServiceClient::new(transport, config);
                        assert_no_credential_connection(
                            run(client.revoke_self(Default::default())),
                            &listener,
                        );
                    }
                    for method in [::http::Method::POST, ::http::Method::GET] {
                        let get = method == ::http::Method::GET;
                        let route = if get {
                            "objects/object"
                        } else {
                            Method::RevokeSelf.route()
                        };
                        let mut request = Request::builder()
                            .method(method)
                            .uri(format!("{}/api/v2/{route}", base.serialized()))
                            .header(header::CONTENT_TYPE, "application/json")
                            .body(full_body(if get {
                                Bytes::new()
                            } else {
                                Bytes::from_static(b"{}")
                            }))
                            .unwrap();
                        let name = ::http::header::HeaderName::from_bytes(name.as_bytes()).unwrap();
                        for _ in 0..count {
                            request
                                .headers_mut()
                                .append(name.clone(), ::http::HeaderValue::from_static(value));
                        }
                        assert_eq!(request.headers().get_all(&name).iter().count(), count);
                        if get {
                            // Http::get currently constructs its own headers.
                            // Inject at its shared network send boundary so the
                            // test also protects future GET header forwarding.
                            assert_no_credential_connection(run(http.send(request)), &listener);
                        } else {
                            let transport = http.rpc(Settings::new(base.clone()), Codec::Json);
                            assert_no_credential_connection(
                                run(transport.send(request)),
                                &listener,
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn authorization_header_never_connects_for_rpc_or_get() {
        check_credential_header(
            &[
                "authorization",
                "Authorization",
                "AUTHORIZATION",
                "aUtHoRiZaTiOn",
            ],
            "Bearer must-not-leave-this-process",
        );
    }

    #[test]
    fn proxy_authorization_header_never_connects_for_rpc_or_get() {
        check_credential_header(
            &[
                "proxy-authorization",
                "Proxy-Authorization",
                "PROXY-AUTHORIZATION",
                "pRoXy-AuThOrIzAtIoN",
            ],
            "Basic dGVzdDpzZWNyZXQ=",
        );
    }

    #[test]
    fn cookie_header_never_connects_for_rpc_or_get() {
        check_credential_header(
            &["cookie", "Cookie", "COOKIE", "cOoKiE"],
            "session=must-not-leave-this-process",
        );
    }

    fn sized_head(prefix: &[u8], suffix: &[u8], size: usize, padding: u8) -> Vec<u8> {
        let mut bytes = prefix.to_vec();
        bytes.resize(size - suffix.len(), padding);
        bytes.extend_from_slice(suffix);
        assert_eq!(bytes.len(), size);
        bytes
    }

    #[test]
    fn http1_raw_reason_and_whitespace_limits_include_the_status_line() {
        for size in [65_536, 65_537] {
            for (prefix, suffix, padding) in [
                (b"HTTP/1.1 200 ".as_slice(), b"\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n".as_slice(), b'A'),
                (b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nx:".as_slice(), b"value\r\nContent-Length: 2\r\nConnection: close\r\n\r\n".as_slice(), b' '),
                (b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nx:".as_slice(), b"value\r\nContent-Length: 2\r\nConnection: close\r\n\r\n".as_slice(), b'\t'),
            ] {
                let mut raw = sized_head(prefix, suffix, size, padding);
                raw.extend_from_slice(b"{}");
                let (result, _) = call(raw, Codec::Json);
                if size == 65_536 { result.unwrap(); } else {
                    assert_eq!(rpc::diagnostic(&result.unwrap_err()), Some(local(Reason::headers_too_large)));
                }
            }
        }
    }

    #[test]
    fn http1_informational_heads_have_their_own_raw_limit() {
        for size in [65_536, 65_537] {
            let mut raw = sized_head(
                b"HTTP/1.1 103 Early Hints\r\nx:",
                b"value\r\n\r\n",
                size,
                b' ',
            );
            // A second, separately valid 103 also rules out an aggregate budget.
            raw.extend_from_slice(b"HTTP/1.1 103 Early Hints\r\nx: small\r\n\r\n");
            raw.extend_from_slice(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}");
            let (result, _) = call(raw, Codec::Json);
            if size == 65_536 {
                result.unwrap();
            } else {
                assert_eq!(
                    rpc::diagnostic(&result.unwrap_err()),
                    Some(local(Reason::headers_too_large))
                );
            }
        }
    }

    #[test]
    fn get_ingress_uses_the_same_raw_http1_head_limit() {
        let mut raw = sized_head(
            b"HTTP/1.1 200 ",
            b"\r\nContent-Type: application/json\r\nContent-Length: 2\r\n\r\n",
            65_537,
            b'A',
        );
        raw.extend_from_slice(b"{}");
        let (base, server) = server(raw);
        let settings = Settings::new(base.clone());
        let result = run(http().get(
            &settings.policy(true),
            &format!("{}/api/v2/objects/object", base.serialized()),
            Binding::object(2, "application/json"),
        ));
        assert!(matches!(result, Err(Error::Rejected(d)) if d == local(Reason::headers_too_large)));
        server.join().unwrap();
    }

    #[test]
    fn generated_client_keeps_unknown_error_codes_independent_of_http_status() {
        for codec in [Codec::Binary, Codec::Json] {
            for code in ["future_code", "unknown", "unauthenticated"] {
                let body = format!(
                    r#"{{"code":"{code}","message":"retained","details":[{{"type":"impluvium.v2.ErrorInfo","value":"CAU="}}],"future":1e999}}"#
                );
                let raw = format!(
                    "HTTP/1.1 401 Unauthorized\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let (result, _) = call(raw.into_bytes(), codec);
                let error = result.unwrap_err();
                assert_eq!(
                    error.code,
                    if code == "unauthenticated" {
                        connectrpc::ErrorCode::Unauthenticated
                    } else {
                        connectrpc::ErrorCode::Unknown
                    }
                );
                assert_eq!(error.message.as_deref(), Some("retained"));
                assert_eq!(error.details.len(), 1);
                assert!(rpc::diagnostic(&error).is_none());
            }
        }
    }

    fn hpack_text(out: &mut Vec<u8>, bytes: &[u8]) {
        let mut length = bytes.len();
        if length < 127 {
            out.push(length as u8);
        } else {
            out.push(127);
            length -= 127;
            while length >= 128 {
                out.push((length & 127) as u8 | 128);
                length >>= 7;
            }
            out.push(length as u8);
        }
        out.extend_from_slice(bytes);
    }

    fn h2_frame(socket: &mut std::net::TcpStream, kind: u8, flags: u8, stream: u32, bytes: &[u8]) {
        let size = bytes.len() as u32;
        let mut head = [0u8; 9];
        head[..3].copy_from_slice(&size.to_be_bytes()[1..]);
        head[3] = kind;
        head[4] = flags;
        head[5..].copy_from_slice(&stream.to_be_bytes());
        socket.write_all(&head).unwrap();
        socket.write_all(bytes).unwrap();
    }

    fn h2_server(size: usize) -> (BaseUrl, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base =
            BaseUrl::parse(&format!("http://{}/prefix", listener.local_addr().unwrap())).unwrap();
        listener.set_nonblocking(true).unwrap();
        let server = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(5);
            let mut socket = loop {
                match listener.accept() {
                    Ok((socket, _)) => break socket,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(Instant::now() < deadline, "HTTP/2 client did not connect");
                        thread::sleep(Duration::from_millis(1));
                    }
                    Err(error) => panic!("accept failed: {error}"),
                }
            };
            socket
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut preface = [0; 24];
            socket.read_exact(&mut preface).unwrap();
            assert_eq!(&preface, b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n");
            h2_frame(&mut socket, 4, 0, 0, &[]);
            // Read the actual generated POST through END_STREAM before replying.
            let stream = loop {
                let mut head = [0; 9];
                socket.read_exact(&mut head).unwrap();
                let length = u32::from_be_bytes([0, head[0], head[1], head[2]]) as usize;
                let mut payload = vec![0; length];
                socket.read_exact(&mut payload).unwrap();
                let stream = u32::from_be_bytes(head[5..].try_into().unwrap()) & 0x7fff_ffff;
                if stream != 0 && matches!(head[3], 0 | 1) && head[4] & 1 != 0 {
                    break stream;
                }
            };
            let mut block = vec![0x88]; // HPACK static :status = 200
            block.push(0); // Literal without indexing, new name.
            hpack_text(&mut block, b"content-type");
            hpack_text(&mut block, b"application/json");
            block.push(0);
            hpack_text(&mut block, b"x");
            // :status = 42, content-type = 60, x overhead = 33.
            hpack_text(&mut block, &vec![b'a'; size - 135]);
            let chunks = block.chunks(16_384);
            let count = chunks.len();
            for (index, chunk) in chunks.enumerate() {
                h2_frame(
                    &mut socket,
                    if index == 0 { 1 } else { 9 },
                    if index + 1 == count { 4 } else { 0 },
                    stream,
                    chunk,
                );
            }
            // The over-limit peer may already have reset the stream.
            let mut data = vec![0, 0, 2, 0, 1];
            data.extend_from_slice(&stream.to_be_bytes());
            data.extend_from_slice(b"{}");
            let _ = socket.write_all(&data);
            let mut buffer = [0; 1024];
            while let Ok(n) = socket.read(&mut buffer) {
                if n == 0 {
                    break;
                }
            }
        });
        (base, server)
    }

    #[test]
    fn actual_http2_ingress_accepts_the_inclusive_header_list_boundary() {
        for size in [65_536, 65_537] {
            let (base, server) = h2_server(size);
            let transport = http()
                .http2_prior_knowledge()
                .rpc(Settings::new(base), Codec::Json);
            let config = transport.client_config().unwrap();
            let client = AuthServiceClient::new(transport, config);
            let result = run(client.revoke_self(Default::default()));
            if size == 65_536 {
                assert_eq!(
                    result.unwrap().headers().get("x").unwrap().as_bytes().len(),
                    65_401
                );
            } else {
                // h2 rejects >=65537 before the local inclusive check. Its
                // protocol failure must not turn into a successful RPC decode.
                assert!(result.is_err());
            }
            server.join().unwrap();
        }
    }
}
