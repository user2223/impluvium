// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! The real SIGTERM handler: the process signals itself and must observe it.

#![cfg(unix)]

#[path = "../src/shutdown.rs"]
mod shutdown;

use std::time::Duration;

use shutdown::{Cause, ShutdownSignal};

#[test]
fn sigterm_to_the_own_process_is_observed_within_the_bound() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let cause = runtime.block_on(async {
        // Register before sending: an unhandled SIGTERM would end the test process.
        let mut signals = ShutdownSignal::install().unwrap();
        // SAFETY: kill has no memory-safety preconditions; the target is this process.
        let sent = unsafe { libc::kill(libc::getpid(), libc::SIGTERM) };
        assert_eq!(sent, 0);
        tokio::time::timeout(Duration::from_secs(5), signals.recv())
            .await
            .expect("the signal is observed within the bound")
    });
    assert_eq!(cause, Cause::Terminate);
}
