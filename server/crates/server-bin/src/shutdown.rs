// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Bounded graceful shutdown.
//!
//! The process stops on SIGTERM or SIGINT (on Windows: Ctrl-C or
//! Ctrl-Break). [`serve_until`] then tells the server to stop accepting and
//! to let in-flight requests finish, and gives it a fixed time. When the
//! time is up the server is abandoned and the caller exits.
//!
//! The module depends on tokio only, so it can be included on its own in
//! tests.

#![allow(dead_code)]

use std::future::{Future, poll_fn};
use std::io;
use std::pin::{Pin, pin};
use std::task::{Context, Poll};
use std::time::Duration;

use tokio::sync::oneshot;

/// How long in-flight requests may take once shutdown has begun.
pub(crate) const DEADLINE: Duration = Duration::from_secs(30);

/// What ended the run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Outcome {
    /// The server finished, on its own or within the deadline.
    Completed,
    /// The server was still busy when the deadline expired and was abandoned.
    DeadlineExceeded,
}

/// The signal that asked the process to stop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Cause {
    /// SIGTERM, or Ctrl-Break on Windows.
    Terminate,
    /// SIGINT, or Ctrl-C on Windows.
    Interrupt,
}

/// A future that completes when graceful shutdown begins. The server hands
/// it to its connection handling as the signal to stop accepting.
pub(crate) struct GracefulStop(oneshot::Receiver<()>);

impl Future for GracefulStop {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<()> {
        Pin::new(&mut self.0).poll(context).map(|_| ())
    }
}

/// Runs a server until it ends or `trigger` completes, then allows it
/// `deadline` to finish.
///
/// `serve` receives the [`GracefulStop`] and returns the server future. When
/// the trigger fires, the stop completes; the server is expected to stop
/// accepting and to finish what is in flight. A server still running when
/// the deadline expires is dropped.
pub(crate) async fn serve_until<F, S, T>(serve: F, trigger: T, deadline: Duration) -> Outcome
where
    F: FnOnce(GracefulStop) -> S,
    S: Future<Output = ()>,
    T: Future<Output = ()>,
{
    let (stop, stopped) = oneshot::channel();
    let mut server = pin!(serve(GracefulStop(stopped)));
    let mut trigger = pin!(trigger);
    let ended_alone = poll_fn(|context| {
        if server.as_mut().poll(context).is_ready() {
            return Poll::Ready(true);
        }
        if trigger.as_mut().poll(context).is_ready() {
            return Poll::Ready(false);
        }
        Poll::Pending
    })
    .await;
    if ended_alone {
        return Outcome::Completed;
    }
    // A closed channel also counts as the stop, so the result is not needed.
    let _ = stop.send(());
    match tokio::time::timeout(deadline, server.as_mut()).await {
        Ok(()) => Outcome::Completed,
        Err(_) => Outcome::DeadlineExceeded,
    }
}

/// The registered process signal handlers.
#[cfg(unix)]
pub(crate) struct ShutdownSignal {
    terminate: tokio::signal::unix::Signal,
    interrupt: tokio::signal::unix::Signal,
}

#[cfg(unix)]
impl ShutdownSignal {
    /// Registers the handlers. Call it inside the runtime, before the server
    /// starts, so that no signal arrives unhandled.
    pub(crate) fn install() -> io::Result<Self> {
        use tokio::signal::unix::{SignalKind, signal};
        Ok(Self {
            terminate: signal(SignalKind::terminate())?,
            interrupt: signal(SignalKind::interrupt())?,
        })
    }

    /// Completes when the first of the signals arrives.
    pub(crate) async fn recv(&mut self) -> Cause {
        poll_fn(|context| {
            if self.terminate.poll_recv(context).is_ready() {
                return Poll::Ready(Cause::Terminate);
            }
            if self.interrupt.poll_recv(context).is_ready() {
                return Poll::Ready(Cause::Interrupt);
            }
            Poll::Pending
        })
        .await
    }
}

/// The registered console control handlers.
#[cfg(windows)]
pub(crate) struct ShutdownSignal {
    ctrl_break: tokio::signal::windows::CtrlBreak,
    ctrl_c: tokio::signal::windows::CtrlC,
}

#[cfg(windows)]
impl ShutdownSignal {
    /// Registers the handlers. Call it inside the runtime, before the server
    /// starts, so that no signal arrives unhandled.
    pub(crate) fn install() -> io::Result<Self> {
        Ok(Self {
            ctrl_break: tokio::signal::windows::ctrl_break()?,
            ctrl_c: tokio::signal::windows::ctrl_c()?,
        })
    }

    /// Completes when the first of the signals arrives.
    pub(crate) async fn recv(&mut self) -> Cause {
        poll_fn(|context| {
            if self.ctrl_break.poll_recv(context).is_ready() {
                return Poll::Ready(Cause::Terminate);
            }
            if self.ctrl_c.poll_recv(context).is_ready() {
                return Poll::Ready(Cause::Interrupt);
            }
            Poll::Pending
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Instant;

    use super::*;

    fn runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .unwrap()
    }

    #[test]
    fn graceful_stop_lets_in_flight_work_finish() {
        let finished = Arc::new(AtomicBool::new(false));
        let work = Arc::clone(&finished);
        let outcome = runtime().block_on(serve_until(
            move |stop| async move {
                stop.await;
                // A request that is still running when shutdown begins.
                tokio::time::sleep(Duration::from_millis(50)).await;
                work.store(true, Ordering::SeqCst);
            },
            async {},
            Duration::from_secs(5),
        ));
        assert_eq!(outcome, Outcome::Completed);
        assert!(finished.load(Ordering::SeqCst));
    }

    #[test]
    fn a_server_that_ends_by_itself_completes_without_the_trigger() {
        let outcome = runtime().block_on(serve_until(
            |_stop| async {},
            std::future::pending::<()>(),
            Duration::from_secs(5),
        ));
        assert_eq!(outcome, Outcome::Completed);
    }

    #[test]
    fn a_hung_request_is_abandoned_at_the_deadline() {
        let started = Instant::now();
        let outcome = runtime().block_on(serve_until(
            |_stop| std::future::pending::<()>(),
            async {},
            Duration::from_millis(100),
        ));
        assert_eq!(outcome, Outcome::DeadlineExceeded);
        let elapsed = started.elapsed();
        assert!(elapsed >= Duration::from_millis(100));
        assert!(elapsed < Duration::from_secs(5), "{elapsed:?}");
    }

    #[test]
    fn the_default_deadline_is_thirty_seconds() {
        assert_eq!(DEADLINE, Duration::from_secs(30));
    }
}
