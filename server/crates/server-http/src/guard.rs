// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Turns a panic inside request handling into an internal error.
//!
//! The panic payload never reaches the response. It is leaked on purpose
//! instead of dropped, because dropping a payload runs its destructor, which
//! can panic again. The process panic hook, owned by the executable, decides
//! what is logged.

use std::future::Future;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::pin::Pin;
use std::task::{Context, Poll};

use axum::extract::Request;
use axum::middleware::Next;
use axum::response::Response;
use server_api_types::ErrorCode;

use crate::error::error_response;

type Inner = Pin<Box<dyn Future<Output = Response> + Send>>;

/// Polls the inner future and reports a panic as `Err`.
struct CatchUnwind {
    inner: Inner,
}

impl Future for CatchUnwind {
    type Output = Result<Response, ()>;

    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        match catch_unwind(AssertUnwindSafe(|| this.inner.as_mut().poll(context))) {
            Ok(Poll::Ready(response)) => Poll::Ready(Ok(response)),
            Ok(Poll::Pending) => Poll::Pending,
            Err(payload) => {
                // Dropping a payload runs its destructor, which can panic
                // again; leaking the small box is the safe choice.
                std::mem::forget(payload);
                Poll::Ready(Err(()))
            }
        }
    }
}

/// Layer function: runs the rest of the stack and answers a panic with the
/// internal error.
pub(crate) async fn catch_panics(request: Request, next: Next) -> Response {
    let inner: Inner = Box::pin(next.run(request));
    match (CatchUnwind { inner }).await {
        Ok(response) => response,
        Err(()) => error_response(ErrorCode::InternalError),
    }
}
