<!--
SPDX-FileCopyrightText: 2026 the impluvium contributors
SPDX-License-Identifier: 0BSD
-->

# Shared HTTP client

`trust::Tls::new(Platform::current(), extra_roots)` initializes aws-lc-rs before
constructing a client. Share this TLS configuration with every HTTP stack.
`initialize_aws_lc` is idempotent. `initialize_provider` accepts repeated calls
with the same Arc and rejects replacement or an externally installed default.
Linux uses rustls-platform-verifier, including additional user roots. The native
Windows integration is reserved and returns `platform_trust_unavailable` before
provider initialization or certificate-store access, including for HTTP URLs.

Construct `http::Http` with the TLS configuration, a protocol user agent and a
finite deadline. `Http::rpc(settings, codec)` returns the transport to pass
directly to any generated `connect::impluvium::v2::*ServiceClient::new`, together
with `transport.client_config()`. The base URL in `Settings` is the user-entered
server base; the generated config adds `/api/v2`, preserving its path prefix.
Reconstruct the transport when changing settings or stopping a trust context.

The generated client receives a body only after request preflight, destination
approval, head checks, a bounded streaming read and schema prechecks. Error
wrappers and binary ErrorInfo details pass the protocol guard before Connect's
error JSON parser or detail decoder can see them. The three request-bound
library response identities are checked against the actual request. The generic
precheck does not authenticate a signed descriptor or establish a pairing pin;
those operations require the existing chapter-specific APIs and trust context.
No credential store is read by this module. Authorization, Proxy-Authorization
and Cookie headers and the proof-bearing PollPairing and CompleteSession methods
are rejected before body polling or any network I/O with
`Error::CredentialsRequireVerifiedConnection`.
`TrustState::Trusted` does not authorize transfer on a physical connection.
Connection-bound authorization, including replacement connections, is reserved
for a subsequent integration. `Http::get` is a credential-free, finite-body
ingress; SSE reception needs its separate per-event receiver.

Both codecs keep the request's schema and limits regardless of response headers.
The response coding must be identity. The direct Hyper adapter installs no
decompression, redirect, retry, referer or proxy service. Chunk GET redirects
are decided by `transport::redirect`, including fresh preflight on each target.
Timeouts and interrupted transfers retain their transport error source. Use
`rpc::diagnostic(&error)` on a generated call's error to recover a local diagnostic;
it is not a server wire reason.

## Response-head boundary

The network adapter uses Hyper 1.12.0's HTTP/1 connection builder with
`max_header_size(65536)` and a bounded read buffer. The parser counts the complete
raw head, including the status line and whitespace, separately for every 1xx and
final response. `is_parse_too_large()` maps to `headers_too_large` with field `""`;
the pinned Hyper version requires its `server` feature for this public predicate.
No server is constructed. Parsed HTTP/1 fields are never used to reconstruct the
raw size. HTTP/2 uses the same connector and TLS ALPN; its exclusive h2 limit is
65537, with the local inclusive 65536 header-list check retained. Cleartext h2c
can be explicitly selected with `Http::http2_prior_knowledge()`.

Both paths use the same OS trust, finite deadline and response guard. The public
`ReqwestTransport` type name is retained for compatibility; reqwest's parsed-head
API cannot enforce the required HTTP/1 boundary. Unknown checked error codes are
normalized to `unknown` before Connect can substitute a code from the HTTP status;
message and details are retained.

## Verification

The Rust tests use actual generated clients over an in-process HTTP transport,
plus loopback HTTP/1 and HTTP/2 servers. A probe at the generated client's
response-body input counts entry before JSON conversion, binary view decoding or
error parsing.
Malformed profiles, element floods inside byte limits, singular duplicates,
message splits, JSON key collisions and aliases, bad timestamps and hostile error
details must leave that count unchanged. Counterexamples exercise a disabled guard
and a guard moved after the generated decode. Oneof profile tests use ServerEvent;
this revision's RPC response schemas do not contain a oneof.

Run `cargo test -p impluvium-client -p impluvium-protocol` and
`cargo clippy -p impluvium-client -p impluvium-protocol --all-targets -- -D warnings`
in the build container. These commands have not been run in this working copy.
Network regressions cover credentials rejected before connecting, raw reason
phrases, whitespace, 103 heads, exact HTTP/1 and HTTP/2 boundaries, and unknown
error codes through a generated client. No test-count ratchet or lockfile is
edited here. This revision adds direct edges to already locked `hyper`,
`tower-service` and `serde_json` (with `raw_value`), and moves `tokio` from dev to
normal dependencies. Let Cargo regenerate the lockfile's dependency edges in the
container.
