// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: 0BSD

//! Immutable pins and pure connection-state transitions. A successful check is
//! scoped to one connection identity supplied by the transport owner.

use super::{Codec, fields};
use crate::{
    decode::{Diagnostic, Reason, error},
    envelope,
    proto::impluvium::{
        signed::v1::KeyRole,
        v2::{Discovery, PollPairingResponse, ProtocolVersion},
    },
};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug)]
pub struct ClientVersion {
    pub major: u32,
    pub revision: u32,
    pub min_server_revision: u32,
}
impl Default for ClientVersion {
    fn default() -> Self {
        Self {
            major: 2,
            revision: 1,
            min_server_revision: 1,
        }
    }
}
/// Major, server minimum, client minimum, in that order. A larger revision by
/// itself is compatible and no revision decision requests credential cleanup.
pub fn check_version(server: &ProtocolVersion, client: ClientVersion) -> Result<(), Diagnostic> {
    let failure = if server.major != client.major {
        Some((Reason::protocol_unsupported, "ProtocolVersion.major"))
    } else if server.revision < client.min_server_revision {
        Some((Reason::server_too_old, "ProtocolVersion.revision"))
    } else if client.revision < server.min_client_revision {
        Some((
            Reason::client_too_old,
            "ProtocolVersion.min_client_revision",
        ))
    } else {
        None
    };
    failure.map_or(Ok(()), |(r, f)| Err(error(r, f)))
}
pub fn check_capabilities(required: &[String], implemented: &[String]) -> Result<(), Diagnostic> {
    if required.iter().all(|r| implemented.contains(r)) {
        Ok(())
    } else {
        Err(error(
            Reason::client_too_old,
            "Discovery.required_client_capabilities",
        ))
    }
}
/// Intersection preserves offer order. Unknown server capabilities grant no
/// behavior; reserved names must not be included in `implemented` by callers.
pub fn accepted_capabilities(offered: &[String], implemented: &[String]) -> Vec<String> {
    offered
        .iter()
        .filter(|c| implemented.contains(c))
        .cloned()
        .collect()
}

/// The base URL is owned by the containing credential record. This immutable
/// value cannot be replaced by discovery or by any failed pin comparison.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Pin {
    instance_id: String,
    content_public_key: [u8; 32],
}
impl Pin {
    pub fn instance_id(&self) -> &str {
        &self.instance_id
    }
    pub fn content_public_key(&self) -> &[u8; 32] {
        &self.content_public_key
    }
    /// Reconstitute a persisted, previously established pin; never call this
    /// with a discovery fingerprint in place of a pairing result's public key.
    pub fn from_stored(
        instance_id: String,
        content_public_key: [u8; 32],
    ) -> Result<Self, Diagnostic> {
        if !crate::manifest::uuid(&instance_id) {
            return Err(error(Reason::invalid_uuid, ""));
        }
        envelope::check_point(&content_public_key)
            .map_err(|_| error(Reason::invalid_trusted_key, ""))?;
        Ok(Self {
            instance_id,
            content_public_key,
        })
    }
}
fn fingerprints_match(d: &Discovery, key: &[u8]) -> bool {
    let fingerprints: Vec<_> = d
        .key_fingerprints
        .iter()
        .filter(|f| f.role.to_i32() == KeyRole::Content as i32 && f.sha256.len() == 32)
        .collect();
    let hash = Sha256::digest(key);
    fingerprints.is_empty()
        || fingerprints
            .iter()
            .any(|f| f.sha256.as_slice() == &hash[..])
}

/// A successful discovery check bound to its transport connection and pin.
/// Only `check_discovery` can construct this single-use evidence. An unpinned
/// pairing check cannot authorize credentials for a pinned trust context.
#[derive(Debug)]
pub struct CheckedConnection {
    connection: String,
    instance_id: String,
    pin: Option<Pin>,
}

/// The caller has already completed credential-free discovery transport and
/// profile checks on this connection (trust.md §4.1 steps 1–2).
/// Revalidating the supplied model enforces steps 3–6 even for constructed data.
/// `connection` identifies that physical connection, not a pool or origin.
pub fn check_discovery(
    connection: &str,
    d: &Discovery,
    pin: Option<&Pin>,
    client: ClientVersion,
    implemented: &[String],
) -> Result<CheckedConnection, Diagnostic> {
    use buffa::Message;
    let p = super::project(
        crate::limits::MessageKind::Discovery,
        Codec::Binary,
        &d.encode_to_vec(),
        crate::decode::InputLimit::response(65_536),
    )?;
    super::discovery_fields(&p)?;
    if let Some(pin) = pin {
        if d.instance_id != pin.instance_id {
            return Err(error(Reason::instance_mismatch, "Discovery.instance_id"));
        }
        if !fingerprints_match(d, &pin.content_public_key) {
            return Err(error(
                Reason::content_key_changed,
                "Discovery.key_fingerprints",
            ));
        }
    }
    check_version(&d.protocol, client)?;
    check_capabilities(&d.required_client_capabilities, implemented)?;
    Ok(CheckedConnection {
        connection: connection.into(),
        instance_id: d.instance_id.clone(),
        pin: pin.cloned(),
    })
}

/// PollPairingResponse has its own chapter order, including point checks after
/// the instance comparison. Only a successful result can create a replacement pin.
pub fn decode_pairing_result(
    codec: Codec,
    bytes: &[u8],
    discovery: &Discovery,
) -> Result<(PollPairingResponse, Pin), Diagnostic> {
    let p = super::project(
        crate::limits::MessageKind::PollPairingResponse,
        codec,
        bytes,
        crate::decode::InputLimit::response(65_536),
    )?;
    fields::validate(&p)?;
    let response: PollPairingResponse = crate::decode::decode_projection(&p)?;
    if response.instance_id != discovery.instance_id {
        return Err(error(
            Reason::instance_mismatch,
            "PollPairingResponse.instance_id",
        ));
    }
    envelope::check_point(&response.content_public_key).map_err(|_| {
        error(
            Reason::invalid_trusted_key,
            "PollPairingResponse.content_public_key",
        )
    })?;
    if !fingerprints_match(discovery, &response.content_public_key) {
        return Err(error(
            Reason::content_key_changed,
            "PollPairingResponse.content_public_key",
        ));
    }
    let pin = Pin {
        instance_id: response.instance_id.clone(),
        content_public_key: response
            .content_public_key
            .as_slice()
            .try_into()
            .expect("field rule checked key length"),
    };
    Ok((response, pin))
}

/// Evidence binding failures are local API misuse, not protocol diagnostics.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VerificationError {
    ConnectionMismatch,
    ContextMismatch,
    Stopped(Diagnostic),
}

#[derive(Clone, Debug)]
pub struct Connections {
    pin: Pin,
    verified: BTreeSet<String>,
    stopped: Option<Diagnostic>,
}
impl Connections {
    /// A new or replaced trust context starts with no verified connections.
    pub fn new(pin: &Pin) -> Self {
        Self {
            pin: pin.clone(),
            verified: BTreeSet::new(),
            stopped: None,
        }
    }
    /// A fresh or replacement connection never inherits verification.
    pub fn open(&self, connection: &str) -> Self {
        let mut next = self.clone();
        next.verified.remove(connection);
        next
    }
    pub fn stopped(&self) -> Option<Diagnostic> {
        self.stopped
    }
    pub fn credential_allowed(&self, connection: &str) -> bool {
        self.stopped.is_none() && self.verified.contains(connection)
    }
    /// Consume evidence only for this connection and this context's exact pin.
    /// Mismatched evidence never verifies the requested connection.
    pub fn checked(
        &self,
        connection: &str,
        check: CheckedConnection,
    ) -> (Self, Result<(), VerificationError>) {
        if let Some(d) = self.stopped {
            return (
                self.clone(),
                Err(VerificationError::Stopped(error(d.reason, ""))),
            );
        }
        let mut next = self.clone();
        next.verified.remove(connection);
        if check.connection != connection {
            return (next, Err(VerificationError::ConnectionMismatch));
        }
        if check.pin.as_ref() != Some(&self.pin) {
            return (next, Err(VerificationError::ContextMismatch));
        }
        next.verified.insert(connection.into());
        (next, Ok(()))
    }

    /// Failed discovery invalidates this connection. Trust mismatch states
    /// latch until an explicit, successfully completed new pairing.
    pub fn rejected(
        &self,
        connection: &str,
        diagnostic: Diagnostic,
    ) -> (Self, Result<(), Diagnostic>) {
        if let Some(d) = self.stopped {
            return (self.clone(), Err(error(d.reason, "")));
        }
        let mut next = self.clone();
        next.verified.remove(connection);
        if matches!(
            diagnostic.reason,
            Reason::instance_mismatch | Reason::content_key_changed
        ) {
            next.stopped = Some(diagnostic);
            next.verified.clear();
        }
        (next, Err(diagnostic))
    }
}

impl CheckedConnection {
    /// Unpinned discovery evidence authorizes pairing only on its own physical
    /// connection. It never authorizes a pinned context's token or cleanup.
    pub fn pairing_instance(&self, connection: &str) -> Option<&str> {
        (self.pin.is_none() && self.connection == connection).then_some(self.instance_id.as_str())
    }
}
