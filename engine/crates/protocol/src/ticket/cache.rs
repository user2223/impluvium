// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: 0BSD

//! Pure cache transitions over a supplied snapshot, with no storage or clock.

use super::{
    AcceptanceSet, Context, Time, ValidKey, VerifiedTicket, exp, nanos, validate_claims,
    verify_bindings,
};
use crate::{
    decode::{Diagnostic, Reason, error},
    ids::ManifestId,
    proto::impluvium::signed::v1::KeyRole,
};

/// Cache identity uses exact binary bytes, role and acceptance-set generation.
/// A hash may index these bytes externally, but cannot replace their equality.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Identity<'a> {
    pub ticket: &'a [u8],
    pub role: KeyRole,
    pub generation: u64,
}
#[derive(Clone, Debug)]
struct Entry<T> {
    ticket: Vec<u8>,
    role: KeyRole,
    generation: u64,
    expires: Time,
    value: T,
}
/// A one-entry injected-state model. `T` and `fresh` are trusted observations
/// from the caller, not cryptographic evidence. Use `Cache` with `verify_cached`
/// for authenticated tickets. Capacity/eviction policies belong to the caller.
#[derive(Clone, Debug)]
pub struct State<T> {
    entry: Option<Entry<T>>,
}
impl<T> Default for State<T> {
    fn default() -> Self {
        Self { entry: None }
    }
}
impl<T: Clone> State<T> {
    /// A hit requires exact identity and an unexpired entry. Time and request
    /// checks run on every hit. Failures never replace the previous snapshot.
    pub fn step<E>(
        &self,
        identity: Identity<'_>,
        now: Time,
        fresh: impl FnOnce() -> Result<(T, Time), E>,
        recheck: impl FnOnce(&T) -> Result<(), E>,
    ) -> (Self, bool, Result<T, E>) {
        if let Some(entry) = &self.entry
            && entry.ticket == identity.ticket
            && entry.role == identity.role
            && entry.generation == identity.generation
            && nanos(now) < nanos(entry.expires)
        {
            let result = recheck(&entry.value).map(|()| entry.value.clone());
            return (self.clone(), true, result);
        }
        let (value, expires) = match fresh() {
            Ok(v) => v,
            Err(e) => return (self.clone(), false, Err(e)),
        };
        if let Err(e) = recheck(&value) {
            return (self.clone(), false, Err(e));
        }
        // Do not retain an expired injected observation even if its caller
        // supplied a permissive recheck. Such an entry can never produce a hit.
        let next = if nanos(now) < nanos(expires) {
            Self {
                entry: Some(Entry {
                    ticket: identity.ticket.to_vec(),
                    role: identity.role,
                    generation: identity.generation,
                    expires,
                    value: value.clone(),
                }),
            }
        } else {
            self.clone()
        };
        (next, false, Ok(value))
    }
}

/// Only this wrapper can be used for authenticated cache verification. Its state
/// cannot be populated with the logical model's injected observations.
#[derive(Clone, Debug, Default)]
pub struct Cache {
    state: State<VerifiedTicket>,
}

/// Full verification as a pure state transition. A set-generation change forces
/// authentication again. The returned snapshot replaces the caller's cache;
/// the boolean records whether an eligible entry was rechecked.
pub fn verify_cached(
    cache: &Cache,
    bytes: &[u8],
    set: &AcceptanceSet,
    context: &Context<'_>,
    requested: &ManifestId,
) -> (Cache, bool, Result<VerifiedTicket, Diagnostic>) {
    let (state, hit, result) = cache.state.step(
        Identity {
            ticket: bytes,
            role: KeyRole::DepotTicket,
            generation: set.generation(),
        },
        context.now,
        || {
            verify_bindings(bytes, set.trusted(), context).map(|ticket| {
                let expires = exp(ticket.claims());
                (ticket, expires)
            })
        },
        |ticket| {
            // Resolve every cached signature against the supplied set again.
            // Under the generation invariant these are the original statuses.
            let keys = ticket
                .valid_keys()
                .iter()
                .map(|valid| {
                    let key = set
                        .trusted()
                        .keys()
                        .iter()
                        .find(|k| k.key_id() == valid.key_id)
                        .ok_or_else(|| error(Reason::no_trusted_signature, ""))?;
                    Ok(ValidKey {
                        key_id: key.key_id(),
                        status: key.ticket_status().expect("validated ticket key set"),
                    })
                })
                .collect::<Result<Vec<_>, Diagnostic>>()?;
            validate_claims(ticket.claims(), &keys, context)?;
            ticket.check_scope(requested)
        },
    );
    (Cache { state }, hit, result)
}
