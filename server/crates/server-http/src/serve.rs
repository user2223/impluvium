// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Binding the listener and serving the application router.
//!
//! [`serve`] hands every request the address of its peer, which the policy gate needs, and stops
//! accepting connections when the `shutdown` future completes. It then waits for the requests that
//! are still running; the caller bounds that wait.

use std::future::Future;
use std::io;
use std::net::SocketAddr;

use axum::extract::{ConnectInfo, Request};
use axum::middleware::{self, Next};
use axum::response::Response;

use crate::auth::PeerAddress;
use crate::router::ApplicationRouter;

/// A bound TCP listener that is not serving yet.
#[derive(Debug)]
pub struct BoundListener(tokio::net::TcpListener);

impl BoundListener {
    /// Binds the address. Nothing is accepted before [`serve`] runs.
    pub async fn bind(address: SocketAddr) -> io::Result<Self> {
        tokio::net::TcpListener::bind(address).await.map(Self)
    }

    /// The address the listener is bound to; useful when port 0 was requested.
    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.0.local_addr()
    }
}

/// Puts the peer address into the request extensions, where the policy gate reads it.
async fn add_peer(
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    mut request: Request,
    next: Next,
) -> Response {
    request.extensions_mut().insert(PeerAddress(peer));
    next.run(request).await
}

/// Serves the router on the listener until `shutdown` completes, then lets in-flight requests
/// finish. Returns when the last connection is done.
pub async fn serve(
    listener: BoundListener,
    app: ApplicationRouter,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> io::Result<()> {
    let router = app.into_router().layer(middleware::from_fn(add_peer));
    axum::serve(
        listener.0,
        router.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown)
    .await
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpStream;
    use std::sync::Arc;

    use server_domain::credential::NoCredentials;
    use tokio::sync::oneshot;

    use super::*;
    use crate::auth::{ExternalOrigin, Scheme, SecurityContext};
    use crate::proxy::TrustedProxies;
    use crate::registry;
    use crate::router::build;

    fn app() -> ApplicationRouter {
        let context = SecurityContext::new(
            ExternalOrigin::new(Scheme::Https, "app.example.test", None).unwrap(),
            TrustedProxies::default(),
            Arc::new(NoCredentials),
        );
        build(registry::application(), context)
    }

    fn runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
    }

    #[test]
    fn requests_reach_the_gate_with_their_peer_address_and_shutdown_ends_the_server() {
        let runtime = runtime();
        runtime.block_on(async {
            let listener = BoundListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
                .await
                .unwrap();
            let address = listener.local_addr().unwrap();
            let (stop, stopped) = oneshot::channel::<()>();
            let client = std::thread::spawn(move || {
                let mut stream = TcpStream::connect(address).unwrap();
                stream
                    .write_all(
                        b"GET /api/health/live HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
                    )
                    .unwrap();
                let mut answer = String::new();
                stream.read_to_string(&mut answer).unwrap();
                let _ = stop.send(());
                answer
            });
            let result = serve(listener, app(), async {
                let _ = stopped.await;
            })
            .await;
            assert!(result.is_ok());
            let answer = client.join().unwrap();
            assert!(answer.starts_with("HTTP/1.1 200"), "{answer}");
        });
    }
}
