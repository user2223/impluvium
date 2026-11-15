<!--
SPDX-FileCopyrightText: 2026 the impluvium contributors
SPDX-License-Identifier: 0BSD
-->

# Protocol design decisions and rejected alternatives

Each record states the decision, the earlier draft statement it replaces where there was one, the reason, and the
alternatives that were rejected. "Earlier draft" refers to the design documents written before the specification; they
are not normative.

## 1. Schema and code generation

1. **One schema source, committed generated code.** The `.proto` files under `engine/protocol/proto/` are the only
   schema. Rust and TypeScript code is generated from them with `buf` and locally installed generator plugins and is
   committed; a drift check fails when committed output and schema disagree. A normal build needs neither `protoc` nor
   code generation nor network access to a schema registry.
   - *Replaces:* an earlier draft generated Rust types with a different runtime from a descriptor set in a custom build
     step.
   - *Why:* the chosen Rust runtime has native ProtoJSON support and a matching Connect implementation, which the
     server needs; committed output keeps wire changes reviewable as diffs.
   - *Rejected:* OpenAPI, JSON Schema or TypeSpec as the contract (weaker generators, a second toolchain, binary
     manifests would become base64); build-time generation inside every consumer build (hides wire changes from review).
2. **Location under `engine/`.** Schema, specification, vectors, reference tooling and the protocol crates live under
   `engine/`, which is 0BSD except for third-party material that `engine/protocol/NOTICE` lists with its license
   (currently the FastCDC constants, MIT).
   - *Replaces:* a separate top-level permissively licensed `protocol/` directory.
   - *Why:* the licence boundary stays one rule: nothing below `engine/` is copyleft or taken from the rest of the
     repository, and every third-party part is listed with its notice.
3. **One implementation of the security core.** The server and all clients are written in Rust and share one
   implementation of envelope, keys, tickets, proofs, descriptors and validators. The TypeScript package contains
   generated types and test helpers only; it signs and verifies nothing in production.
   - *Replaces:* an earlier draft with a TypeScript server that carried its own signer and verifier next to the Rust
     library.
   - *Why:* two production implementations of the same security rules double the attack surface. Correlated mistakes
     are caught instead by an independent reference verifier that is written from this specification alone, is never
     shipped, and must agree on every vector.
4. **Rust speaks both codecs.** Rust clients and the Rust server use the Connect protocol with `application/proto` and
   `application/json`.
   - *Replaces:* "Rust clients speak only `application/proto` and use no Connect implementation".
   - *Why:* the server is written in Rust and must serve both codecs; the shared client code uses the same stack.

## 2. Encoding

1. **Strict, unambiguous acceptance profile** ([encoding.md](../encoding.md)). Duplicate singular fields, split
   sub-messages, several `oneof` members, duplicate map keys, duplicate JSON keys and both JSON names of one field are
   rejected; a known field with the wrong wire type is an error, never an unknown field.
   - *Why:* general-purpose decoders resolve these cases differently (last one wins, merge, overwrite). Different
     answers in different components are a classic source of signature and validation bypasses.
   - *Rejected:* relying on the default behaviour of the chosen runtime.
2. **Unknown fields are skipped without storage.** Every message accepts unknown fields; they are validated
   syntactically, skipped and never stored.
   - *Replaces:* a proposal to accept at most 16 unknown fields in total across all sub-structures of a manifest.
   - *Why:* a later optional field in a manifest entry would appear in thousands of entries and break older clients at
     the 17th occurrence. Skipping without storage protects memory just as well and keeps forward compatibility.
3. **Small type set** ([wire-types.md](../wire-types.md)). Only `uint32`, `uint64`, `bool`, enums, `string`, `bytes`,
   messages, `map<string, string>`, `oneof` and `google.protobuf.Timestamp`.
   - *Why:* every additional type needs its own cross-codec vectors.
   - *Consequence:* `ErrorInfo.retry_after_seconds` is a `uint32` instead of a `Duration`.
4. **Timestamps are range-checked on input in both codecs.**
   - *Why:* a binary timestamp outside the JSON-representable range would otherwise be stored and fail only later, when
     it is written as JSON.
5. **Standard ProtoJSON input forms, read one way.** Integers may be numbers or strings, including exponent forms, as
   long as they denote an exact integer in range; `bytes` may use either base64 alphabet with or without padding;
   timestamps follow RFC 3339. The profile fixes how these forms are evaluated (exactly, never through floating point),
   and restricts JSON only where an input could be read in two ways (duplicate keys, two names of one field, several
   `oneof` members, wrong value types).
   - *Why:* clients written against standard ProtoJSON must keep working; the ambiguity rules are what security needs.
6. **Standard ProtoJSON for RPC messages.** `bytes` appear as base64 and 64-bit integers as decimal strings in JSON;
   hexadecimal is used only in URL paths.
   - *Replaces:* "hashes and identifiers are lowercase hexadecimal in JSON".
   - *Why:* a second JSON dialect next to ProtoJSON would need its own codecs and vectors in every language.

## 3. Signatures and keys

1. **One envelope with the DSSE pre-authentication encoding and Ed25519** for every signed document.
   - *Rejected:* canonical JSON, home-grown domain prefixes, JWS/COSE (algorithm agility), detached minisign files,
     signing a hash with a separate binding.
2. **Explicit Ed25519 acceptance profile** ([envelope.md §4.2](../envelope.md#42-acceptance-profile)): canonical
   point encodings, no small-order points, `S < L`, cofactorless equation.
   - *Why:* Ed25519 libraries differ in what they accept; equal library names prove nothing.
3. **Multiple signatures, trusted key sets supplied by the caller.** A known key with an invalid signature rejects the
   whole envelope; unknown key ids are ignored; the envelope never extends the trusted set.
   - *Why:* allows a later second signer and key overlap without a format change, without letting an attacker choose
     the verifying key.
   - *Rejected:* a signed revocation list or key-set statement in this revision; such payload types are unknown and
     rejected.
4. **All server signing keys are derived from one instance key with HKDF-SHA-256** ([keys.md §2](../keys.md#2-derivation-of-server-keys)).
   - *Replaces:* two separately generated key files for content and depot tickets.
   - *Why:* one secret to back up; depot-ticket keys can switch generation by changing the derivation label.
5. **The content key does not rotate in this revision.** Devices pin it at pairing.
6. **Depot-ticket key generations overlap** until every ticket of the previous generation has expired.
   - *Why:* a switch must not invalidate tickets that are still in use, and an old key must not issue new tickets.

## 4. Documents

1. **Manifest and release descriptor are separate signed documents.**
   - *Rejected:* launch and setup data inside the manifest (every typo fix would change the content identity); unsigned
     launch data (a proxy could inject commands).
2. **Per-file chunks.** A chunk never spans files; small files are one chunk.
   - *Rejected:* packs that combine several files (one inserted file shifts all later packs; validators get complex).
3. **No platform field in the manifest.** Platforms come from the descriptor.
4. **Content hash SHA-256; BLAKE3 stays readable through its algorithm id.**
   - *Replaces:* a provisional default with a planned switch to BLAKE3 if benchmarks showed a large speed-up on CPUs
     without SHA extensions.
   - *Why:* the benchmarks did not justify a switch; SHA-256 is universally available. The benchmark records are
     published with the benchmark material.
5. **Chunking parameters: FastCDC 2020, minimum 64 KiB, average 256 KiB, maximum 1 MiB; files up to 1 MiB are one
   chunk.**
   - *Replaces:* provisional parameters of 256 KiB / 1 MiB / 4 MiB and a fallback to fixed-size blocks.
   - *Why:* chosen by the benchmark selection rule; consumers never recompute boundaries, so the policy can change later
     without a format change.
   - *Constants:* the gear table and masks are those of the implementation the benchmarks used (the Rust crate
     fastcdc 5.0.0, module v2020), written out in the specification so that an independent implementation reproduces
     the same cut points; the gear table can also be regenerated from its MD5 rule. Provenance and license notice are in
     `engine/protocol/NOTICE`.
6. **Empty files have no chunk.** The hash of the empty string identifies their content.
   - *Rejected:* one zero-length chunk (needs chunk size 0 in the schema and a depot rule for empty ranges).
7. **Single-chunk files keep both the file hash and the identical chunk id.** No special encoding removes the
   redundancy; the cost is accepted for a simpler model.
8. **No encryption and no compression in the format.** Transport confidentiality is TLS's job; compression can come
   later behind a required feature.
9. **The descriptor also has `required_features`.**
   - *Why:* commands are executed; a later descriptor feature that changes execution must not be ignored by older
     clients.
10. **Commands are structured and literal.** Executable, arguments, working directory and environment; no shell, no
    template expansion.
11. **The Windows path profile is complete and defined independently of the platform** ([paths.md §4](../paths.md#4-windows-profile)):
    all characters that Windows file names cannot contain, all reserved device names including the superscript-digit
    forms, and a case-collision rule over fixed Unicode 16.0.0 data that includes ancestor directories.
    - *Replaces:* an earlier list naming case-fold collisions, reserved names and trailing dots or spaces only.
    - *Why:* the profile must give the same answer on every build platform, and a tree it accepts must be installable on
      Windows.

## 5. Identity, pairing and sessions

1. **Challenge-response sessions without a device clock.**
   - *Rejected:* signed timestamp-and-nonce requests (machines that switch between operating systems often have clocks
     that are off by hours); a ±300-second tolerance window; refresh tokens (races between local processes).
2. **Pairing in the shape of the device authorization grant**, approved only from a fully authenticated session with
   completed multi-factor authentication.
   - *Rejected:* invalidating a pairing after a number of wrong codes (a wrong code cannot be attributed to a pairing);
     a local redirect listener; a pairing token or device timestamp in the pairing proof.
3. **No deep link and no web-scope tokens in this revision**; both are reserved ([api.md §10](../api.md#10-reserved-flows)).
4. **The pairing result carries no capability list.**
   - *Replaces:* an earlier draft that returned accepted capabilities from pairing.
   - *Why:* capabilities are negotiated per session and never stored per device.
5. **Revocation is server state.** Devices are listed and revoked through paginated RPCs.
   - *Rejected:* a signed revocation list distributed to clients.

## 6. API and errors

1. **Discovery uses a revision scheme** (`major`, `revision`, `min_client_revision`, plus the client's compiled-in
   `min_server_revision`), with the server's source revision and source link added.
   - *Rejected:* a minimum/maximum protocol range; a `/.well-known/` path (breaks deployments under a path prefix);
     per-request version headers.
2. **Pinning binds instance identity and content key to the base URL; discovery fingerprints are informational.** The
   pin checks belong to one transport connection; every new connection repeats them before it carries a credential.
   - *Why:* neither a TLS connection nor a fingerprint from an unauthenticated document may replace the pin, and a
     pool that opens a new connection must not inherit a check made on another one.
3. **One error body format everywhere.** RPC errors, discovery, document routes, the event stream at open time and the
   depot all use the Connect error JSON with an optional `ErrorInfo` detail.
   - *Replaces:* a separate JSON rendering of `ErrorInfo` for non-RPC routes.
   - *Why:* one pre-check schema and one parser.
4. **Every list method is paginated**, including the versions of one game.
5. **No content coding on document routes in this revision.** Clients do not decompress.
   - *Replaces:* manifest responses compressed according to `Accept-Encoding`.
   - *Why:* no decompression before the size limits apply; compression can return as a negotiated capability.
6. **Broken error bodies never delete credentials.** Only a validated `DEVICE_REVOKED` or `DEVICE_EXPIRED` from the
   pinned server does.

## 7. Events

1. **Server-Sent Events with epoch and sequence numbers.**
   - *Replaces:* a WebSocket channel with in-band close codes and resumption by sequence number alone.
   - *Why:* SSE passes standard reverse proxies; a random epoch per server start detects restarts, so a client never
     replays the history of another process; terminal states are typed events.
2. **Resumption from the applied state, a per-stream chain (`prev_seq`) with heartbeats for gap detection, versioned
   library merge.** Events a stream does not receive never enter its chain, so filtered topics and withdrawn access
   cause no false gaps.
   - *Replaces:* restarting a library listing whenever a snapshot revision changed, which does not converge under
     continuous changes.
3. **Only the `library` topic has a payload in this revision.** Other topics are reserved.
4. **Supersession is keyed by device for bearer streams and by session for cookie streams.** All processes of one
   device that use the same client identifier share one stream.
5. **Every library change is a sequenced entry event.** Changes caused by access rules produce deletion markers like any
   other removal; a deletion marker carries only the entry identity and needs no permission on the removed content.
   - *Rejected:* an unsequenced "access changed" signal, which a disconnected client would miss and then resume past.
6. **Events are classified by their SSE event name.** ProtoJSON has no container for a `oneof`, so the event name tells
   the client which payload to expect and whether an unknown kind is a data or a control event.

## 8. Transport

1. **No proxy support; direct connections only.** Proxy environment variables and system settings are ignored.
2. **Depot origins need two approvals**: the authenticated download response and the client configuration.
   - *Why:* a server response alone must not make a client send a ticket to a new host.
3. **API calls follow no redirects; chunk requests follow at most five redirects within one origin.**
4. **No internal server-to-depot RPC is part of the protocol.** The depot is part of the server by default and verifies
   tickets with public keys only.
   - *Replaces:* an internal job API between server and a separate depot process.
