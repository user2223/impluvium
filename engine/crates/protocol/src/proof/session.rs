// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: 0BSD

//! Pure, injected-state session model of proofs.md §2 and §4.1.
//! Callers serialize transitions and replace their snapshot with the returned one.
//! Proof observations are test inputs, not authentication evidence or an RPC API.

pub use crate::keys::SwitchTime as Time;
use crate::{
    decode::{Diagnostic, Reason, error},
    manifest::uuid,
};
use subtle::ConstantTimeEq;

pub(super) fn nanos(t: Time) -> i128 {
    i128::from(t.seconds) * 1_000_000_000 + i128::from(t.nanos)
}
pub(super) fn expires(now: Time, seconds: i64) -> Result<Time, Diagnostic> {
    let end = now
        .seconds
        .checked_add(seconds)
        .ok_or(error(Reason::invalid_timestamp, ""))?;
    if !(-62_135_596_800..=253_402_300_799).contains(&now.seconds)
        || !(-62_135_596_800..=253_402_300_799).contains(&end)
        || now.nanos >= 1_000_000_000
    {
        return Err(error(Reason::invalid_timestamp, ""));
    }
    Ok(Time {
        seconds: end,
        nanos: now.nanos,
    })
}

#[derive(Clone, Debug)]
pub struct Device {
    pub device_id: String,
    pub registered: bool,
    pub revoked: bool,
    /// Pairing completion time if no session has ever completed.
    pub activity_at: Time,
    pub idle_seconds: u64,
}

/// Secret challenge octets are never included in Debug output.
#[derive(Clone)]
pub struct Challenge {
    pub value: [u8; 32],
    pub device_id: String,
    pub expires_at: Time,
    pub consumed: bool,
}
#[derive(Clone)]
pub struct State {
    pub device: Device,
    pub challenges: Vec<Challenge>,
    pub capacity: usize,
}
#[derive(Clone, Copy, Debug)]
pub enum Restart {
    Preserve,
    Forget,
}
impl State {
    /// BeginSession after request-profile checks. Fresh CSPRNG randomness is
    /// supplied, never generated here. Revoked and idle-expired registered
    /// devices get challenges.
    pub fn begin(&self, now: Time, random: [u8; 32]) -> (Self, Result<(), Diagnostic>) {
        let mut next = self.clone();
        let result = (|| {
            if !uuid(&self.device.device_id) {
                return Err(error(Reason::invalid_uuid, "BeginSessionRequest.device_id"));
            }
            if !self.device.registered {
                return Err(error(
                    Reason::unknown_device,
                    "BeginSessionRequest.device_id",
                ));
            }
            let expires_at = expires(now, 60)?;
            let occupied = self
                .challenges
                .iter()
                .filter(|c| nanos(now) < nanos(c.expires_at))
                .count();
            if occupied >= self.capacity {
                return Err(error(Reason::rate_limited, ""));
            }
            // Only expired entries may be removed, including consumed entries.
            next.challenges.retain(|c| nanos(now) < nanos(c.expires_at));
            next.challenges.push(Challenge {
                value: random,
                device_id: self.device.device_id.clone(),
                expires_at,
                consumed: false,
            });
            Ok(())
        })();
        (next, result)
    }

    fn check(&self, challenge: &[u8; 32], now: Time) -> Result<usize, Diagnostic> {
        let device_field = "CompleteSessionRequest.device_id";
        let challenge_field = "CompleteSessionRequest.challenge";
        if self.device.revoked {
            return Err(error(Reason::device_revoked, device_field));
        }
        if nanos(now)
            >= nanos(self.device.activity_at) + i128::from(self.device.idle_seconds) * 1_000_000_000
        {
            return Err(error(Reason::device_expired, device_field));
        }
        let index = self
            .challenges
            .iter()
            .position(|c| {
                bool::from(c.value.ct_eq(challenge)) && c.device_id == self.device.device_id
            })
            .ok_or(error(Reason::challenge_unknown, challenge_field))?;
        let c = &self.challenges[index];
        if nanos(now) >= nanos(c.expires_at) {
            return Err(error(Reason::challenge_expired, challenge_field));
        }
        if c.consumed {
            return Err(error(Reason::challenge_consumed, challenge_field));
        }
        Ok(index)
    }

    /// Check proof before device/challenge state; recheck 4a–4e at the atomic
    /// boundary. The closure injects the current stored snapshot and server time.
    /// Its changes survive a losing recheck (FORMAT.md's model convention).
    pub fn complete(
        &self,
        challenge: &[u8; 32],
        proof: Result<(), Diagnostic>,
        now: Time,
        atomic: impl FnOnce(Self) -> (Self, Time),
    ) -> (Self, Result<(), Diagnostic>) {
        let early = if self.device.registered {
            proof
        } else {
            Err(error(
                Reason::unknown_device,
                "CompleteSessionRequest.device_id",
            ))
        }
        .and_then(|()| self.check(challenge, now).map(|_| ()));
        if let Err(d) = early {
            return (self.clone(), Err(d));
        }
        let (mut next, time) = atomic(self.clone());
        let result = next.check(challenge, time).map(|index| {
            next.challenges[index].consumed = true;
            next.device.activity_at = time;
        });
        (next, result)
    }

    /// Both allowed restart policies preserve the no-reopening invariant.
    pub fn restart(&self, policy: Restart) -> Self {
        let mut next = self.clone();
        if matches!(policy, Restart::Forget) {
            next.challenges.clear();
        }
        next
    }
}
