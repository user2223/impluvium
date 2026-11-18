// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: 0BSD

//! Connect unary protection below the generated client's codec boundary.
//! No generated method can observe a body until this transport returns it.

use std::{error::Error as StdError, sync::Arc};

use ::http::{Request, Response, header};
use connectrpc::client::{BoxFuture, ClientBody, ClientConfig, ClientTransport, full_body};
use http_body_util::BodyExt;
use impluvium_protocol::{
    api::{self, Codec},
    decode::{self, InputLimit, Value},
    limits::MessageKind,
    proto::impluvium::v2::{GetGameResponse, GetVersionResponse, ListVersionsResponse},
};

use crate::{
    error::{Diagnostic, Reason, local},
    response::{self, Binding, HeadDecision, Header},
    transport::{self, BaseUrl, DepotTargets, Method, Policy, TrustState},
};

/// Public configuration only. Request destinations are rechecked on every send,
/// including calls made with a mutated generated-client configuration.
#[derive(Clone, Debug)]
pub struct Settings {
    pub base: BaseUrl,
    pub insecure_mode: bool,
    pub depots: DepotTargets,
    pub trust: TrustState,
}
impl Settings {
    pub fn new(base: BaseUrl) -> Self {
        Self {
            base,
            insecure_mode: false,
            depots: DepotTargets::default(),
            trust: TrustState::Trusted,
        }
    }
    pub fn policy(&self, platform_trust_available: bool) -> Policy<'_> {
        Policy {
            base: self.base_text(),
            platform_trust_available,
            insecure_mode: self.insecure_mode,
            depots: &self.depots,
            trust: self.trust,
        }
    }
    fn base_text(&self) -> &str {
        self.base.as_url().as_str()
    }
    pub fn client_config(&self, codec: Codec) -> Result<ClientConfig, Error> {
        let uri = format!("{}/api/v2", self.base.serialized())
            .parse()
            .map_err(|_| Error::Rejected(local(Reason::invalid_url)))?;
        let config = ClientConfig::new(uri)
            .with_default_max_message_size(4_194_304)
            .with_default_element_memory_limit(2_147_483_647);
        Ok(match codec {
            Codec::Binary => config.proto(),
            Codec::Json => config.json(),
        })
    }
}

#[derive(Debug)]
pub enum Error {
    Rejected(Diagnostic),
    Transport(Arc<dyn StdError + Send + Sync>),
    UnsupportedRequest,
    /// The network adapter cannot yet authorize credentials on a physical
    /// connection. This local prerequisite is never a server wire reason.
    CredentialsRequireVerifiedConnection,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Rejected(d) => write!(f, "response rejected: {:?} ({})", d.reason, d.field),
            Self::Transport(_) => f.write_str("HTTP transport failed"),
            Self::UnsupportedRequest => f.write_str("expected an uncompressed Connect unary POST"),
            Self::CredentialsRequireVerifiedConnection => {
                f.write_str("credentials require authorization on the physical connection")
            }
        }
    }
}
impl StdError for Error {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::Transport(error) => Some(error.as_ref()),
            _ => None,
        }
    }
}
impl From<Diagnostic> for Error {
    fn from(value: Diagnostic) -> Self {
        Self::Rejected(value)
    }
}
impl Error {
    pub(crate) fn transport(error: impl StdError + Send + Sync + 'static) -> Self {
        Self::Transport(Arc::new(error))
    }
}

/// Carries the local reason through Connect's transport error source chain.
/// Local diagnostics never become a server ErrorInfo or wire error reason.
pub fn diagnostic(error: &(dyn StdError + 'static)) -> Option<Diagnostic> {
    let mut current = Some(error);
    while let Some(error) = current {
        if let Some(Error::Rejected(d)) = error.downcast_ref::<Error>() {
            return Some(*d);
        }
        current = error.source();
    }
    None
}

/// A protected transport for the actual generated service clients. `inner` is
/// an HTTP transport, not a decoded RPC service. The response profile is checked
/// before returning from `send`, for successes and for errors in either codec.
/// The inner adapter must bound raw HTTP/1 heads, including informational heads,
/// before parsing them; normalized fields cannot establish that raw boundary.
#[derive(Clone)]
pub struct ProtectedTransport<T> {
    inner: T,
    settings: Settings,
    codec: Codec,
    platform_trust_available: bool,
}
impl<T> ProtectedTransport<T> {
    /// In-process adapters may supply their platform availability explicitly.
    /// Network callers use `Http::rpc`, which derives it from initialized TLS.
    pub fn new(inner: T, settings: Settings, codec: Codec, platform_trust_available: bool) -> Self {
        Self {
            inner,
            settings,
            codec,
            platform_trust_available,
        }
    }
    pub fn client_config(&self) -> Result<ClientConfig, Error> {
        self.settings.client_config(self.codec)
    }
}

impl<T> ClientTransport for ProtectedTransport<T>
where
    T: ClientTransport,
    <T::ResponseBody as http_body::Body>::Error: StdError + Send + Sync + 'static,
{
    type ResponseBody = ClientBody;
    type Error = Error;

    fn send(
        &self,
        mut request: Request<ClientBody>,
    ) -> BoxFuture<'static, Result<Response<ClientBody>, Error>> {
        let this = self.clone();
        Box::pin(async move {
            let url = transport::request_url(&request.uri().to_string())?;
            if !this.platform_trust_available {
                return Err(local(Reason::platform_trust_unavailable).into());
            }
            if url.scheme() == "http"
                && !this.settings.insecure_mode
                && !transport::is_loopback(&url)
            {
                return Err(local(Reason::insecure_scheme_not_allowed).into());
            }
            let route = request
                .uri()
                .path()
                .strip_prefix(&format!("{}/api/v2/", this.settings.base.prefix()))
                .and_then(Method::from_route)
                .ok_or(local(Reason::origin_not_allowed))?;
            let binding = Binding::rpc(route, this.codec);
            transport::preflight(
                &this.settings.policy(this.platform_trust_available),
                &request.uri().to_string(),
                binding.clone(),
            )?;
            crate::http::credential_free(&request)?;
            let expected_type = match this.codec {
                Codec::Binary => "application/proto",
                Codec::Json => "application/json",
            };
            if request.method() != ::http::Method::POST
                || request
                    .headers()
                    .get(header::CONTENT_TYPE)
                    .and_then(|v| v.to_str().ok())
                    != Some(expected_type)
                || request.headers().contains_key(header::CONTENT_ENCODING)
            {
                return Err(Error::UnsupportedRequest);
            }
            request.headers_mut().insert(
                header::ACCEPT_ENCODING,
                ::http::HeaderValue::from_static("identity"),
            );
            let (request_parts, request_body) = request.into_parts();
            let request_bytes = http_body_util::Limited::new(request_body, 65_536)
                .collect()
                .await
                .map_err(|_| Error::Rejected(local(Reason::body_too_large)))?
                .to_bytes();
            let identity = request_identity(route, this.codec, &request_bytes)?;
            let request = Request::from_parts(request_parts, full_body(request_bytes));
            let response = this.inner.send(request).await.map_err(Error::transport)?;
            let (mut parts, body) = response.into_parts();
            let headers = headers(&parts.headers)?;
            // HTTP/1 size enforcement belongs to the network parser, including
            // each informational head. Never reconstruct its raw octet count.
            let measured = parts.extensions.get::<MeasuredHead>().map_or_else(
                || parsed_head_size(parts.version, &parts.headers),
                |size| size.0,
            );
            let HeadDecision::Body(mut reader) =
                response::head(&binding, measured, parts.status.as_u16(), &headers)?
            else {
                return Err(local(Reason::redirect_not_allowed).into());
            };
            let mut body = std::pin::pin!(body);
            while let Some(frame) = body.frame().await {
                let frame = frame.map_err(Error::transport)?;
                if let Ok(data) = frame.into_data() {
                    reader.push(&data)?;
                }
            }
            let bytes = reader.finish_for_decoder()?;
            if parts.status.as_u16() == 200
                && let Some(identity) = identity
            {
                // These identities come from the request, never from response
                // headers. Each chapter decoder scans before materialization.
                match route {
                    Method::GetGame => {
                        api::decode_generic::<GetGameResponse>(this.codec, &bytes, &identity)?;
                    }
                    Method::GetVersion => {
                        api::decode_generic::<GetVersionResponse>(this.codec, &bytes, &identity)?;
                    }
                    Method::ListVersions => {
                        api::decode_generic::<ListVersionsResponse>(this.codec, &bytes, &identity)?;
                    }
                    _ => unreachable!("only identity-bound requests carry an identity"),
                }
            }
            // Identity coding is already enforced; removing it avoids asking
            // the Connect compression registry to interpret an HTTP coding.
            parts.headers.remove(header::CONTENT_ENCODING);
            parts.headers.insert(
                header::CONTENT_TYPE,
                expected_type.parse().expect("static type"),
            );
            if parts.status.as_u16() != 200 {
                parts.headers.insert(
                    header::CONTENT_TYPE,
                    ::http::HeaderValue::from_static("application/json"),
                );
                // Connect treats every 2xx as success. The protocol's sole RPC
                // success status is 200; a checked wrapper on another 2xx must
                // follow Connect's error path as well.
                if parts.status.is_success() {
                    parts.status = ::http::StatusCode::BAD_REQUEST;
                }
            }
            Ok(Response::from_parts(parts, full_body(bytes.into())))
        })
    }
}

fn request_identity(
    method: Method,
    codec: Codec,
    bytes: &[u8],
) -> Result<Option<String>, Diagnostic> {
    let kind = match method {
        Method::GetGame => MessageKind::GetGameRequest,
        Method::GetVersion => MessageKind::GetVersionRequest,
        Method::ListVersions => MessageKind::ListVersionsRequest,
        _ => return Ok(None),
    };
    let projection = match codec {
        Codec::Binary => decode::project_binary(kind, bytes, InputLimit::body(65_536)),
        Codec::Json => decode::project_json(kind, bytes, InputLimit::body(65_536)),
    }?;
    Ok(Some(
        projection
            .fields
            .into_iter()
            .find_map(|(number, value)| match (number, value) {
                (1, Value::String(value)) => Some(value),
                _ => None,
            })
            .unwrap_or_default(),
    ))
}

pub(crate) fn headers(map: &::http::HeaderMap) -> Result<Vec<Header<'_>>, Error> {
    map.iter()
        .map(|(name, value)| {
            Ok(Header {
                name: name.as_str(),
                value: value.to_str().map_err(Error::transport)?,
            })
        })
        .collect()
}

pub(crate) fn parsed_head_size(version: ::http::Version, headers: &::http::HeaderMap) -> u64 {
    if version == ::http::Version::HTTP_2 {
        response::h2_header_list_size(
            headers
                .iter()
                .map(|(name, value)| (name.as_str().as_bytes(), value.as_bytes())),
        ) + 42 // :status (7 + 3 + 32)
    } else {
        // HTTP/1 adapters must enforce the raw limit before returning a head.
        // There is no meaningful reconstruction from normalized header fields.
        0
    }
}

/// Exact adapter measurement when raw header bytes are available. Never derive
/// this value from Content-Length or from a normalized HTTP/1 header map.
#[derive(Clone, Copy, Debug)]
pub struct MeasuredHead(pub u64);

#[cfg(test)]
mod tests;
