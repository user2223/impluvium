// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: 0BSD

//! Depot tickets: exact-byte authentication, injected time and explicit bindings.

pub use crate::proto::impluvium::v2::DepotTicketClaims;
use crate::{
    decode::{self, Diagnostic, InputLimit, Reason, error},
    envelope,
    ids::{KeyId, ManifestId},
    keys::{DepotTicket, SigningKey, SwitchTime, TicketStatus, TrustedKeySet},
    limits,
    manifest::uuid,
    proto::impluvium::signed::v1::Envelope,
};
use buffa::Message;

pub mod acceptance;
pub mod cache;
pub use acceptance::AcceptanceSet;

/// Caller-supplied server time, in seconds and nanoseconds. No clock is read.
pub type Time = SwitchTime;
pub const MAX_TTL_SECONDS: u32 = 3_600;
pub const MIN_ISSUED_TTL_SECONDS: u32 = 300;
pub const DEFAULT_TTL_SECONDS: u32 = MAX_TTL_SECONDS;
pub const MAX_BINARY_BYTES: usize = limits::SMALL_ENVELOPE_BYTES;
pub const MAX_TOKEN_CHARACTERS: usize = (MAX_BINARY_BYTES * 8).div_ceil(6);
pub const MAX_AUTHORIZATION_BYTES: usize = b"Bearer ".len() + MAX_TOKEN_CHARACTERS;

// Widen before arithmetic: even caller-supplied i64 extremes cannot overflow.
pub(crate) fn nanos(t: Time) -> i128 {
    i128::from(t.seconds) * 1_000_000_000 + i128::from(t.nanos)
}
fn iat(c: &DepotTicketClaims) -> Time {
    Time {
        seconds: c.iat.seconds,
        nanos: c.iat.nanos as u32,
    }
}
fn exp(c: &DepotTicketClaims) -> Time {
    Time {
        seconds: c.exp.seconds,
        nanos: c.exp.nanos as u32,
    }
}

/// Expected instance and optional device/user bindings, supplied by the verifier.
#[derive(Clone, Copy, Debug)]
pub struct Context<'a> {
    pub now: Time,
    pub audience: &'a str,
    pub subject: Option<&'a str>,
    pub user_id: Option<&'a str>,
}

/// Metadata for all valid trusted signatures, in envelope order. This is not
/// evidence of authentication by itself; `verify` obtains it from the envelope.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ValidKey {
    pub key_id: KeyId,
    pub status: TicketStatus,
}

/// A ticket that passed authentication and checks 1–11. Scope is a separate
/// operation so a depot can defer it until after path decoding (tickets.md §7).
#[derive(Clone, Debug)]
pub struct VerifiedTicket {
    claims: DepotTicketClaims,
    valid_keys: Vec<ValidKey>,
}
impl VerifiedTicket {
    pub fn claims(&self) -> &DepotTicketClaims {
        &self.claims
    }
    pub fn valid_keys(&self) -> &[ValidKey] {
        &self.valid_keys
    }
    pub fn check_scope(&self, requested: &ManifestId) -> Result<(), Diagnostic> {
        check_scope(&self.claims, requested)
    }
}

/// Bounded payload scan only, including timestamp ranges, without authentication
/// or semantic checks. Useful for producers; never establishes trust.
pub fn decode_payload(bytes: &[u8]) -> Result<DepotTicketClaims, Diagnostic> {
    decode::decode_binary(bytes, InputLimit::payload(MAX_BINARY_BYTES))
}

/// Claims checks in tickets.md §4 order (1–11). Inputs to this substage must have
/// passed the payload scan; valid-key metadata must come from authentication.
pub fn validate_claims(
    c: &DepotTicketClaims,
    valid_keys: &[ValidKey],
    context: &Context<'_>,
) -> Result<(), Diagnostic> {
    use Reason::*;
    if !c.iat.is_set() || !c.exp.is_set() {
        return Err(error(missing_field, ""));
    }
    let duration = nanos(exp(c)) - nanos(iat(c));
    if duration <= 0 || duration > i128::from(MAX_TTL_SECONDS) * 1_000_000_000 {
        return Err(error(invalid_validity_window, ""));
    }
    recheck_time(c, valid_keys, context.now)?;
    if !uuid(&c.aud) {
        return Err(error(invalid_uuid, "DepotTicketClaims.aud"));
    }
    if c.aud != context.audience {
        return Err(error(wrong_audience, "DepotTicketClaims.aud"));
    }
    for (value, field) in [
        (&c.sub, "DepotTicketClaims.sub"),
        (&c.user_id, "DepotTicketClaims.user_id"),
    ] {
        if !uuid(value) {
            return Err(error(invalid_uuid, field));
        }
    }
    if c.manifest_ids.is_empty() {
        return Err(error(missing_field, "DepotTicketClaims.manifest_ids"));
    }
    for (index, id) in c.manifest_ids.iter().enumerate() {
        if id.len() != 32 {
            return Err(error(invalid_manifest_id, "DepotTicketClaims.manifest_ids"));
        }
        if index > 0 && *id <= c.manifest_ids[index - 1] {
            return Err(error(
                unsorted_manifest_ids,
                "DepotTicketClaims.manifest_ids",
            ));
        }
    }
    check_subject(c, context)
}
fn recheck_time(c: &DepotTicketClaims, keys: &[ValidKey], now: Time) -> Result<(), Diagnostic> {
    if nanos(now) >= nanos(exp(c)) {
        return Err(error(Reason::ticket_expired, "DepotTicketClaims.exp"));
    }
    if nanos(iat(c)) > nanos(now) {
        return Err(error(Reason::ticket_not_yet_valid, "DepotTicketClaims.iat"));
    }
    for key in keys {
        if let TicketStatus::Retiring {
            switch_time: Some(switch),
        } = key.status
            && nanos(iat(c)) >= nanos(switch)
        {
            return Err(error(Reason::ticket_key_retired, "DepotTicketClaims.iat"));
        }
    }
    Ok(())
}
fn check_subject(c: &DepotTicketClaims, context: &Context<'_>) -> Result<(), Diagnostic> {
    if context.subject.is_some_and(|s| s != c.sub)
        || context.user_id.is_some_and(|u| u != c.user_id)
    {
        Err(error(Reason::wrong_subject, ""))
    } else {
        Ok(())
    }
}
/// Per-request scope check, tickets.md §4 step 12.
pub fn check_scope(c: &DepotTicketClaims, requested: &ManifestId) -> Result<(), Diagnostic> {
    if c.manifest_ids.iter().any(|id| id.as_slice() == requested) {
        Ok(())
    } else {
        Err(error(
            Reason::ticket_scope,
            "DepotTicketClaims.manifest_ids",
        ))
    }
}

/// Authenticate exact envelope bytes, scan the payload, then apply checks 1–11.
pub fn verify_bindings(
    bytes: &[u8],
    trusted: &TrustedKeySet<DepotTicket>,
    context: &Context<'_>,
) -> Result<VerifiedTicket, Diagnostic> {
    let authenticated = envelope::authenticate(bytes, envelope::DEPOT_TICKET, trusted)?;
    let claims = decode_payload(authenticated.payload())?;
    let valid_keys = authenticated
        .valid_keys()
        .iter()
        .map(|k| ValidKey {
            key_id: k.key_id(),
            status: k.ticket_status().expect("validated ticket key set"),
        })
        .collect::<Vec<_>>();
    validate_claims(&claims, &valid_keys, context)?;
    Ok(VerifiedTicket { claims, valid_keys })
}
/// Full ticket verification, including the requested manifest.
pub fn verify(
    bytes: &[u8],
    trusted: &TrustedKeySet<DepotTicket>,
    context: &Context<'_>,
    requested: &ManifestId,
) -> Result<VerifiedTicket, Diagnostic> {
    let ticket = verify_bindings(bytes, trusted, context)?;
    ticket.check_scope(requested)?;
    Ok(ticket)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IssueError {
    InvalidLifetime,
    InvalidTime,
    NotCurrentKey,
    Claims(Diagnostic),
}
/// Issuance inputs are an already resolved complete manifest set. Entitlement and
/// dependency resolution belong to the caller; the set is never shortened here.
pub struct Issue<'a> {
    pub audience: &'a str,
    pub subject: &'a str,
    pub user_id: &'a str,
    pub manifest_ids: &'a [ManifestId],
    pub now: Time,
    pub lifetime_seconds: u32,
}
/// Issue under the acceptance set's current key only. Claims are checked before
/// signing; the full verifier also checks the envelope before bytes are returned.
pub fn issue(
    input: Issue<'_>,
    set: &AcceptanceSet,
    key: &SigningKey<DepotTicket>,
) -> Result<Vec<u8>, IssueError> {
    if !(MIN_ISSUED_TTL_SECONDS..=MAX_TTL_SECONDS).contains(&input.lifetime_seconds) {
        return Err(IssueError::InvalidLifetime);
    }
    let public = key.public_key();
    if !set
        .trusted()
        .keys()
        .iter()
        .any(|k| k.public_key() == &public && k.ticket_status() == Some(TicketStatus::Current))
    {
        return Err(IssueError::NotCurrentKey);
    }
    if !(-62_135_596_800..=253_402_300_799).contains(&input.now.seconds)
        || input.now.nanos >= 1_000_000_000
    {
        return Err(IssueError::InvalidTime);
    }
    let end = input
        .now
        .seconds
        .checked_add(i64::from(input.lifetime_seconds))
        .filter(|s| *s <= 253_402_300_799)
        .ok_or(IssueError::InvalidTime)?;
    let claims = DepotTicketClaims {
        aud: input.audience.into(),
        sub: input.subject.into(),
        user_id: input.user_id.into(),
        manifest_ids: input.manifest_ids.iter().map(|id| id.to_vec()).collect(),
        iat: buffa_types::google::protobuf::Timestamp {
            seconds: input.now.seconds,
            nanos: input.now.nanos as i32,
            ..Default::default()
        }
        .into(),
        exp: buffa_types::google::protobuf::Timestamp {
            seconds: end,
            nanos: input.now.nanos as i32,
            ..Default::default()
        }
        .into(),
        ..Default::default()
    };
    let payload = claims.encode_to_vec();
    let checked = decode_payload(&payload).map_err(IssueError::Claims)?;
    let context = Context {
        now: input.now,
        audience: input.audience,
        subject: Some(input.subject),
        user_id: Some(input.user_id),
    };
    validate_claims(
        &checked,
        &[ValidKey {
            key_id: crate::ids::key_id(&public),
            status: TicketStatus::Current,
        }],
        &context,
    )
    .map_err(IssueError::Claims)?;
    let signature = envelope::sign_payload(envelope::DEPOT_TICKET, &payload, key)
        .map_err(IssueError::Claims)?;
    let bytes = Envelope {
        payload_type: envelope::DEPOT_TICKET.as_str().into(),
        payload,
        signatures: vec![signature],
        ..Default::default()
    }
    .encode_to_vec();
    verify_bindings(&bytes, set.trusted(), &context).map_err(IssueError::Claims)?;
    Ok(bytes)
}
