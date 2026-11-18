// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: 0BSD

//! Credential release on one owned TCP/TLS connection. Verification always
//! consumes discovery received by this handle, never caller-supplied evidence.
//! Dropping or losing the sender requires a new handle and fresh discovery.

use std::{error::Error as StdError, sync::Arc};

use ::http::{Request, header};
use bytes::Bytes;
use connectrpc::client::{ClientBody, full_body};
use http_body_util::BodyExt;
use impluvium_protocol::{
    api::{
        self, Codec,
        trust::{self, ClientVersion},
    },
    proto::impluvium::v2::{Discovery, RefreshTicketResponse},
};
use tokio::time::{Instant, timeout_at};

use crate::{
    credentials::{
        Authorization, Context, CredentialPurpose, CredentialSource, PairingContext, Ticket,
    },
    error::{Reason, local},
    http::{Http, Physical},
    response::{self, Binding, HeadDecision, Outcome},
    rpc::{Error, Settings},
    transport::{self, DepotTargets, Method, PreparedRequest, Purpose},
};

// Each handle exclusively borrows its context, clears this slot on entry and
// exit, and never replaces its physical sender. The name cannot carry evidence
// across handles; response provenance additionally uses a private Arc identity.
const CONNECTION: &str = "owned-credential-connection";

/// The adapter attaches only the credential kind prescribed by the method.
/// A proof payload is the complete encoded request, produced after the source
/// read (which may return a signing handle rather than private key bytes).
pub enum Payload {
    Public(Bytes),
    Proof(Bytes),
    AccessToken { token: String, body: Bytes },
}
impl Payload {
    fn parts(self, purpose: CredentialPurpose) -> Result<(Bytes, Option<String>), Error> {
        match (purpose, self) {
            (CredentialPurpose::None, Self::Public(body))
            | (CredentialPurpose::Proof, Self::Proof(body)) => Ok((body, None)),
            (CredentialPurpose::AccessToken, Self::AccessToken { token, body }) => {
                if token.len() != 43
                    || !token
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
                {
                    return Err(Error::CredentialPurposeMismatch);
                }
                Ok((body, Some(format!("Bearer {token}"))))
            }
            _ => Err(Error::CredentialPurposeMismatch),
        }
    }
}

enum Owner<'a> {
    Pinned(&'a mut Context),
    Pairing(&'a mut PairingContext),
}
impl Owner<'_> {
    fn reset(&mut self) {
        match self {
            Self::Pinned(context) => context.open(CONNECTION),
            Self::Pairing(context) => context.open(),
        }
    }
    fn authorize<'a>(&'a self, request: &'a PreparedRequest) -> Result<Authorization<'a>, Error> {
        match self {
            Self::Pinned(context) => context
                .authorize(request, CONNECTION, None)
                .map_err(Into::into),
            Self::Pairing(context) => context.authorize(request, CONNECTION).map_err(Into::into),
        }
    }
}

/// An exclusive, non-cloneable API connection. Construction does no I/O.
/// Call `verify` before `rpc`; a failed or canceled exchange permanently loses
/// this sender. Neither reconnection nor credential replay is implicit.
pub struct CredentialConnection<'a> {
    http: Http,
    settings: Settings,
    owner: Owner<'a>,
    physical: Option<Physical>,
    opened: bool,
    discovery: Option<Discovery>,
    identity: Arc<()>,
}

impl Http {
    pub fn credential_connection<'a>(
        &self,
        settings: Settings,
        context: &'a mut Context,
    ) -> Result<CredentialConnection<'a>, Error> {
        if settings.base.serialized() != context.base().serialized() {
            return Err(local(Reason::origin_not_allowed).into());
        }
        Ok(CredentialConnection::new(
            self.clone(),
            settings,
            Owner::Pinned(context),
        ))
    }

    pub fn pairing_connection<'a>(
        &self,
        settings: Settings,
        context: &'a mut PairingContext,
    ) -> Result<CredentialConnection<'a>, Error> {
        if settings.base.serialized() != context.base().serialized() {
            return Err(local(Reason::origin_not_allowed).into());
        }
        Ok(CredentialConnection::new(
            self.clone(),
            settings,
            Owner::Pairing(context),
        ))
    }
}

impl<'a> CredentialConnection<'a> {
    fn new(http: Http, settings: Settings, mut owner: Owner<'a>) -> Self {
        owner.reset();
        Self {
            http,
            settings,
            owner,
            physical: None,
            opened: false,
            discovery: None,
            identity: Arc::new(()),
        }
    }

    fn prepare(&self, url: &str, binding: Binding) -> Result<PreparedRequest, Error> {
        let mut policy = self.settings.policy(true);
        if let Owner::Pinned(context) = &self.owner {
            policy.trust = context.trust_state();
        }
        transport::preflight(&policy, url, binding).map_err(Into::into)
    }

    /// Credential-free discovery on this sender, including profile, immutable
    /// pin, fingerprint, version and required-capability checks in spec order.
    /// This API implements major 2; other client majors are unsupported input.
    pub async fn verify(
        &mut self,
        client: ClientVersion,
        implemented: &[String],
    ) -> Result<&Discovery, Error> {
        if client.major != 2 {
            return Err(Error::UnsupportedRequest);
        }
        self.discovery = None;
        self.owner.reset();
        let prepared = self.prepare(
            &format!("{}/api/info", self.settings.base.serialized()),
            Binding::discovery(),
        )?;
        let expires = Instant::now() + self.http.deadline;
        if !self.opened {
            self.opened = true;
            let uri = prepared.url().as_str().parse().map_err(Error::transport)?;
            self.physical = Some(
                timeout_at(expires, self.http.open_physical(uri))
                    .await
                    .map_err(Error::transport)??,
            );
        }
        let result = self.exchange(&prepared, Bytes::new(), None, expires).await;
        let checked: Result<Discovery, Error> = (|| {
            let Outcome::Success(body) = result? else {
                return Err(local(Reason::not_an_impluvium_server).into());
            };
            let discovery = body.discovery()?;
            let pin = match &self.owner {
                Owner::Pinned(context) => Some(context.pin()),
                Owner::Pairing(_) => None,
            };
            let evidence =
                trust::check_discovery(CONNECTION, &discovery, pin, client, implemented)?;
            match &mut self.owner {
                Owner::Pinned(context) => context.checked(CONNECTION, evidence),
                Owner::Pairing(context) => context.checked(CONNECTION, evidence),
            }
            .map_err(Error::Verification)?;
            Ok(discovery)
        })();
        match checked {
            Ok(discovery) => {
                self.discovery = Some(discovery);
                Ok(self.discovery.as_ref().expect("discovery just stored"))
            }
            Err(error) => {
                self.physical = None;
                if let (Owner::Pinned(context), Error::Rejected(diagnostic)) =
                    (&mut self.owner, &error)
                {
                    let _ = context.rejected(CONNECTION, *diagnostic);
                }
                Err(error)
            }
        }
    }

    /// Preflight and connection authorization precede both the credential read
    /// and `build`. The callback receives no credential for public methods.
    /// Returned bodies retain the shared protected chapter decoders.
    pub async fn rpc<S, F>(
        &mut self,
        method: Method,
        codec: Codec,
        source: &mut S,
        build: F,
    ) -> Result<Reply, Error>
    where
        S: CredentialSource,
        S::Error: StdError + Send + Sync + 'static,
        F: FnOnce(Option<S::Credential>) -> Result<Payload, Error>,
    {
        let prepared = self.prepare(
            &format!(
                "{}/api/v2/{}",
                self.settings.base.serialized(),
                method.route(),
            ),
            Binding::rpc(method, codec),
        )?;
        self.require_verified()?;
        let credential = match self.owner.authorize(&prepared)? {
            Authorization::None => None,
            Authorization::Read(permit) => Some(
                permit
                    .read(source)
                    .map_err(|error| Error::CredentialSource(Arc::new(error)))?,
            ),
            _ => return Err(Error::CredentialsRequireVerifiedConnection),
        };
        let (body, token) = build(credential)?.parts(method.credential())?;
        if body.len() > 65_536 {
            return Err(local(Reason::body_too_large).into());
        }
        let expires = Instant::now() + self.http.deadline;
        let outcome = self.exchange(&prepared, body, token, expires).await?;
        if let Outcome::Success(body) = &outcome
            && let Err(error) = body.precheck_rpc()
        {
            self.physical = None;
            self.discovery = None;
            self.owner.reset();
            return Err(error.into());
        }
        Ok(Reply {
            request: prepared,
            outcome,
            identity: self.identity.clone(),
        })
    }

    /// Protected finite API documents use the same verified sender and source.
    /// The caller still performs the chapter's signature/identity checks through
    /// CheckedBody; SSE requires a separate streaming receiver.
    pub async fn get<S>(
        &mut self,
        url: &str,
        binding: Binding,
        source: &mut S,
    ) -> Result<Reply, Error>
    where
        S: CredentialSource<Credential = String>,
        S::Error: StdError + Send + Sync + 'static,
    {
        let prepared = self.prepare(url, binding)?;
        if !matches!(
            prepared.purpose(),
            Purpose::Manifest | Purpose::Descriptor | Purpose::Object,
        ) {
            return Err(Error::CredentialPurposeMismatch);
        }
        self.require_verified()?;
        let Authorization::Read(permit) = self.owner.authorize(&prepared)? else {
            return Err(Error::CredentialsRequireVerifiedConnection);
        };
        let token = permit
            .read(source)
            .map_err(|error| Error::CredentialSource(Arc::new(error)))?;
        let (body, token) = Payload::AccessToken {
            token,
            body: Bytes::new(),
        }
        .parts(CredentialPurpose::AccessToken)?;
        let expires = Instant::now() + self.http.deadline;
        let outcome = self.exchange(&prepared, body, token, expires).await?;
        Ok(Reply {
            request: prepared,
            outcome,
            identity: self.identity.clone(),
        })
    }

    fn require_verified(&self) -> Result<(), Error> {
        let Some(physical) = &self.physical else {
            return Err(Error::CredentialsRequireVerifiedConnection);
        };
        if self.discovery.is_none() || physical.is_closed() {
            return Err(Error::CredentialsRequireVerifiedConnection);
        }
        Ok(())
    }

    async fn exchange(
        &mut self,
        prepared: &PreparedRequest,
        body: Bytes,
        token: Option<String>,
        expires: Instant,
    ) -> Result<Outcome, Error> {
        // Taking the driver out also makes cancellation fail closed: a dropped
        // future drops the connection, and no later call can reconnect it.
        let mut physical = self
            .physical
            .take()
            .ok_or(Error::CredentialsRequireVerifiedConnection)?;
        let result = match timeout_at(
            expires,
            roundtrip(&self.http, &mut physical, prepared, body, token),
        )
        .await
        {
            Ok(result) => result,
            Err(error) => Err(Error::transport(error)),
        };
        if result.is_ok() {
            self.physical = Some(physical);
        } else {
            self.discovery = None;
            self.owner.reset();
        }
        result
    }

    fn pinned_reply<'b>(&'b self, reply: &Reply) -> Result<&'b Context, Error> {
        // A complete response keeps its provenance even when its sender has
        // since received Connection: close or GOAWAY. It cannot authorize a send.
        if !Arc::ptr_eq(&self.identity, &reply.identity) {
            return Err(Error::CredentialsRequireVerifiedConnection);
        }
        match &self.owner {
            Owner::Pinned(context) => {
                if let Some(diagnostic) = context.trust_state().diagnostic() {
                    return Err(diagnostic.into());
                }
                Ok(context)
            }
            Owner::Pairing(_) => Err(Error::CredentialPurposeMismatch),
        }
    }

    /// Only an actual protected response of this handle can mint provenance.
    pub fn download_response(
        &self,
        reply: &Reply,
        version: &str,
        game: Option<&str>,
    ) -> Result<(api::Download, Option<Ticket>), Error> {
        let context = self.pinned_reply(reply)?;
        let Outcome::Success(body) = &reply.outcome else {
            return Err(Error::UnsupportedRequest);
        };
        context
            .download_response(&reply.request, CONNECTION, body, version, game)
            .map_err(Into::into)
    }

    pub fn refresh_response(
        &self,
        reply: &Reply,
    ) -> Result<(RefreshTicketResponse, Option<Ticket>), Error> {
        let context = self.pinned_reply(reply)?;
        let Outcome::Success(body) = &reply.outcome else {
            return Err(Error::UnsupportedRequest);
        };
        context
            .refresh_response(&reply.request, CONNECTION, body)
            .map_err(Into::into)
    }

    /// Explicit cleanup uses errors.md §7 only; a failed verification never
    /// reads, deletes, replaces or rewrites stored credentials or the pin.
    pub fn cleanup<S: CredentialSource>(&self, reply: &Reply, source: &mut S) -> Result<(), Error>
    where
        S::Error: StdError + Send + Sync + 'static,
    {
        let context = self.pinned_reply(reply)?;
        if let Outcome::Error(body) = &reply.outcome
            && let Some(cleanup) = context.cleanup(&reply.request, CONNECTION, body)
        {
            cleanup
                .apply(source)
                .map_err(|error| Error::CredentialSource(Arc::new(error)))?;
        }
        Ok(())
    }

    /// Depot connections deliberately have no API discovery. Only a proven
    /// ticket can be attached, after exact endpoint and current-origin preflight.
    /// Every redirect repeats endpoint preflight, remains on the original
    /// origin and preserves both resource identifiers, with at most five hops.
    pub async fn depot_get(
        &self,
        depots: &DepotTargets,
        url: &str,
        binding: Binding,
        ticket: &Ticket,
    ) -> Result<Outcome, Error> {
        let Owner::Pinned(context) = &self.owner else {
            return Err(Error::CredentialPurposeMismatch);
        };
        let mut policy = self.settings.policy(true);
        policy.depots = depots;
        policy.trust = context.trust_state();
        let original = transport::preflight(&policy, url, binding)?;
        if original.purpose() != Purpose::Chunk {
            return Err(Error::CredentialPurposeMismatch);
        }
        let mut prepared = original.clone();
        let mut followed = 0;
        loop {
            let Authorization::Ticket(bytes) =
                context.authorize(&prepared, CONNECTION, Some(ticket))?
            else {
                return Err(Error::CredentialsRequireVerifiedConnection);
            };
            let token =
                crate::credentials::ticket_header(bytes).ok_or(Error::CredentialPurposeMismatch)?;
            let expires = Instant::now() + self.http.deadline;
            let outcome = timeout_at(expires, async {
                let uri = prepared.url().as_str().parse().map_err(Error::transport)?;
                let mut physical = self.http.open_physical(uri).await?;
                roundtrip(
                    &self.http,
                    &mut physical,
                    &prepared,
                    Bytes::new(),
                    Some(token),
                )
                .await
            })
            .await
            .map_err(Error::transport)??;
            match outcome {
                Outcome::Redirect(target) => {
                    prepared =
                        transport::redirect(&policy, &original, prepared.url(), followed, &target)?;
                    followed += 1;
                }
                outcome => return Ok(outcome),
            }
        }
    }
}

impl Drop for CredentialConnection<'_> {
    fn drop(&mut self) {
        self.owner.reset();
    }
}

/// The request and private connection identity cannot be substituted by a
/// caller when requesting ticket provenance or credential cleanup.
pub struct Reply {
    request: PreparedRequest,
    outcome: Outcome,
    identity: Arc<()>,
}
impl Reply {
    pub fn outcome(&self) -> &Outcome {
        &self.outcome
    }
    pub fn into_outcome(self) -> Outcome {
        self.outcome
    }
}

async fn roundtrip(
    http: &Http,
    physical: &mut Physical,
    prepared: &PreparedRequest,
    body: Bytes,
    token: Option<String>,
) -> Result<Outcome, Error> {
    let method = if matches!(prepared.purpose(), Purpose::Rpc(_)) {
        ::http::Method::POST
    } else {
        ::http::Method::GET
    };
    let mut request = Request::builder()
        .method(method)
        .uri(prepared.url().as_str());
    if let Purpose::Rpc(_) = prepared.purpose() {
        request = request.header("connect-protocol-version", "1");
        // The response binding fixes the request codec before credentials exist.
        let content_type = prepared
            .binding()
            .rpc_content_type()
            .ok_or(Error::UnsupportedRequest)?;
        request = request.header(header::CONTENT_TYPE, content_type);
    }
    if let Some(token) = token {
        let mut value = token
            .parse::<::http::HeaderValue>()
            .map_err(Error::transport)?;
        value.set_sensitive(true);
        request = request.header(header::AUTHORIZATION, value);
    }
    if let Some(range) = prepared.binding().range_header() {
        request = request.header(header::RANGE, range);
    }
    let request: Request<ClientBody> = request.body(full_body(body)).map_err(Error::transport)?;
    let mut incoming = physical.send(http, request).await?;
    let fields = crate::rpc::headers(incoming.headers())?;
    let measured = crate::rpc::parsed_head_size(incoming.version(), incoming.headers());
    match response::head(
        prepared.binding(),
        measured,
        incoming.status().as_u16(),
        &fields,
    )? {
        HeadDecision::Body(mut reader) => {
            while let Some(frame) = incoming.body_mut().frame().await {
                if let Ok(chunk) = frame.map_err(Error::transport)?.into_data() {
                    reader.push(&chunk)?;
                }
            }
            reader.finish().map_err(Into::into)
        }
        HeadDecision::Redirect(target) => Ok(Outcome::Redirect(target)),
        _ => Err(Error::UnsupportedRequest),
    }
}
