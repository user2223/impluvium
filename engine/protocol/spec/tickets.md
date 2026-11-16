<!--
SPDX-FileCopyrightText: 2026 the impluvium contributors
SPDX-License-Identifier: 0BSD
-->

# Depot tickets and the depot API

A depot ticket authorises its holder to fetch the chunks of a fixed set of manifests for a short time. The server issues
it after one entitlement check; the depot verifies it offline with depot-ticket public keys only and needs no call to
the server per request.

Payload type: `application/vnd.impluvium.depot-ticket.v1+protobuf`, signed with a depot-ticket key
([keys.md §4](keys.md#4-depot-ticket-key-generations)). Schema:
[`impluvium/v2/ticket.proto`](../proto/impluvium/v2/ticket.proto).

## 1. `DepotTicketClaims`

| No. | Field | Type | Presence | Limits | Meaning |
|---|---|---|---|---|---|
| 1 | `aud` | `string` | required | canonical UUID text | `instance_id` of the issuing server. |
| 2 | `sub` | `string` | required | canonical UUID text | `device_id` of the device the ticket was issued to. |
| 3 | `user_id` | `string` | required | canonical UUID text | User the ticket was issued for. |
| 4 | `manifest_ids` | repeated `bytes` | required | 1–8 elements, each exactly 32 bytes, strictly ascending | The requested version's manifest and the manifests of all resolved dependencies. |
| 5 | `iat` | `Timestamp` | required | [encoding.md §5](encoding.md#5-timestamps) | Issue time. |
| 6 | `exp` | `Timestamp` | required | [encoding.md §5](encoding.md#5-timestamps) | Expiry time. |

The whole binary ticket envelope is at most 4 096 bytes ([envelope.md §2](envelope.md#2-payload-types)).

## 2. Issuance

1. `OpenDownload` and `RefreshTicket` ([api.md §6.4](api.md#64-downloadservice)) issue tickets after the server's
   entitlement check for the requested version. Refresh repeats the entitlement check; a device or user that has lost
   access receives no new ticket.
2. `manifest_ids` is the complete resolved set: the requested version and the transitive closure of its `requires`
   ([descriptor.md §7](descriptor.md#7-publication-checks)). The set has at most 8 manifests including the requested
   one. The server never shortens the set silently; a version whose closure is larger cannot be published.
3. Default lifetime `exp − iat` is 3 600 seconds. A server MAY be configured with a shorter lifetime of at least 300
   seconds. The format maximum `max_ttl` is 3 600 seconds.
4. Tickets are issued only under the *current* depot-ticket key ([keys.md §4.2](keys.md#42-switching-generations)).

## 3. Claims rules

1. `0 < exp − iat ≤ 3 600` seconds, computed exactly on `(seconds, nanos)`.
2. `manifest_ids` has 1 to 8 elements, each 32 bytes, strictly ascending by bytes (no duplicates).
3. `aud`, `sub` and `user_id` are canonical UUID text.

## 4. Verification

Inputs: the ticket envelope bytes; the acceptance set ([keys.md §4.1](keys.md#41-acceptance-set)); the current time
`now`; the expected audience (the verifier's own `instance_id`); the requested `manifest_id`; optionally an expected
`sub` and an expected `user_id`. Time is supplied by the caller; verifiers do not read a clock themselves.

Steps 1 to 11 of [envelope.md §7](envelope.md#7-verification-procedure) run first, with the expected payload type
`…depot-ticket.v1…` and the acceptance set as trusted set. Then:

| Order | Check | Reason |
|---:|---|---|
| 1 | `iat` and `exp` present | `missing_field` |
| 2 | `0 < exp − iat ≤ 3 600 s` | `invalid_validity_window` |
| 3 | `now < exp` | `ticket_expired` |
| 4 | `iat ≤ now` | `ticket_not_yet_valid` |
| 5 | For every valid signature by a *retiring* key of the acceptance set, in envelope order: `iat < t_switch` of that key ([keys.md §4.3](keys.md#43-verification-rule)) | `ticket_key_retired` |
| 6 | `aud` canonical UUID text | `invalid_uuid` |
| 7 | `aud` equals the expected audience | `wrong_audience` |
| 8 | `sub`, then `user_id`, canonical UUID text | `invalid_uuid` |
| 9 | `manifest_ids` not empty | `missing_field` |
| 10 | Each element in order: 32 bytes (`invalid_manifest_id`), greater than the previous (`unsorted_manifest_ids`) | as listed |
| 11 | If an expected `sub` or `user_id` is supplied, it is equal | `wrong_subject` |
| 12 | The requested `manifest_id` is an element of `manifest_ids` | `ticket_scope` |

`iat` and `exp` have already passed the timestamp range check of [encoding.md §5](encoding.md#5-timestamps) during
step 11 of the envelope procedure (the payload scan), so steps 2 to 5 compare valid timestamps only. A ticket is
accepted exactly at times `iat ≤ now < exp`; at `now = exp` it is expired. There is no clock tolerance: issuer and depot
use the same server time source. A verification cache MAY shortcut the envelope steps and steps 1 to 10 under the rules
of [keys.md §5](keys.md#5-ticket-verification-cache); steps 3 to 5 and 11 to 12 are re-evaluated on every hit.

## 5. Transport of tickets

1. A ticket travels as `Authorization: Bearer <t>`, where `<t>` is the unpadded base64url encoding
   ([encoding.md §6.5](encoding.md#65-base64url-without-padding)) of the exact binary envelope bytes. The 4 096-byte
   limit applies to the binary envelope before encoding; the encoded token is at most 5 462 characters and the header
   value at most 5 469 bytes.
2. A client sends a ticket only to a depot origin that is allowed for the download ([transport.md §5](transport.md#5-depot-origins-and-credentials)).
   A ticket is never sent with an API call, and an API access token is never sent to a depot.
3. Clients do not verify tickets; they treat them as opaque bytes of at most 4 096 bytes. A client MAY decode the claims
   without verification only to schedule a refresh.
4. A client refreshes a ticket when half of its lifetime has passed, or when the depot answers 401 with reason
   `TICKET_EXPIRED`, and then continues the download.

## 6. Promises after revocation

- **Depot tickets** are verified offline. A ticket issued before a device was revoked or before a user lost access
  stays usable until its `exp`, at most 3 600 seconds. The next refresh is refused.
- **API access tokens** are checked by the server on every request. Revoking a device or a token takes effect on the
  next request.

## 7. Depot HTTP API

### 7.1 Request

```
GET {depot_base_url}/depot/v2/{manifest_id_hex}/{chunk_id_hex}
Authorization: Bearer <ticket>
Range: bytes=<range>          (optional)
```

`{depot_base_url}` is a `DepotEndpoint.base_url` ([api.md §6.4](api.md#64-downloadservice)); it may contain a path
prefix, and the depot serves the route below that prefix. `{manifest_id_hex}` and `{chunk_id_hex}` are 64 lowercase
hexadecimal digits each. No query string is used. Only `GET` is defined; other methods receive 405. Which requests
carry a ticket is defined in [transport.md §5](transport.md#5-depot-origins-and-credentials).

### 7.2 Processing order

The first failing step determines the result; the rate/concurrency row retains its explicitly permitted placement
after step 2. Steps 0 and 0a are the common GET dispatch checks of
[api.md §1.1](api.md#11-common-get-dispatch-checks); neither validates a path identifier or reads credentials.

| Order | Condition | Status | Reason in the error body |
|---:|---|---:|---|
| 0 | The server is starting | 503 | `SERVER_STARTING` |
| 0a | The method is not `GET` | 405 | – (`unimplemented`) |
| 1 | No `Authorization` header, more than one `Authorization` header field, not the `Bearer` scheme, value longer than 5 469 bytes, or token not valid unpadded base64url | 401 | – |
| 2 | Ticket verification §4 steps up to 11 fails | 401 | `TICKET_EXPIRED` for `ticket_expired`, otherwise none |
| 3 | A path segment is not 64 lowercase hexadecimal digits | 404 | – |
| 4 | `manifest_id` is not in the ticket (§4 step 12) | 403 | `TICKET_SCOPE` |
| 5 | The chunk is not part of that manifest | 404 | – |
| 6 | The stored source of the chunk changed since the manifest was built | 409 | `LIBRARY_CHANGED` |
| 7 | `Range` present but not exactly one satisfiable byte range | 416 | – |
| – | Rate or concurrency limit (per ticket subject), at any point after step 2 | 429 or 503 | `RATE_LIMITED` |

A 401 response carries `WWW-Authenticate: Bearer error="invalid_token"`. 429 and 503 responses carry `Retry-After`.
Error bodies use the error body format of [errors.md §3](errors.md#3-error-bodies). No route under `/depot/v2` returns
content without a valid ticket.

### 7.3 Successful responses

| Case | Status | Headers | Body |
|---|---:|---|---|
| No `Range` | 200 | `Content-Type: application/octet-stream`; `Content-Length` = chunk size; `ETag: "<chunk_id_hex>"`; `Accept-Ranges: bytes`; `Cache-Control: private, immutable` | the chunk bytes |
| One satisfiable range | 206 | as above, plus `Content-Range: bytes <first>-<last>/<chunk size>`; `Content-Length` = range length | the range |

`Range` follows RFC 9110 with exactly one `bytes` range (`first-last`, `first-` or `-suffix`). A header with several
ranges is answered with 416 and `Content-Range: bytes */<chunk size>`. Responses carry no `Content-Encoding`.

### 7.4 Client obligations

The client checks that the received length equals the chunk size (or the requested range length) and that the hash of
the complete chunk equals its id under the manifest's algorithm before it writes the bytes to any target. A mismatch
discards the bytes. Chunks are fetched with the limits of [limits.md §5](limits.md#5-http-bodies-outside-rpc).
