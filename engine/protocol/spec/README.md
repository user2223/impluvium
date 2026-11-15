<!--
SPDX-FileCopyrightText: 2026 the impluvium contributors
SPDX-License-Identifier: 0BSD
-->

# impluvium protocol specification

Status: **draft for review**, protocol major 2, revision 1. The wire formats in this directory are frozen at the first
release candidate of impluvium 0.1. Until then they may still change, and every change is reviewed against this text.

This specification defines every format that crosses a process boundary between an impluvium server, its depot, the
desktop client, the engine and the command-line client: the discovery document, the device API, the event stream, the
depot chunk API and five signed documents. It is the contract that the shipped Rust implementation and an independent,
non-shipped reference verifier are both written against. Every rule needed to decide whether an input is accepted, and
with which reason it is rejected, is stated here; nothing that this text fixes is left to an implementation.

The Protocol Buffers sources in [`../proto/`](../proto/) are the only schema source. Every field of every message
appears in a field table in one of the chapters below, and every container appears in the limit inventory.

## 1. Conformance language

The key words "MUST", "MUST NOT", "REQUIRED", "SHALL", "SHALL NOT", "SHOULD", "SHOULD NOT", "RECOMMENDED",
"NOT RECOMMENDED", "MAY" and "OPTIONAL" in this specification are to be interpreted as described in BCP 14 (RFC 2119,
RFC 8174) when, and only when, they appear in all capitals.

Sections marked *informative* contain explanations and examples and do not add requirements.

## 2. Document map

| File | Content |
|---|---|
| [encoding.md](encoding.md) | Accepted Protocol Buffers profile, unambiguous binary and JSON representations, identifiers, timestamps, enums, strings |
| [wire-types.md](wire-types.md) | Inventory of every wire type in use and the vector classes each one needs |
| [envelope.md](envelope.md) | Signed envelope, DSSE pre-authentication encoding, Ed25519 acceptance profile, multiple signatures, payload types, `manifest_id` and `key_id` |
| [keys.md](keys.md) | Key roles, derivation of server keys, trusted key sets, depot-ticket key generations and the verification cache |
| [trust.md](trust.md) | Pinning, instance and content-key changes, discovery fingerprints, device credential context |
| [manifest.md](manifest.md) | Manifest format 3: entries, chunks, hashes, chunking, empty files, producer determinism, validation order |
| [paths.md](paths.md) | Path and symbolic-link rules: base profile and Windows profile |
| [descriptor.md](descriptor.md) | Release descriptor version 1, structured commands, context checks |
| [tickets.md](tickets.md) | Depot tickets, their verification and the depot HTTP API |
| [proofs.md](proofs.md) | Session and pairing proofs, challenges, user codes, replay rules |
| [api.md](api.md) | Discovery, version check, capabilities, the four services, pagination and document GET routes |
| [transport.md](transport.md) | HTTP rules for clients: TLS, origins, redirects, credentials, response protection |
| [errors.md](errors.md) | Error model, reason catalog, local validation reasons, mapping to Connect codes, client rules for broken bodies |
| [limits.md](limits.md) | Complete inventory of size, element and nesting limits and the pre-decode rules |
| [events.md](events.md) | Server-Sent Events channel: epochs, sequences, resume, control events, library merge rules |
| [security.md](security.md) | What signatures, tickets and tokens protect, and what they do not |
| [examples.md](examples.md) | Hand-checkable byte layouts |
| [decisions/](decisions/) | Design decisions and the alternatives that were rejected |

## 3. Roles

- **Server**: one impluvium instance. It holds the instance key, signs manifests, descriptors and depot tickets, issues
  challenges and access tokens, and serves the API, the event stream and (by default) the depot.
- **Depot**: the component that serves chunk bytes under `/depot/v2`. It verifies depot tickets offline with depot-ticket
  public keys only. By default it is part of the server process and shares its origin.
- **Device**: a paired client installation of one operating-system user for one instance. The desktop client and the
  command-line client of the same operating-system user share one device identity per instance.
- **Producer**: whoever creates a signed document (the server, and the manifest builder for manifest payloads).
- **Consumer**: whoever accepts a document or message received from another party (clients, engine, depot, server).

A **verifier** is any consumer function that decides acceptance. The rules for verifiers apply equally to the shipped
implementation and to the reference verifier.

## 4. Notation and units

- A *byte* is an octet. "Bytes" of a string are the bytes of its UTF-8 encoding.
- KiB = 1024 bytes, MiB = 1 048 576 bytes, GiB = 1 073 741 824 bytes.
- Hexadecimal byte strings are written in lowercase, two digits per byte, most significant nibble first, for example
  `0a 24`. Numbers written `0x…` are hexadecimal integers; numbers written `0o…` are octal integers.
- `||` denotes byte concatenation. `len(x)` is the byte length of `x`.
- `SP` is the byte `0x20`.
- Timestamps in prose are UTC.
- Field references use `Message.field`, for example `Manifest.entries`.
- Section references use `file.md §n.m`.

## 5. Versioning of the protocol

- Protocol **major** 2 is carried in the API path (`/api/v2`), the package `impluvium.v2` and the depot path
  (`/depot/v2`). A new major is served side by side under a new path.
- Protocol **revision** is an integer. It increases with every release that changes the schema or a normative rule of
  this specification. Revision 1 is the first frozen revision.
- Signed documents are versioned by their **payload type** (`…manifest.v3…`, `…release.v1…` and so on). New consumer
  logic for an existing payload type is introduced through `required_features` (manifest, descriptor) or a new payload
  type, never by changing the meaning of an existing field.
- Removed fields are reserved and their numbers and names are never reused.

## 6. Stable rejection reasons and test vectors

Every rejection defined here has a stable **local reason** name in `snake_case` ([errors.md §5](errors.md#5-local-reason-catalog)).
Local reasons name behaviour; they are used by the local verification functions and by the test vectors, and some of
them map to a wire `ErrorReason`. When an input violates several rules, the **check order** of the relevant chapter
decides which reason is reported. Two conforming verifiers report the same reason for the same input.

Known-answer vectors, the malicious-document corpus and the reference verifier are added beside this directory. Each
vector names its input, the expected bytes or reason, the rule of this specification it exercises, and its provenance.
Expected values are never taken from the output of the shipped implementation. Where an example in this specification
needs a computed value (a hash, a signature, a derived key), it uses a placeholder and refers to the vectors.

## 7. Normative references

- RFC 2119, RFC 8174: requirement levels.
- RFC 3339: date and time format.
- RFC 4648: base64 and base64url.
- RFC 5869: HKDF.
- RFC 8032: Ed25519.
- RFC 8594 and RFC 9745: `Sunset` and `Deprecation` response headers.
- RFC 8628: OAuth 2.0 device authorization grant (shape of the pairing flow).
- RFC 9110: HTTP semantics (status codes, `Range`, `Retry-After`, `ETag`).
- RFC 9562: UUID text format.
- FIPS 180-4: SHA-256 and SHA-512.
- RFC 1321: MD5, used only in the rule that regenerates the FastCDC gear table.
- BLAKE3 specification (version 1 of the BLAKE3 paper and reference implementation): hash mode, 32-byte output.
- DSSE, protocol version 1 (secure-systems-lab/dsse): pre-authentication encoding.
- Protocol Buffers language guide (proto3), binary wire format and ProtoJSON format.
- Connect protocol, unary requests over HTTP.
- WHATWG HTML Living Standard, Server-Sent Events.
- WHATWG URL Standard: URL parsing and origins.
- Unicode 16.0.0: `UnicodeData.txt` and `CaseFolding.txt`.
- FastCDC: Xia et al., "The Design of Fast Content-Defined Chunking for Data Deduplication Based Storage Systems",
  IEEE Transactions on Parallel and Distributed Systems, 2020.
