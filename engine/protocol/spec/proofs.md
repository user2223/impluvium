<!--
SPDX-FileCopyrightText: 2026 the impluvium contributors
SPDX-License-Identifier: 0BSD
-->

# Device proofs, sessions and pairing

A device proves possession of its private key with two kinds of signed proof: a **session proof** over a single-use
server challenge, and a **pairing proof** over a pending pairing. Neither proof contains a device timestamp; no rule in
this chapter depends on the device clock. Schema: [`impluvium/v2/proofs.proto`](../proto/impluvium/v2/proofs.proto),
[`impluvium/v2/auth.proto`](../proto/impluvium/v2/auth.proto).

## 1. Messages

### 1.1 `SessionProof`

Payload type `application/vnd.impluvium.session-proof.v1+protobuf`, signed with the device key.

| No. | Field | Type | Presence | Limits | Meaning |
|---|---|---|---|---|---|
| 1 | `instance_id` | `string` | required | canonical UUID text | Instance the proof is for. |
| 2 | `device_id` | `string` | required | canonical UUID text | Device that signs. |
| 3 | `challenge` | `bytes` | required | exactly 32 bytes | Challenge from `BeginSession`. |

### 1.2 `PairingProof`

Payload type `application/vnd.impluvium.pairing-proof.v1+protobuf`, signed with the device key.

| No. | Field | Type | Presence | Limits | Meaning |
|---|---|---|---|---|---|
| 1 | `instance_id` | `string` | required | canonical UUID text | Instance the proof is for. |
| 2 | `pairing_id` | `bytes` | required | exactly 16 bytes | Pairing from `StartPairing`. |

A proof envelope, including its payload, is at most 4 096 bytes. Fields that this schema does not define (for example
a pairing token or a device timestamp) are unknown fields: they are skipped and never stand in for a required field.

### 1.3 Common request checks

For `BeginSession`, `CompleteSession`, `StartPairing` and `PollPairing`, the server first checks, in this order:

1. The HTTP method and request content coding ([api.md §5](api.md#5-connect-unary-calls) item 3). A rejection is
   `unimplemented` (HTTP 501), without a wire reason, before any body read or proof check.
2. For `StartPairing` only, the client-address rate limit (§3.1); if reached, return `resource_exhausted`, `RATE_LIMITED`.
3. The request-body limit ([api.md §5](api.md#5-connect-unary-calls) item 7), then its encoding profile. No method's
   field, device, pairing, proof or capacity check precedes these checks.

The method-specific orders below follow this prefix. Their body-limit and profile rows refer to step 3 here and
do not run a second time. A failure of the prefix changes no session, challenge or pairing state.

## 2. Sessions

### 2.1 `BeginSession`

After §1.3:

1. The request carries `device_id` (canonical UUID text, otherwise `invalid_uuid`).
2. If no device with this `device_id` is registered, the server answers `unauthenticated` without an error reason
   (`unknown_device`). The server reports revocation and idle expiry only after a valid proof (§2.2); it issues a
   challenge to every registered device.
3. The server creates a challenge of 32 bytes from a cryptographically secure random generator, bound to the
   `device_id`, with `issued_at = now` and `expires_at = issued_at + 60 s`, and returns `challenge` and `expires_at`.
4. If the challenge store has no free capacity, the server refuses with `resource_exhausted` and reason `RATE_LIMITED`.
   It never evicts a challenge that has not yet expired to make room.

### 2.2 `CompleteSession`

The server applies §1.3, then processes a `CompleteSessionRequest` in this order; the first failing check determines
the result. Checks 1 to 3 change no state.

| Order | Check | Local reason | Wire result |
|---:|---|---|---|
| 1 | Body limits and encoding profile ([limits.md §4](limits.md#4-rpc-bodies)) | profile and limit reasons | `invalid_argument` or `resource_exhausted` |
| 2a | `device_id` canonical UUID text | `invalid_uuid` | `invalid_argument` |
| 2b | `challenge` exactly 32 bytes | `invalid_challenge_length` | `invalid_argument` |
| 2c | `proof` present | `missing_field` | `invalid_argument` |
| 2d | `scope = SESSION_SCOPE_API` (an absent scope is `SESSION_SCOPE_UNSPECIFIED`) | `unsupported_session_scope` | `invalid_argument` |
| 2e | `client_capabilities` in order: each a capability name (`invalid_name`), not equal to an earlier one (`duplicate_capability`) | as listed | `invalid_argument` |
| 2f | Every capability in the server's `required_client_capabilities` is offered | `client_too_old` | `failed_precondition`, `CLIENT_TOO_OLD` |
| 3a | `proof` at most 4 096 bytes | `envelope_too_large` | `resource_exhausted` |
| 3b | A device with `device_id` is registered | `unknown_device` | `unauthenticated`, `CHALLENGE_INVALID` |
| 3c | `proof` passes [envelope.md §7](envelope.md#7-verification-procedure) steps 2 to 11 with payload type `…session-proof.v1…` and the registered device key as the only trusted key | envelope reasons | `unauthenticated`, `CHALLENGE_INVALID` |
| 3d | `SessionProof` fields: canonical UUIDs, 32-byte challenge | `invalid_uuid`, `invalid_challenge_length` | `unauthenticated`, `CHALLENGE_INVALID` |
| 3e | `proof.instance_id` equals the server's `instance_id` | `proof_instance_mismatch` | `unauthenticated`, `CHALLENGE_INVALID` |
| 3f | `proof.device_id` equals the request's `device_id` | `proof_device_mismatch` | `unauthenticated`, `CHALLENGE_INVALID` |
| 3g | `proof.challenge` equals the request's `challenge` | `proof_challenge_mismatch` | `unauthenticated`, `CHALLENGE_INVALID` |
| 4a | The device is not revoked | `device_revoked` | `unauthenticated`, `DEVICE_REVOKED` |
| 4b | The device is not idle-expired (§2.4) | `device_expired` | `unauthenticated`, `DEVICE_EXPIRED` |
| 4c | The challenge exists and was issued to this device | `challenge_unknown` | `unauthenticated`, `CHALLENGE_INVALID` |
| 4d | `now < expires_at` of the challenge | `challenge_expired` | `unauthenticated`, `CHALLENGE_INVALID` |
| 4e | The challenge is still open | `challenge_consumed` | `unauthenticated`, `CHALLENGE_INVALID` |
| 5 | In one atomic operation, with the current server time `t`: checks 4a to 4e run again in this order (with `now = t`); if all pass, the challenge becomes consumed, the device's activity time (§2.4) becomes `t`, and the session with its token hash is recorded | as in 4a to 4e | as in 4a to 4e |
| 6 | Return the access token (§2.3) | – | success |

A request that fails before step 5, or in step 5, neither consumes the challenge nor reserves a replay entry nor
changes the activity time. Steps 4a to 4e are an early rejection only; the decision is taken in step 5, so a device
that is revoked or reaches its idle limit between step 4 and step 5 is refused in step 5 with `device_revoked` or
`device_expired`, and its activity time is not renewed. A revocation that happens after step 5 takes effect on the
next request with the token (§2.4). Of several concurrent requests with valid proofs for the same challenge, exactly
one succeeds.

### 2.3 Access tokens

1. An access token is 32 bytes from a cryptographically secure random generator, transmitted as 43 characters of
   unpadded base64url ([encoding.md §6.5](encoding.md#65-base64url-without-padding)). The server stores only a hash of
   it and checks it on every request, so revocation takes effect immediately.
2. Default lifetime: 15 minutes (configurable by the operator). `expires_at` states the expiry.
3. `accepted_capabilities` lists, in request order, the offered capabilities that the server implements. Capabilities
   are stored with the session, never with the device.
4. `scope` is `SESSION_SCOPE_API`. `SESSION_SCOPE_WEB` is reserved and not issued in this revision.
5. A token is sent as `Authorization: Bearer <access_token>` on API requests only ([transport.md §5](transport.md#5-depot-origins-and-credentials)).
6. Any number of local processes of the same device may mint tokens independently through their own sessions.

### 2.4 Token use and device state

On every request that requires an access token, the server checks in this order:

1. Exactly one `Authorization` header field with the Bearer scheme; a missing or repeated field or another scheme
   is `unauthenticated`, without a wire reason. Cookie sessions do not authenticate these requests.
2. The token is known (`unauthenticated`, no reason).
3. The token is not expired (`unauthenticated`, `TOKEN_EXPIRED`).
4. Its device is not revoked (`unauthenticated`, `DEVICE_REVOKED`).
5. Its device is not idle-expired (`unauthenticated`, `DEVICE_EXPIRED`).

`RevokeSelf` revokes the calling device; a client that signs out calls it before it deletes its local state.

**Idle expiry.** Each device has an **activity time**: the time `t` recorded by the atomic step 5 of its last
successful `CompleteSession` (§2.2), or, if it has never completed a session, the time at which its pairing completed
(step 13 of §3.4). With the idle limit `D` (default 15 552 000 seconds, that is 180 days of 86 400 seconds;
configurable by the operator), the device is idle-expired at every time `now ≥ activity time + D`. A device that paired
and never opened a session therefore expires `D` after pairing. An idle-expired device stays expired; a later successful
proof does not renew it, because the atomic step 5 of §2.2 refuses it before it would record a new activity time.

## 3. Pairing

The pairing flow follows the shape of the OAuth 2.0 device authorization grant (RFC 8628): the device starts a pairing,
the user confirms a short code in an authenticated browser session, and the device polls until the pairing completes.
No deep link is part of this revision.

### 3.1 `StartPairing`

Unauthenticated and rate-limited per client address (`resource_exhausted`, `RATE_LIMITED`). The rate check is at §1.3
step 2, before the body is read. After §1.3, check the fields in order:

| Order | Field | Rule | Reason |
|---:|---|---|---|
| 1 | `device_name` | 1–128 bytes, display text | `invalid_text` |
| 2 | `platform` | 1–32 bytes, display text | `invalid_text` |
| 3 | `arch` | 1–32 bytes, display text | `invalid_text` |
| 4 | `public_key` | exactly 32 bytes, passes [envelope.md §4.2](envelope.md#42-acceptance-profile) step 2 | `invalid_public_key` |
| 5 | `client_version` | 1–64 bytes, display text | `invalid_text` |

The server then creates a pending pairing with:

- `pairing_id`: 16 bytes from a cryptographically secure random generator, known only to the requesting device;
- `user_code`: 8 characters drawn uniformly and independently from the alphabet of §3.2, unique among open pairings;
- `interval_seconds`: 5;
- `expires_at`: `now + 600 s`;
- `verification_uri`: the absolute URL of the approval page; `verification_uri_complete`: the same URL with the code
  filled in. Both are at most 2 048 bytes and use the scheme and host of the server's external URL.

### 3.2 User code

The alphabet has exactly these 32 characters:

```
0 1 2 3 4 5 6 7 8 9 A B C D E F G H J K M N P Q R S T V W X Y Z
```

(digits, and the uppercase Latin letters without `I`, `L`, `O` and `U`). A code is 8 characters, about 40 bits. On the
wire and on screen it is grouped as `XXXX-XXXX`: four characters, `-` (U+002D), four characters, 9 bytes in total.

To compare a code that a user typed, the approval page removes every `-` and every space (U+0020), maps `a`–`z` to
`A`–`Z`, and then requires exactly 8 characters of the alphabet; anything else is not a valid code. No other
substitution takes place. Wrong-code attempts are rate-limited per user and per client address; a wrong code is not
attributed to any pairing.

### 3.3 Approval

Approval happens in the server's web interface and is outside the device API. It requires a fully authenticated
session with completed multi-factor authentication, shows the device name, platform, requesting address and time, and
requires an explicit confirmation. The user can deny the pairing instead.

### 3.4 `PollPairing`

The device polls no more often than every `interval_seconds`. The server applies §1.3, then processes a request in
this order:

| Order | Check | Local reason | Wire result |
|---:|---|---|---|
| 1 | Body limits and encoding profile | profile and limit reasons | `invalid_argument` or `resource_exhausted` |
| 2 | `pairing_id` exactly 16 bytes | `invalid_pairing_id_length` | `invalid_argument` |
| 3 | `proof` present (`missing_field`, `invalid_argument`), then at most 4 096 bytes (`envelope_too_large`, `resource_exhausted`) | as listed | as listed |
| 4 | A pairing with `pairing_id` exists and has not completed | `pairing_unknown` | `failed_precondition`, `PAIRING_EXPIRED` |
| 5 | `proof` passes [envelope.md §7](envelope.md#7-verification-procedure) steps 2 to 11 with payload type `…pairing-proof.v1…` and the pairing's `public_key` as the only trusted key | envelope reasons | `unauthenticated`, `CHALLENGE_INVALID` |
| 6 | `PairingProof` fields: canonical UUID, 16-byte `pairing_id` | `invalid_uuid`, `invalid_pairing_id_length` | `unauthenticated`, `CHALLENGE_INVALID` |
| 7 | `proof.instance_id` equals the server's `instance_id` | `proof_instance_mismatch` | `unauthenticated`, `CHALLENGE_INVALID` |
| 8 | `proof.pairing_id` equals the request's `pairing_id` | `proof_pairing_mismatch` | `unauthenticated`, `CHALLENGE_INVALID` |
| 9 | `now < expires_at` of the pairing | `pairing_expired` | `failed_precondition`, `PAIRING_EXPIRED` |
| 10 | The pairing was not denied | `pairing_denied` | `permission_denied`, `PAIRING_DENIED` |
| 11 | The previous accepted poll lies at least the current interval in the past | `slow_down` | `resource_exhausted`, `SLOW_DOWN`, `retry_after_seconds` = new interval |
| 12 | The pairing is approved; otherwise it is pending | `pairing_pending` | `failed_precondition`, `PAIRING_PENDING` |
| 13 | In one atomic operation, with the current server time, the checks of steps 4, 9, 10 and 12 run again in this order; if all pass, the device is registered and the pairing becomes completed | as in steps 4, 9, 10, 12 | as in steps 4, 9, 10, 12 |
| 14 | Return `device_id`, `instance_id` and `content_public_key` | – | success |

Polls that end at step 11 or 12 are accepted polls and are repeatable. A failure at steps 1 to 8 changes no pairing state.
On `SLOW_DOWN` the device adds 5 seconds to its polling interval for all later polls of this pairing. After the single
successful completion, every further poll with the same `pairing_id` ends at step 4. A pairing that has been deleted or
has expired is never recreated from a proof.

The client then checks the result as described in [trust.md §2](trust.md#2-establishing-a-trust-context) and stores the
trust context.

## 4. Replay and state invariants

These invariants bind the server's state handling; this specification defines them and the test models check them with
injected time. Persistence, capacity planning and clean-up belong to the server.

### 4.1 Challenges

1. `expires_at = issued_at + 60 s`, set by the server. Acceptance requires `now < expires_at` on the server clock. There
   is no tolerance window and no device timestamp.
2. Consumption re-checks expiry and state in the same atomic operation that marks the challenge consumed. Of two
   concurrent valid completions exactly one succeeds.
3. Invalid signatures or bindings never consume a challenge and never reserve a replay entry.
4. The consumed state is kept at least until `expires_at`. After `expires_at` the challenge is invalid for ever;
   deleting it never makes it usable again.
5. When the store is full, new challenges are refused; challenges that have not expired are never evicted.
6. After a server restart a challenge is either in its previous state or treated as expired or unknown; it is never
   open again after having been consumed.

### 4.2 Pairings

1. A pairing expires 600 seconds after `StartPairing`, on the server clock: it can complete only while
   `now < expires_at`.
2. Polling is repeatable while the pairing is open; completion happens atomically and at most once. The atomic
   completion re-checks completion state, expiry, denial and approval with the current time (§3.4 step 13), so a
   request that passed step 9 shortly before expiry cannot register a device after `expires_at`.
3. Unknown, deleted or completed pairing ids are never recreated or reopened from an old proof.
4. A device is registered only by the completion step of §3.4.

### 4.3 Model test cases

The state-model vectors cover at least: completion exactly at and just before `expires_at`; consumed state kept until
expiry; a full store without eviction; restart before and after consumption; no consumption after an invalid signature;
exactly one success out of parallel completions; repeated pending polls; a completed pairing polled again; a pairing
that expires between step 9 and step 13; idle expiry of a device that never opened a session; a device that reaches its
idle limit, or is revoked, between step 4 and step 5 of §2.2; challenge
lengths of 31, 32 and 33 bytes; pairing ids of 15, 16 and 17 bytes; and user codes that violate §3.2.
