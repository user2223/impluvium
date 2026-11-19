// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! What the readiness probe reports.
//!
//! The server is ready when two things hold: the startup finished and the server has not begun to
//! stop ([`Readiness::open`] and [`Readiness::close`]), and this process still owns the database
//! (the state of the owner guard is `Held`). Losing the database lock turns the probe to `503` at
//! once, without waiting for the shutdown to begin.

use std::sync::atomic::{AtomicBool, Ordering};

use server_db::start_guard::Ownership;
use server_domain::credential::BoxFuture;
use server_http::health::ReadinessPort;
use tokio::sync::watch;

/// The readiness state shared by the probe and the lifecycle code.
#[derive(Debug)]
pub(crate) struct Readiness {
    serving: AtomicBool,
    ownership: watch::Receiver<Ownership>,
}

impl Readiness {
    /// Not ready yet; ready after [`Readiness::open`] while `ownership` stays `Held`.
    pub(crate) fn new(ownership: watch::Receiver<Ownership>) -> Self {
        Self {
            serving: AtomicBool::new(false),
            ownership,
        }
    }

    /// The startup is complete and the listener is bound.
    pub(crate) fn open(&self) {
        self.serving.store(true, Ordering::SeqCst);
    }

    /// The server is stopping; the probe answers `503` from now on.
    pub(crate) fn close(&self) {
        self.serving.store(false, Ordering::SeqCst);
    }

    pub(crate) fn is_ready(&self) -> bool {
        self.serving.load(Ordering::SeqCst) && *self.ownership.borrow() == Ownership::Held
    }
}

impl ReadinessPort for Readiness {
    fn ready(&self) -> BoxFuture<'_, bool> {
        Box::pin(async move { self.is_ready() })
    }
}

#[cfg(test)]
mod tests {
    use server_db::start_guard::LossReason;

    use super::*;

    #[test]
    fn not_ready_then_ready_then_lost_gives_503() {
        let (state, receiver) = watch::channel(Ownership::Held);
        let readiness = Readiness::new(receiver);
        assert!(!readiness.is_ready(), "ownership alone is not enough");
        readiness.open();
        assert!(readiness.is_ready());
        state.send_replace(Ownership::Lost(LossReason::ConnectionError));
        assert!(!readiness.is_ready());
    }

    #[test]
    fn closing_and_releasing_also_turn_the_probe_off() {
        let (state, receiver) = watch::channel(Ownership::Held);
        let readiness = Readiness::new(receiver);
        readiness.open();
        readiness.close();
        assert!(!readiness.is_ready());
        readiness.open();
        assert!(readiness.is_ready());
        state.send_replace(Ownership::Released);
        assert!(!readiness.is_ready());
    }

    #[test]
    fn the_probe_port_reports_the_same_state() {
        let (state, receiver) = watch::channel(Ownership::Held);
        let readiness = Readiness::new(receiver);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        assert!(!runtime.block_on(readiness.ready()));
        readiness.open();
        assert!(runtime.block_on(readiness.ready()));
        state.send_replace(Ownership::Lost(LossReason::Timeout));
        assert!(!runtime.block_on(readiness.ready()));
    }
}
