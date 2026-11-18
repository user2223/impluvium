// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: 0BSD

//! Pure acceptance-set replacements. Persistence and atomic installation are the
//! caller's responsibility. A full set waits for an explicit removal/revocation.

use super::{MAX_TTL_SECONDS, Time, nanos};
use crate::keys::{ConfigurationError, DepotTicket, KeyInput, TicketStatus, TrustedKeySet};
use crate::proto::impluvium::signed::v1::KeyRole;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Entry<K> {
    pub key: K,
    pub status: TicketStatus,
}
/// Prevalidated logical keys for the state substage; this model does not confer
/// cryptographic trust. Use `AcceptanceSet` for actual ticket verification.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct State<K> {
    pub keys: Vec<Entry<K>>,
    pub generation: u64,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Change<K> {
    Switch(K),
    Remove(K),
    HardRevoke(K),
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Outcome {
    Wait,
    Switched,
    Removed,
    Revoked,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StateError {
    InvalidState,
    DuplicateKey,
    UnknownKey,
    EmptySet,
    InvalidGenerationStep,
    GenerationExhausted,
}
impl<K: Clone + Eq> State<K> {
    /// Return a replacement without mutating the supplied state, including on
    /// exhaustion. A positive step is caller policy, not a protocol counter unit.
    pub fn apply(
        &self,
        change: Change<K>,
        now: Time,
        generation_step: u64,
    ) -> Result<(Self, Outcome), StateError> {
        if generation_step == 0 {
            return Err(StateError::InvalidGenerationStep);
        }
        if self.keys.is_empty()
            || self.keys.len() > 8
            || self
                .keys
                .iter()
                .filter(|k| k.status == TicketStatus::Current)
                .count()
                > 1
            || self
                .keys
                .iter()
                .any(|k| matches!(k.status, TicketStatus::Retiring { switch_time: None }))
            || self
                .keys
                .iter()
                .enumerate()
                .any(|(i, k)| self.keys[..i].iter().any(|p| p.key == k.key))
        {
            return Err(StateError::InvalidState);
        }
        let mut next = self.clone();
        let hard = matches!(&change, Change::HardRevoke(_));
        let outcome = match change {
            Change::Switch(key) => {
                if self.keys.iter().any(|k| k.key == key) {
                    return Err(StateError::DuplicateKey);
                }
                if self.keys.len() == 8 {
                    return Ok((next, Outcome::Wait));
                }
                for entry in &mut next.keys {
                    if entry.status == TicketStatus::Current {
                        entry.status = TicketStatus::Retiring {
                            switch_time: Some(now),
                        };
                    }
                }
                next.keys.push(Entry {
                    key,
                    status: TicketStatus::Current,
                });
                Outcome::Switched
            }
            Change::Remove(key) | Change::HardRevoke(key) => {
                let index = self
                    .keys
                    .iter()
                    .position(|k| k.key == key)
                    .ok_or(StateError::UnknownKey)?;
                if !hard {
                    let removable = match self.keys[index].status {
                        TicketStatus::Retiring {
                            switch_time: Some(switch),
                        } => {
                            nanos(now) - nanos(switch)
                                >= i128::from(MAX_TTL_SECONDS) * 1_000_000_000
                        }
                        _ => false,
                    };
                    if !removable {
                        return Ok((next, Outcome::Wait));
                    }
                }
                if self.keys.len() == 1 {
                    return Err(StateError::EmptySet);
                }
                next.keys.remove(index);
                if hard {
                    Outcome::Revoked
                } else {
                    Outcome::Removed
                }
            }
        };
        next.generation = self
            .generation
            .checked_add(generation_step)
            .ok_or(StateError::GenerationExhausted)?;
        Ok((next, outcome))
    }
}

/// Validated ticket-key configuration plus its caller-managed set generation.
#[derive(Debug)]
pub struct AcceptanceSet {
    trusted: TrustedKeySet<DepotTicket>,
    generation: u64,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChangeError {
    State(StateError),
    Configuration(ConfigurationError),
}
impl AcceptanceSet {
    /// Validate local configuration before any ticket bytes are read. Callers
    /// importing replacements must advance generation on every set change and
    /// must never reuse a generation for a different set while caches exist.
    pub fn new(generation: u64, inputs: &[KeyInput<'_>]) -> Result<Self, ConfigurationError> {
        Ok(Self {
            trusted: TrustedKeySet::new(inputs)?,
            generation,
        })
    }
    pub fn trusted(&self) -> &TrustedKeySet<DepotTicket> {
        &self.trusted
    }
    pub fn generation(&self) -> u64 {
        self.generation
    }
    pub fn state(&self) -> State<[u8; 32]> {
        State {
            generation: self.generation,
            keys: self
                .trusted
                .keys()
                .iter()
                .map(|k| Entry {
                    key: *k.public_key(),
                    status: k.ticket_status().expect("validated ticket key set"),
                })
                .collect(),
        }
    }
    pub fn apply(
        &self,
        change: Change<[u8; 32]>,
        now: Time,
        generation_step: u64,
    ) -> Result<(Self, Outcome), ChangeError> {
        let (state, outcome) = self
            .state()
            .apply(change, now, generation_step)
            .map_err(ChangeError::State)?;
        let inputs = state
            .keys
            .iter()
            .map(|k| KeyInput {
                public_key: &k.key,
                role: KeyRole::DepotTicket,
                ticket_status: Some(k.status),
            })
            .collect::<Vec<_>>();
        let next = Self::new(state.generation, &inputs).map_err(ChangeError::Configuration)?;
        Ok((next, outcome))
    }
}
