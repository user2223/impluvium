// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: 0BSD

//! Pure single-pairing model of proofs.md §3.4, below request checks.
//! State and proof observations are injected for tests. No persistence or clock.

use super::session::{Time, expires, nanos};
use crate::decode::{Diagnostic, Reason, error};

#[derive(Clone, Debug)]
pub struct State {
    pub exists: bool,
    pub completed: bool,
    pub approved: bool,
    pub denied: bool,
    pub expires_at: Time,
    pub interval_seconds: u64,
    pub last_accepted_poll: Option<Time>,
    pub device_registered: bool,
}
impl State {
    pub fn start(now: Time) -> Result<Self, Diagnostic> {
        Ok(Self {
            exists: true,
            completed: false,
            approved: false,
            denied: false,
            expires_at: expires(now, 600)?,
            interval_seconds: 5,
            last_accepted_poll: None,
            device_registered: false,
        })
    }
    fn open(&self) -> Result<(), Diagnostic> {
        if !self.exists || self.completed {
            Err(failure(Reason::pairing_unknown))
        } else {
            Ok(())
        }
    }
    fn live(&self, now: Time) -> Result<(), Diagnostic> {
        if nanos(now) >= nanos(self.expires_at) {
            return Err(failure(Reason::pairing_expired));
        }
        if self.denied {
            return Err(failure(Reason::pairing_denied));
        }
        Ok(())
    }
    fn approved(&self) -> Result<(), Diagnostic> {
        if self.approved {
            Ok(())
        } else {
            Err(failure(Reason::pairing_pending))
        }
    }
    /// Steps 4–13. `proof` is the injected result of verification and bindings.
    /// Each invocation serializes step 11 against the supplied snapshot. `atomic`
    /// injects step 13's current snapshot/time, independently from step 11.
    /// Every accepted poll retains its timestamp, even if completion loses a race.
    /// A SLOW_DOWN diagnostic's retry interval is in the returned snapshot.
    pub fn poll(
        &self,
        proof: Result<(), Diagnostic>,
        now: Time,
        atomic: impl FnOnce(Self) -> (Self, Time),
    ) -> (Self, Result<(), Diagnostic>) {
        let early = self.open().and(proof).and_then(|()| self.live(now));
        if let Err(d) = early {
            return (self.clone(), Err(d));
        }
        let mut next = self.clone();
        let too_early = next.last_accepted_poll.is_some_and(|last| {
            nanos(now) < nanos(last) + i128::from(next.interval_seconds) * 1_000_000_000
        });
        next.last_accepted_poll = Some(now);
        if too_early {
            // The finite model saturates beyond representable intervals. Such
            // intervals already exceed the entire lifetime of a live pairing.
            next.interval_seconds = next.interval_seconds.saturating_add(5);
            return (next, Err(failure(Reason::slow_down)));
        }
        if let Err(d) = next.approved() {
            return (next, Err(d));
        }
        let (mut next, time) = atomic(next);
        let result = next
            .open()
            .and_then(|()| next.live(time))
            .and_then(|()| next.approved())
            .map(|()| {
                next.completed = true;
                next.device_registered = true;
            });
        (next, result)
    }
}
fn failure(reason: Reason) -> Diagnostic {
    error(reason, "PollPairingRequest.pairing_id")
}
