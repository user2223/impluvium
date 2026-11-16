<!--
SPDX-FileCopyrightText: 2026 the impluvium contributors
SPDX-License-Identifier: 0BSD
-->

# Trust establishment and pinning

## 1. Trust context

A device trusts a server through a **trust context** that it stores after a confirmed pairing. A trust context contains:

| Element | Source | Meaning |
|---|---|---|
| Base URL | entered by the user before pairing | Where the server's API lives ([transport.md §2](transport.md#2-base-url-and-origins)) |
| `instance_id` | discovery at pairing time, confirmed by the pairing result | Identity of the server instance |
| Content public key | pairing result (`PollPairingResponse.content_public_key`) | The only key trusted for manifests and descriptors of this instance |
| `device_id` | pairing result | The device's identity at this instance |
| Device private key | generated on the device | Signs session proofs |

The pin is the pair (`instance_id`, content public key), bound to the base URL. The origin of the base URL is not a
substitute for the `instance_id`, and a successful TLS connection is not a substitute for the pin.

## 2. Establishing a trust context

1. The client reads discovery from the base URL ([api.md §2](api.md#2-discovery)) and validates it (§4.1 steps 1 to 3
   and 5 to 6). Pairing requests are sent on a connection on which this discovery check ran (§4.2 applies with the
   comparison of step 4 omitted, because there is no pin yet).
2. It runs the pairing flow ([proofs.md §3](proofs.md#3-pairing)) with the `instance_id` from discovery in every pairing
   proof.
3. On success it validates the `PollPairingResponse` in this order. This is the complete check order of that message
   ([encoding.md §7.1](encoding.md#71-semantic-check-orders)); the first failing check abandons the pairing with its
   reason:
   1. Field rules (stage 1 of the generic order, [encoding.md §7.2](encoding.md#72-the-generic-order)) of all fields,
      with the reasons of the field table ([api.md §6.1](api.md#61-authservice)).
   2. `PollPairingResponse.instance_id` equals the `instance_id` it used (`instance_mismatch`).
   3. `PollPairingResponse.content_public_key` passes the point checks of
      [envelope.md §4.2](envelope.md#42-acceptance-profile) step 2 (`invalid_trusted_key`).
   4. If discovery lists one or more fingerprints with role `KEY_ROLE_CONTENT`, one of them equals SHA-256 of the
      received content public key (`content_key_changed`).
4. Only then does the client store the trust context, atomically and in one write.

The pairing result is trusted because it arrives over the transport that [transport.md](transport.md) requires and
because the user confirmed the pairing code in an authenticated browser session. The first pairing is a
trust-on-first-use step; [security.md §3](security.md#3-trust-on-first-use) states its limits.

## 3. States

| State | Condition | Effect |
|---|---|---|
| `trusted` | All comparisons of §4 succeed. | Authenticated calls and content acceptance proceed. |
| `instance_mismatch` | Discovery at the base URL reports another `instance_id` than the pinned one. | Stop. |
| `content_key_changed` | Same `instance_id`, but discovery lists content-key fingerprints and none matches the pinned content key. | Stop. |

"Stop" means: no further authenticated call (no session proof, no bearer token, no ticket) is sent for this trust
context, and no manifest or descriptor is accepted for it, until the user confirms a new pairing. The client MUST NOT
delete, overwrite or replace the stored credentials or the pin automatically in either state. A new pairing that the
user explicitly confirms creates a new trust context; the old one is replaced only after the new pairing has succeeded.

These are local trust states. They are never sent by a server and never derived from a server's error reason.

## 4. Checks before every credential transfer

### 4.1 The checks

The **pin checks** are the following checks, in this order; the first failing check determines the state or reason.

1. Transport preflight of [transport.md §2.1](transport.md#21-request-preflight-order) for the discovery request, then
   its response rules (no redirect, no credential).
2. Bounded read and encoding profile of the discovery document.
3. The checks of [api.md §2.3](api.md#23-validation) in their order: `discovery = 1` and `product = "impluvium"`
   (`not_an_impluvium_server`), field rules, manifest format 3 (`protocol_unsupported`).
4. Pin comparison: `instance_id` (§3, `instance_mismatch`), then content-key fingerprints (§3, `content_key_changed`).
5. Protocol version comparison ([api.md §3](api.md#3-version-check)): `protocol_unsupported`, `server_too_old`,
   `client_too_old`.
6. Required client capabilities ([api.md §4](api.md#4-capabilities)): `client_too_old`.

None of the states of steps 3 to 6 deletes credentials or signs the device out. They are typed states that the user
interface explains.

### 4.2 Verified connections

The result of the pin checks belongs to one transport connection, never to a connection pool, a client object or a
period of time.

1. A **transport connection** is one TCP connection to the server origin together with its TLS session, if any. All
   HTTP/1.1 requests and HTTP/2 streams on it belong to it.
2. A connection is **verified** after the pin checks of §4.1 have succeeded with a discovery request sent and answered
   on that same connection, after it was established. A connection is never verified by a check that ran on another
   connection.
3. A client sends a session proof or an access token to the server origin only on a verified connection. A new
   connection, a connection that replaces a closed one, and a connection that was re-established after a TLS or
   network error are unverified until they run the pin checks themselves.
4. If the pin checks fail on a connection, the client sends no credential on it, and the trust context enters the state
   of §3 that the failing check names.
5. A depot ticket is attached to a request only if it was received in a response on a verified connection of the same
   trust context and the trust context has not entered a stop state since. A depot connection itself, also one to the
   server origin, does not run the pin checks; the origin rules of [transport.md §5](transport.md#5-depot-origins-and-credentials)
   govern it.
6. Every HTTP stack of a client applies these rules in the same way.

## 5. Discovery fingerprints

Discovery fingerprints are informational. Clients display them and use them only to detect a mismatch (§3). A
fingerprint never makes a key trusted, never replaces the pinned content key and never extends a trusted key set. A
fingerprint entry with an unknown role or a `sha256` of a length other than 32 bytes is ignored.

## 6. Device credential context

1. There is exactly one device identity per (operating-system user, `instance_id`). The desktop client and the
   command-line client of the same operating-system user share this identity and its single pairing through one
   credential store implementation. The game-start helper process needs no credentials.
2. The persisted trust context is a versioned record. Data that is specific to an operating system (for example, a
   reference to an entry in an operating-system key store) is marked as such in the record.
3. The trust context is written atomically: a reader sees either the previous complete record or the new complete
   record.
4. Credentials are deleted only by an explicit user action (sign-out, which first calls `RevokeSelf`) or by the rule of
   [errors.md §7](errors.md#7-credential-cleanup).
5. Moving an instance to another base URL is not handled automatically in this revision; it requires an explicit user
   action.
