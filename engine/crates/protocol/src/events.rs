// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: 0BSD

//! Pure event contracts (events.md). HTTP adapters supply a protected request
//! head and credential observations; callers serialize returned state changes.
//! No transition reads a clock, writes a stream, elects a leader or owns a queue.

use crate::proto::impluvium::v2::ResyncReason;

pub mod library;
pub mod open;
mod receive;
pub use receive::{Client, Lifecycle, Received, Receiver};

pub const MAX_EVENT_BYTES: usize = 65_536;
pub const MAX_CLIENT_IDENTIFIERS: usize = 256;
pub type Epoch = [u8; 16];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Position {
    pub epoch: Epoch,
    pub seq: u64,
}
impl Position {
    /// Syntax and length are checked before decimal conversion (§6).
    pub fn parse(text: &str) -> Option<Self> {
        let b = text.as_bytes();
        if !(34..=53).contains(&b.len())
            || b[32] != b'-'
            || !b[..32]
                .iter()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(c))
            || !b[33..].iter().all(u8::is_ascii_digit)
            || (b.len() > 34 && b[33] == b'0')
        {
            return None;
        }
        let mut epoch = [0; 16];
        for (i, pair) in b[..32].as_chunks::<2>().0.iter().enumerate() {
            let nibble = |c: u8| if c <= b'9' { c - b'0' } else { c - b'a' + 10 };
            epoch[i] = nibble(pair[0]) * 16 + nibble(pair[1]);
        }
        Some(Self {
            epoch,
            seq: text[33..].parse().ok()?,
        })
    }
    pub fn id(self) -> String {
        use std::fmt::Write;
        let mut text = String::with_capacity(53);
        for b in self.epoch {
            write!(text, "{b:02x}").expect("writing into String");
        }
        write!(text, "-{}", self.seq).expect("writing into String");
        text
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Resume {
    pub resumed: bool,
    pub reason: Option<ResyncReason>,
    pub last_written: u64,
}
/// `highest_missing` describes retention over the user's entire sequence,
/// before topic/entry filtering. A failed resume leaves the stream open.
pub fn resume(point: Option<&str>, head: Position, highest_missing: Option<u64>) -> Resume {
    let mut result = Resume {
        resumed: false,
        reason: None,
        last_written: head.seq,
    };
    let Some(text) = point else { return result };
    let Some(point) = Position::parse(text) else {
        result.reason = Some(ResyncReason::Malformed);
        return result;
    };
    result.reason = if point.epoch != head.epoch {
        Some(ResyncReason::ForeignEpoch)
    } else if point.seq > head.seq {
        Some(ResyncReason::Ahead)
    } else if highest_missing.is_some_and(|seq| seq > point.seq) {
        Some(ResyncReason::TooOld)
    } else {
        result.resumed = true;
        result.last_written = point.seq;
        None
    };
    result
}

/// The producer must reserve a sequence before committing its corresponding
/// library mutation. `None` forbids both the event and that mutation (§3.2).
pub fn next_sequence(head: u64) -> Option<u64> {
    head.checked_add(1)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Credential {
    Active,
    Revoked,
    Expired,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WriteDecision {
    Skip,
    Data { seq: u64, prev_seq: u64 },
    Heartbeat { seq: u64, prev_seq: u64 },
    SessionEnded(crate::proto::impluvium::v2::SessionEndReason),
    Superseded,
    ResyncRequired(ResyncReason),
    Closed,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StreamChain {
    pub last_written: u64,
    pub closed: bool,
}
impl StreamChain {
    /// Rechecked before every wait and write. Actual queueing and output belong
    /// to the adapter; its overflow is an explicit terminal transition.
    pub fn credential(self, credential: Credential) -> (Self, Option<WriteDecision>) {
        use crate::proto::impluvium::v2::SessionEndReason;
        if self.closed {
            return (self, Some(WriteDecision::Closed));
        }
        let reason = match credential {
            Credential::Active => return (self, None),
            Credential::Revoked => SessionEndReason::Revoked,
            Credential::Expired => SessionEndReason::Expired,
        };
        (
            Self {
                closed: true,
                ..self
            },
            Some(WriteDecision::SessionEnded(reason)),
        )
    }
    pub fn data(
        self,
        seq: u64,
        credential: Credential,
        topics_allowed: bool,
        subscribed: bool,
        deleted: bool,
        entry_allowed: bool,
    ) -> (Self, WriteDecision) {
        let (state, terminal) = self.credential(credential);
        if let Some(decision) = terminal {
            return (state, decision);
        }
        if !topics_allowed {
            return state.overflow_or_access(ResyncReason::AccessChanged);
        }
        if !subscribed || (!deleted && !entry_allowed) {
            return (state, WriteDecision::Skip);
        }
        // Input events are committed user events in increasing sequence order.
        if seq <= state.last_written {
            return (state, WriteDecision::Skip);
        }
        (
            Self {
                last_written: seq,
                ..state
            },
            WriteDecision::Data {
                seq,
                prev_seq: state.last_written,
            },
        )
    }
    fn overflow_or_access(self, reason: ResyncReason) -> (Self, WriteDecision) {
        if self.closed {
            return (self, WriteDecision::Closed);
        }
        (
            Self {
                closed: true,
                ..self
            },
            WriteDecision::ResyncRequired(reason),
        )
    }
    pub fn overflow(self, credential: Credential) -> (Self, WriteDecision) {
        let (state, terminal) = self.credential(credential);
        if let Some(decision) = terminal {
            return (state, decision);
        }
        state.overflow_or_access(ResyncReason::Overflow)
    }
    pub fn heartbeat(self, credential: Credential) -> (Self, WriteDecision) {
        let (state, terminal) = self.credential(credential);
        (
            state,
            terminal.unwrap_or(WriteDecision::Heartbeat {
                seq: state.last_written,
                prev_seq: 0,
            }),
        )
    }
    pub fn superseded(self, credential: Credential) -> (Self, WriteDecision) {
        let (state, terminal) = self.credential(credential);
        if let Some(decision) = terminal {
            return (state, decision);
        }
        (
            Self {
                closed: true,
                ..state
            },
            WriteDecision::Superseded,
        )
    }
}
