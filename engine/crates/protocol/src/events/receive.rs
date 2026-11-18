// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: 0BSD

use super::{Epoch, MAX_EVENT_BYTES, Position, library::Library};
use crate::{
    api::fields,
    decode::{self, Diagnostic, InputLimit, Reason, error},
    limits::MessageKind,
    proto::impluvium::v2::{ServerEvent, server_event::Payload},
};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Receiver {
    pub epoch: Option<Epoch>,
    /// Highest received data sequence, distinct from the library's applied one.
    pub last_data: u64,
}
#[derive(Clone, Debug)]
pub struct Received {
    pub event: ServerEvent,
    pub data_event: bool,
}

struct Frame<'a> {
    name: &'a str,
    data: &'a [u8],
    id: Option<&'a [u8]>,
}

// SSE line endings are CR, LF or CRLF. Borrow line contents, so even comments
// and unknown names consume no additional buffer. The adapter passes one whole
// event including its terminating blank line, not a never-ending byte stream.
fn frame(bytes: &[u8]) -> Result<Frame<'_>, Diagnostic> {
    let malformed = error(Reason::event_malformed, "");
    let mut rest = bytes;
    let (mut name, mut data, mut id) = (None, None, None);
    let mut ended = false;
    while !rest.is_empty() {
        let end = rest
            .iter()
            .position(|b| matches!(b, b'\r' | b'\n'))
            .ok_or(malformed)?;
        let line = &rest[..end];
        let delimiter = if rest[end] == b'\r' && rest.get(end + 1) == Some(&b'\n') {
            2
        } else {
            1
        };
        rest = &rest[end + delimiter..];
        if line.is_empty() {
            ended = true;
            if !rest.is_empty() {
                return Err(malformed);
            }
            break;
        }
        if line[0] == b':' {
            continue;
        }
        let split = line.iter().position(|b| *b == b':');
        let (field, value) = split.map_or((line, &b""[..]), |at| (&line[..at], &line[at + 1..]));
        let value = value.strip_prefix(b" ").unwrap_or(value);
        match field {
            b"event" => {
                if name.is_some() || value.is_empty() {
                    return Err(malformed);
                }
                name = Some(std::str::from_utf8(value).map_err(|_| malformed)?);
            }
            b"data" => {
                if data.replace(value).is_some() {
                    return Err(malformed);
                }
            }
            // WHATWG SSE retains the last id field. Invalid id text is left for
            // receive step 8; it must not outrank the JSON or field checks.
            b"id" => id = Some(value),
            _ => (),
        }
    }
    if !ended {
        return Err(malformed);
    }
    Ok(Frame {
        name: name.ok_or(malformed)?,
        data: data.ok_or(malformed)?,
        id,
    })
}
fn member(name: &str) -> Option<(u32, &'static str)> {
    Some(match name {
        "connected" => (10, "ServerEvent.connected"),
        "heartbeat" => (11, "ServerEvent.heartbeat"),
        "resync_required" => (12, "ServerEvent.resync_required"),
        "session_ended" => (13, "ServerEvent.session_ended"),
        "superseded" => (14, "ServerEvent.superseded"),
        "library_changed" => (20, "ServerEvent.library_changed"),
        _ => return None,
    })
}
impl Receiver {
    /// Validate in the complete order of events.md §7. Errors leave this
    /// snapshot untouched; the owning client marks the connection broken.
    pub fn receive(self, bytes: &[u8]) -> Result<(Self, Received), Diagnostic> {
        if bytes.len() > MAX_EVENT_BYTES {
            return Err(error(Reason::event_too_large, ""));
        }
        let f = frame(bytes)?;
        let p = decode::project_json(
            MessageKind::ServerEvent,
            f.data,
            InputLimit {
                reason: Reason::event_too_large,
                ..InputLimit::body(MAX_EVENT_BYTES)
            },
        )?;
        if self.epoch.is_none() != (f.name == "connected") {
            return Err(error(Reason::event_out_of_order, ""));
        }
        let known = p
            .fields
            .iter()
            .find(|(n, _)| matches!(*n, 10..=14 | 20))
            .map(|(n, _)| *n);
        if let Some((number, field)) = member(f.name) {
            if !p.fields.iter().any(|(n, _)| *n == number) {
                return Err(error(Reason::missing_field, field));
            }
            if known != Some(number) {
                return Err(error(Reason::event_name_mismatch, ""));
            }
        } else if known.is_some() {
            return Err(error(Reason::event_name_mismatch, ""));
        }
        fields::validate(&p)?;
        let event: ServerEvent = decode::decode_projection(&p)?;
        // Exact length passed step 6; conversion cannot select another reason.
        let epoch: Epoch = event.epoch.as_slice().try_into().expect("validated epoch");
        if self.epoch.is_some_and(|expected| expected != epoch) {
            return Err(error(Reason::event_epoch_mismatch, "ServerEvent.epoch"));
        }
        let id_required = matches!(f.name, "library_changed" | "heartbeat");
        if (id_required && f.id.is_none())
            || (member(f.name).is_some() && !id_required && f.id.is_some())
        {
            return Err(error(Reason::event_id_mismatch, ""));
        }
        if let Some(id) = f.id {
            let id = std::str::from_utf8(id).ok().and_then(Position::parse);
            if id
                != Some(Position {
                    epoch,
                    seq: event.seq,
                })
            {
                return Err(error(Reason::event_id_mismatch, ""));
            }
        }
        let data_event =
            f.name == "library_changed" || (member(f.name).is_none() && f.id.is_some());
        if (data_event
            && (event.seq == 0 || event.prev_seq >= event.seq || event.seq <= self.last_data))
            || (f.name == "heartbeat" && event.seq < self.last_data)
        {
            return Err(error(Reason::event_sequence_invalid, ""));
        }
        if let Some(Payload::LibraryChanged(change)) = &event.payload {
            if change.game.is_set() == change.deleted {
                return Err(error(Reason::event_payload_invalid, "LibraryChanged.game"));
            }
            if !change.deleted
                && (change.game.game_id != change.game_id || change.game.entry_version != event.seq)
            {
                return Err(error(Reason::event_identity_mismatch, ""));
            }
        }
        Ok((
            Self {
                epoch: Some(epoch),
                last_data: if data_event {
                    event.seq
                } else {
                    self.last_data
                },
            },
            Received { event, data_event },
        ))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Lifecycle {
    Open,
    /// Reconnect with backoff using Library::resume_point, never a received id.
    Broken(Diagnostic),
    /// Rebuild and reconnect with backoff (OVERFLOW).
    Reconnect,
    /// Discard the topic; wait until a stream for it is accepted.
    AccessChanged,
    /// Confirm device state at the next RPC; never delete credentials here.
    SessionEnded,
    /// Wait for coordination before reusing this client identifier.
    Superseded,
}
#[derive(Clone, Debug)]
pub struct Client {
    pub receiver: Receiver,
    pub library: Library,
    pub lifecycle: Lifecycle,
}
impl Client {
    pub fn new(library: Library) -> Self {
        Self {
            receiver: Receiver::default(),
            library,
            lifecycle: Lifecycle::Open,
        }
    }
    /// Reset receive validation for a connection reopened with the same
    /// credential and identifier. Terminal restrictions require a new Client
    /// after fresh authentication, topic admission or external coordination.
    pub fn reconnect(&self) -> Option<Self> {
        if !matches!(
            self.lifecycle,
            Lifecycle::Open | Lifecycle::Broken(_) | Lifecycle::Reconnect
        ) {
            return None;
        }
        let mut next = self.clone();
        next.receiver = Receiver::default();
        next.lifecycle = Lifecycle::Open;
        Some(next)
    }
    pub fn opening_refused(&self, rejection: &super::open::Rejection) -> Self {
        let mut next = self.clone();
        if rejection.status == 403 {
            next.library = next.library.discard();
            next.lifecycle = Lifecycle::AccessChanged;
        }
        next
    }
    pub fn receive(&self, bytes: &[u8]) -> Self {
        if self.lifecycle != Lifecycle::Open {
            return self.clone();
        }
        let mut next = self.clone();
        let (receiver, received) = match self.receiver.receive(bytes) {
            Ok(value) => value,
            Err(d) => {
                next.lifecycle = Lifecycle::Broken(d);
                return next;
            }
        };
        next.receiver = receiver;
        let event = received.event;
        match event.payload.as_ref() {
            Some(Payload::Connected(_)) => {
                next.library = next
                    .library
                    .connected(receiver.epoch.expect("connected epoch"))
                    .0;
            }
            Some(Payload::Heartbeat(_)) => {
                next.library = next.library.heartbeat(event.seq).0;
            }
            Some(Payload::ResyncRequired(reason)) => {
                let reason = reason.reason.to_i32();
                if reason == 6 {
                    next.library = next.library.discard();
                    next.lifecycle = Lifecycle::AccessChanged;
                } else {
                    next.library = next.library.begin_rebuild().0;
                    if reason == 5 {
                        next.lifecycle = Lifecycle::Reconnect;
                    }
                }
            }
            Some(Payload::SessionEnded(_)) => next.lifecycle = Lifecycle::SessionEnded,
            Some(Payload::Superseded(_)) => next.lifecycle = Lifecycle::Superseded,
            _ if received.data_event => {
                let change = match event.payload {
                    Some(Payload::LibraryChanged(change)) => Some(*change),
                    _ => None,
                };
                next.library = next.library.data(event.seq, event.prev_seq, change).0;
            }
            _ => (),
        }
        next
    }
}
