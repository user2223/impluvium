<!--
SPDX-FileCopyrightText: 2026 the impluvium contributors
SPDX-License-Identifier: 0BSD
-->

# Security model

This chapter states what the protocol's signatures, tickets and tokens protect and where their protection ends. It adds
no wire rule; the rules are in the other chapters.

## 1. What the signatures protect

- Manifests and release descriptors are signed with the server's content key. A depot, a reverse proxy, an object store
  or a network path between server and client cannot change a file tree, a chunk or a launch command without detection:
  every chunk and file is checked against its hash, and the hashes and commands are covered by the signature.
- The content key is held by the server. A compromised server can sign anything. The protocol does not protect against
  a malicious or compromised server, and clients still validate every signed document as hostile input, because a
  correct signature does not make a document well-formed.
- Signed bytes are verified exactly as received. Re-encoding never happens, so differences between Protocol Buffers
  implementations cannot change what was signed.

## 2. Transport confidentiality

TLS provides confidentiality. Over plain `http` (permitted only for loopback hosts or in the explicit insecure
local-network mode, [transport.md §3](transport.md#3-encryption)) signatures still protect integrity, but access tokens,
depot tickets and the content itself can be read by anyone on the path, and a captured token or ticket can be replayed
until it expires.

## 3. Trust on first use

The content key is pinned at the first pairing ([trust.md](trust.md)). An attacker who controls the network path during
that first pairing and can present a certificate the client accepts could pin a key of their choice. Mitigations: TLS
with certificate verification, approval of the pairing code only from a fully authenticated session with completed
multi-factor authentication, display of the content-key fingerprint, and the consistency check against discovery during
pairing. After pairing, a different instance or a different content key at the same URL stops the client instead of
replacing the pin.

## 4. Credentials and their lifetime

| Credential | Lifetime | Revocation | Bound to |
|---|---|---|---|
| Device private key | until revoked or idle-expired (180 days without a session by default) | server-side, immediate for new sessions | the device |
| Access token | 15 minutes by default | immediate: the server checks it on every request | device, user, session |
| Depot ticket | at most 60 minutes | not revocable; it stays usable until `exp`; refresh is refused after revocation | instance, device, user, manifest set, time window |
| Session challenge | 60 seconds, single use | consumed atomically | device |
| Pairing | 10 minutes, completes once | denial, expiry | the pairing's device key |

A stolen device key can open sessions until the device is revoked or idle-expired. A leaked ticket gives access to the
chunks of its manifests until it expires. Tickets carry no secret beyond their signature and are not bound to a TLS
channel.

## 5. Pairing phishing

A user can be persuaded to approve a code that an attacker's device displays. The approval page therefore shows the
requesting device's name, platform, address and time, asks for an explicit confirmation, accepts approval only from a
fully authenticated session, limits wrong-code attempts per user and per address, and lets codes expire after 10
minutes.

## 6. Hostile input

Every input crosses the pre-decode limits and the encoding profile before it is decoded, on the server for requests and
on the client for responses, including error bodies and error details. A malformed or oversized error body can neither
exhaust memory nor trigger a credential deletion ([errors.md §7](errors.md#7-credential-cleanup)).

## 7. Keys and recovery

All server signing keys are derived from the instance key. The content key does not rotate in this revision; losing the
instance key means that every device has to pair again. Depot-ticket keys switch generations with an overlap that keeps
issued tickets valid until they expire ([keys.md §4](keys.md#4-depot-ticket-key-generations)).
