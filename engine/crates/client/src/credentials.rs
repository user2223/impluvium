// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: 0BSD

//! A shared credential source for every HTTP stack. Authorization is separate
//! from reading: pending discovery and ticket provenance are not rejections.

use crate::{
    error::{Diagnostic, local},
    response::CheckedBody,
    transport::{BaseUrl, Method, Origin, PreparedRequest, Purpose, TrustState},
};
use impluvium_protocol::{
    api::{
        self,
        errors::{self, ErrorBody},
        trust::{CheckedConnection, Connections, Pin, VerificationError},
    },
    decode::Reason,
    keys::{Content, KeyInput, TrustedKeySet},
    proto::impluvium::{signed::v1::KeyRole, v2::RefreshTicketResponse},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CredentialPurpose {
    None,
    Proof,
    AccessToken,
    Ticket,
}

/// The platform store decides how device secrets and access tokens are kept.
/// Proof reads provide signing material or a handle. Adapters generate proofs
/// only after this authorized read.
/// No adapter may read a second source or consult a proxy credential provider.
pub trait CredentialSource {
    type Error;
    type Credential;
    fn read(
        &mut self,
        identity: Identity<'_>,
        purpose: CredentialPurpose,
    ) -> Result<Self::Credential, Self::Error>;
    fn delete(&mut self, pin: &Pin) -> Result<(), Self::Error>;
}

pub struct Context {
    base: BaseUrl,
    pin: Pin,
    connections: Connections,
    generation: std::sync::Arc<()>,
}
impl Context {
    pub fn new(base: BaseUrl, pin: Pin) -> Self {
        let connections = Connections::new(&pin);
        Self {
            base,
            pin,
            connections,
            generation: std::sync::Arc::new(()),
        }
    }
    pub fn pin(&self) -> &Pin {
        &self.pin
    }
    pub fn base(&self) -> &BaseUrl {
        &self.base
    }
    pub fn connections(&self) -> &Connections {
        &self.connections
    }
    pub fn open(&mut self, connection: &str) {
        self.connections = self.connections.open(connection);
    }
    pub fn checked(
        &mut self,
        connection: &str,
        evidence: CheckedConnection,
    ) -> Result<(), VerificationError> {
        let (next, result) = self.connections.checked(connection, evidence);
        self.connections = next;
        result
    }
    pub fn rejected(&mut self, connection: &str, diagnostic: Diagnostic) -> Result<(), Diagnostic> {
        let (next, result) = self.connections.rejected(connection, diagnostic);
        self.connections = next;
        result
    }
    pub fn trust_state(&self) -> TrustState {
        match self.connections.stopped().map(|d| d.reason) {
            Some(Reason::instance_mismatch) => TrustState::InstanceMismatch,
            Some(Reason::content_key_changed) => TrustState::ContentKeyChanged,
            _ => TrustState::Trusted,
        }
    }
    /// Binding the stored base and exact pin prevents verification evidence for
    /// one trust context from authorizing another origin or credential store key.
    pub fn authorize<'a>(
        &'a self,
        request: &'a PreparedRequest,
        connection: &str,
        ticket: Option<&'a Ticket>,
    ) -> Result<Authorization<'a>, Diagnostic> {
        if self.base.serialized() != request.base().serialized() {
            return Err(local(Reason::origin_not_allowed));
        }
        let purpose = request.purpose().credential();
        if purpose != CredentialPurpose::None
            && let Some(d) = self.connections.stopped()
        {
            return Err(local(d.reason));
        }
        if matches!(
            request.purpose(),
            Purpose::Rpc(Method::StartPairing | Method::PollPairing)
        ) {
            return Ok(Authorization::Pending(Prerequisite::Discovery));
        }
        if purpose == CredentialPurpose::None {
            return Ok(Authorization::None);
        }
        if purpose == CredentialPurpose::Ticket {
            return Ok(
                match ticket.filter(|t| {
                    t.pin == self.pin
                        && t.base == self.base.serialized()
                        && std::sync::Arc::ptr_eq(&t.generation, &self.generation)
                }) {
                    Some(ticket) => Authorization::Ticket(&ticket.bytes),
                    None => Authorization::Pending(Prerequisite::AuthenticatedDownload),
                },
            );
        }
        if !self.connections.credential_allowed(connection) {
            return Ok(Authorization::Pending(Prerequisite::Discovery));
        }
        Ok(Authorization::Read(ReadPermit {
            identity: Identity::Pinned(&self.pin),
            purpose,
        }))
    }
    /// Ticket provenance follows successful protected download/refresh decoding
    /// on this context's verified physical connection. Depot connections never
    /// create provenance. Call while processing that response, before reusing IDs.
    pub fn download_response(
        &self,
        request: &PreparedRequest,
        connection: &str,
        body: &CheckedBody,
        version: &str,
        game: Option<&str>,
    ) -> Result<(api::Download, Option<Ticket>), Diagnostic> {
        if request.purpose() != Purpose::Rpc(Method::OpenDownload)
            || body.binding().purpose() != request.purpose()
        {
            return Err(local(Reason::broken_error_body));
        }
        let input = KeyInput {
            public_key: self.pin.content_public_key(),
            role: KeyRole::Content,
            ticket_status: None,
        };
        let trusted = TrustedKeySet::<Content>::new(&[input])
            .expect("a validated pin is a valid singleton content key");
        let download = body.download(&trusted, version, game)?;
        let ticket = self.ticket(request, connection, &download.response().ticket);
        Ok((download, ticket))
    }
    pub fn refresh_response(
        &self,
        request: &PreparedRequest,
        connection: &str,
        body: &CheckedBody,
    ) -> Result<(RefreshTicketResponse, Option<Ticket>), Diagnostic> {
        if request.purpose() != Purpose::Rpc(Method::RefreshTicket)
            || body.binding().purpose() != request.purpose()
        {
            return Err(local(Reason::broken_error_body));
        }
        let response = body.generic::<RefreshTicketResponse>(())?;
        let ticket = self.ticket(request, connection, &response.ticket);
        Ok((response, ticket))
    }
    fn ticket(&self, request: &PreparedRequest, connection: &str, bytes: &[u8]) -> Option<Ticket> {
        if bytes.is_empty()
            || bytes.len() > 4096
            || request.base().serialized() != self.base.serialized()
            || !self.connections.credential_allowed(connection)
        {
            return None;
        }
        Some(Ticket {
            pin: self.pin.clone(),
            base: self.base.serialized(),
            bytes: bytes.into(),
            generation: self.generation.clone(),
        })
    }
    /// A validated wrapper alone cannot cause deletion. Server origin, API
    /// surface and exact pin-bound connection evidence are mandatory too.
    pub fn cleanup(
        &self,
        request: &PreparedRequest,
        connection: &str,
        body: &ErrorBody,
    ) -> Option<Cleanup<'_>> {
        let source = if request.purpose() != Purpose::Chunk
            && request.base().serialized() == self.base.serialized()
            && Origin::of(request.url()) == self.base.origin()
        {
            errors::Source::Server {
                response_url: request.url().as_str(),
                transport_verified: self.connections.credential_allowed(connection),
            }
        } else {
            errors::Source::Other
        };
        let reason = body.info().map_or(0, |info| info.reason.to_i32());
        errors::delete_credentials(&self.base.serialized(), source, true, body.code(), reason)
            .then_some(Cleanup { pin: &self.pin })
    }
}

pub enum Authorization<'a> {
    None,
    Pending(Prerequisite),
    Read(ReadPermit<'a>),
    Ticket(&'a [u8]),
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Prerequisite {
    Discovery,
    AuthenticatedDownload,
}
pub struct ReadPermit<'a> {
    identity: Identity<'a>,
    purpose: CredentialPurpose,
}
impl ReadPermit<'_> {
    pub fn read<S: CredentialSource>(self, source: &mut S) -> Result<S::Credential, S::Error> {
        source.read(self.identity, self.purpose)
    }
}
pub struct Cleanup<'a> {
    pin: &'a Pin,
}
impl Cleanup<'_> {
    pub fn apply<S: CredentialSource>(self, source: &mut S) -> Result<(), S::Error> {
        source.delete(self.pin)
    }
}
pub struct Ticket {
    pin: Pin,
    base: String,
    bytes: Vec<u8>,
    generation: std::sync::Arc<()>,
}

/// Tickets are opaque binary envelopes, never parsed or re-encoded by clients.
/// The resulting header is at most 5469 bytes, including `Bearer `.
pub fn ticket_header(bytes: &[u8]) -> Option<String> {
    if bytes.is_empty() || bytes.len() > 4096 {
        return None;
    }
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::from("Bearer ");
    for group in bytes.chunks(3) {
        let n = (u32::from(group[0]) << 16)
            | (u32::from(*group.get(1).unwrap_or(&0)) << 8)
            | u32::from(*group.get(2).unwrap_or(&0));
        for shift in [18, 12, 6, 0].into_iter().take(group.len() + 1) {
            out.push(char::from(ALPHABET[((n >> shift) & 63) as usize]));
        }
    }
    Some(out)
}

/// A new pairing has no pin. Creating this context is the explicit start of the
/// user-authorized pairing flow; it cannot clear an existing context's stop state.
pub struct PairingContext {
    base: BaseUrl,
    checked: Option<CheckedConnection>,
}
#[derive(Clone, Copy, Debug)]
pub enum Identity<'a> {
    Pinned(&'a Pin),
    Pairing {
        base: &'a BaseUrl,
        instance_id: &'a str,
    },
}
impl PairingContext {
    pub fn base(&self) -> &BaseUrl {
        &self.base
    }
    pub fn new(base: BaseUrl) -> Self {
        Self {
            base,
            checked: None,
        }
    }
    pub fn open(&mut self) {
        self.checked = None;
    }
    pub fn checked(
        &mut self,
        connection: &str,
        evidence: CheckedConnection,
    ) -> Result<(), VerificationError> {
        self.checked = None;
        if evidence.pairing_instance(connection).is_none() {
            return Err(VerificationError::ContextMismatch);
        }
        self.checked = Some(evidence);
        Ok(())
    }
    pub fn authorize(
        &self,
        request: &PreparedRequest,
        connection: &str,
    ) -> Result<Authorization<'_>, Diagnostic> {
        if request.base().serialized() != self.base.serialized()
            || !matches!(
                request.purpose(),
                Purpose::Rpc(Method::StartPairing | Method::PollPairing)
            )
        {
            return Err(local(Reason::origin_not_allowed));
        }
        let Some(instance_id) = self
            .checked
            .as_ref()
            .and_then(|evidence| evidence.pairing_instance(connection))
        else {
            return Ok(Authorization::Pending(Prerequisite::Discovery));
        };
        let purpose = request.purpose().credential();
        if purpose == CredentialPurpose::None {
            return Ok(Authorization::None);
        }
        Ok(Authorization::Read(ReadPermit {
            identity: Identity::Pairing {
                base: &self.base,
                instance_id,
            },
            purpose,
        }))
    }
}
