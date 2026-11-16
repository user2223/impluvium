<!--
SPDX-FileCopyrightText: 2026 the impluvium contributors
SPDX-License-Identifier: 0BSD
-->

# Examples *(informative)*

Every value in this chapter can be checked by hand: lengths, prefixes, tags and field order. Hashes, keys and
signatures are written as placeholders in angle brackets, for example `<32 bytes: challenge>`. Their real values come
from the known-answer vectors, which are produced independently of the shipped implementation.

Tag bytes follow from `(field_number << 3) | wire_type`: for wire type 2 (LEN), field 1 is `0a`, field 2 is `12`,
field 3 is `1a`, field 4 is `22`, field 5 is `2a`, field 6 is `32`, field 7 is `3a`; for wire type 0 (VARINT), field 1
is `08`.

## 1. Pre-authentication encoding of a session proof

A `SessionProof` with a 36-byte `instance_id`, a 36-byte `device_id` and a 32-byte `challenge` is encoded as:

| Bytes | Content |
|---|---|
| `0a 24` | field 1, LEN, length 36 |
| 36 bytes | ASCII of the `instance_id`, for example `<36 bytes: 0123abcd-…>` |
| `12 24` | field 2, LEN, length 36 |
| 36 bytes | ASCII of the `device_id` |
| `1a 20` | field 3, LEN, length 32 |
| 32 bytes | `<32 bytes: challenge>` |

Payload length: 2 + 36 + 2 + 36 + 2 + 32 = **110** bytes.

The payload type `application/vnd.impluvium.session-proof.v1+protobuf` has 51 bytes. The signed message is

```
"DSSEv1" SP "51" SP "application/vnd.impluvium.session-proof.v1+protobuf" SP "110" SP <110 payload bytes>
```

that is `44 53 53 45 76 31 20 35 31 20` followed by the 51 type bytes, `20 31 31 30 20` and the payload. Length:
6 + 1 + 2 + 1 + 51 + 1 + 3 + 1 + 110 = **176** bytes.

A pairing proof (`instance_id` 36 bytes, `pairing_id` 16 bytes) has the payload `0a 24 <36 bytes> 12 10 <16 bytes>` of
**56** bytes; its signed message starts with `DSSEv1 51 application/vnd.impluvium.pairing-proof.v1+protobuf 56 ` and is
6 + 1 + 2 + 1 + 51 + 1 + 2 + 1 + 56 = **121** bytes long.

## 2. Binary envelope of that proof

| Bytes | Content |
|---|---|
| `0a 33` | `payload_type`, length 51 |
| 51 bytes | `application/vnd.impluvium.session-proof.v1+protobuf` |
| `12 6e` | `payload`, length 110 |
| 110 bytes | the payload of §1 |
| `1a 4c` | one `signatures` element, length 76 |
| `0a 08` | `key_id`, length 8 |
| 8 bytes | `<8 bytes: first 8 bytes of SHA-256(device public key)>` |
| `12 40` | `sig`, length 64 |
| 64 bytes | `<64 bytes: Ed25519 signature over the 176-byte message>` |

Envelope length: 53 + 112 + 78 = **243** bytes, below the 4 096-byte limit. Metadata: 243 − 110 = 133 bytes.

## 3. Rejected envelope encodings

| Input (start) | Result | Rule |
|---|---|---|
| `0a 01 41 0a 01 41 12 …` (`payload_type` twice, equal values) | `duplicate_field` | [encoding.md §2.4](encoding.md#24-occurrence-rules) |
| `08 05 …` (field 1 with wire type VARINT) | `wrong_wire_type` | [encoding.md §2.1](encoding.md#21-tags-and-wire-types) |
| `1a 4c 0a 08 …` and a second `1a 4c 0a 08 …` with the same 8 `key_id` bytes | `duplicate_signature_key_id` | [envelope.md §5](envelope.md#5-multiple-signatures) |
| a valid envelope whose only signature has a `key_id` not in the trusted set | `no_trusted_signature` | [envelope.md §5](envelope.md#5-multiple-signatures) |
| an envelope with payload type `application/vnd.impluvium.revocation-list.v1+protobuf` | `unknown_payload_type` (payload not read) | [envelope.md §2](envelope.md#2-payload-types) |
| `0b …` (field 1 with wire type 3, a group) | `malformed_wire` | [encoding.md §2.1](encoding.md#21-tags-and-wire-types) |

## 4. Depot ticket sizes

A `DepotTicketClaims` with three 36-byte UUIDs, one `manifest_id`, and `iat`/`exp` whose `seconds` lie between
2^28 = 268 435 456 and 2^35 − 1 (5-byte varints) and whose `nanos` are 0:

| Bytes | Content | Length |
|---|---|---:|
| `0a 24` + 36 | `aud` | 38 |
| `12 24` + 36 | `sub` | 38 |
| `1a 24` + 36 | `user_id` | 38 |
| `22 20` + 32 | one `manifest_ids` element | 34 |
| `2a 06 08` + 5 | `iat` (`Timestamp` with `seconds` only) | 8 |
| `32 06 08` + 5 | `exp` | 8 |

Payload: **164** bytes; with eight manifests 38 × 3 + 34 × 8 + 16 = **402** bytes. The envelope with eight manifests and
one signature: `0a 32` + 50 type bytes (52), `12 92 03` + 402 payload bytes (405; 402 is the varint `92 03`), one
signature (78): **535** bytes. Its unpadded base64url form has 714 characters (535 = 3 × 178 + 1). The largest allowed
ticket, 4 096 bytes, has 5 462 characters (4 096 = 3 × 1 365 + 1), so the header `Authorization: Bearer <token>` has a
value of at most 7 + 5 462 = 5 469 bytes.

## 5. A small manifest

A version with these files, built with SHA-256:

| Path (sorted by bytes) | Type | Size | Notes |
|---|---|---:|---|
| `README.txt` | regular | 5 | one chunk; its id equals its file hash |
| `data.pak` | regular | 1 500 000 | two chunks of 800 000 and 700 000 bytes (illustrative boundaries) |
| `empty.txt` | regular | 0 | no chunk; hash of the empty string |
| `link` | symbolic link | 0 | target `data.pak`, `mode` 0 |
| `saves` | directory | 0 | empty directory |

`R` (0x52) sorts before `d` (0x64), `e` (0x65), `l` (0x6c) and `s` (0x73), so this is the entry order.

Suppose the three distinct chunk ids `<H_readme>`, `<C_1>` (first 800 000 bytes of `data.pak`) and `<C_2>` (last
700 000 bytes) sort as `<C_2>` < `<H_readme>` < `<C_1>`. Then:

| Index | `id` | `size` |
|---:|---|---:|
| 0 | `<C_2>` | 700 000 |
| 1 | `<H_readme>` | 5 |
| 2 | `<C_1>` | 800 000 |

| Entry | `chunk_indexes` | Packed encoding of field 7 |
|---|---|---|
| `README.txt` | `[1]` | `3a 01 01` |
| `data.pak` | `[2, 0]` (file order: first `<C_1>`, then `<C_2>`) | `3a 02 02 00` |
| others | `[]` | omitted |

Checks: `data.pak` sums to 800 000 + 700 000 = 1 500 000; `README.txt` has one chunk whose id equals its `hash`;
`total_size` = 5 + 1 500 000 + 0 = **1 500 005**; every chunk is referenced. An index list `[0, 1, 300]` would be packed
as `3a 04 00 01 ac 02` (300 = `ac 02`).

## 6. Paths

| Path | Base profile | Windows profile |
|---|---|---|
| `bin/game` | accepted | accepted |
| `aux.c` | accepted | `windows_reserved_name` |
| `Readme` next to `README` | accepted (distinct bytes) | `windows_case_collision` |
| `A/x` next to `a/y` | accepted | `windows_case_collision` (ancestors `A` and `a`) |
| `notes.` | accepted | `windows_trailing_dot_or_space` |
| `ab:c` | accepted | `windows_forbidden_character` |
| `c:/x` | `path_drive_prefix` | – |
| `/etc/passwd` | `path_absolute` | – |
| `a//b` | `path_empty_component` | – |
| `a/../b` | `path_dot_component` | – |
| `dir\file` | `path_forbidden_character` | – |
| `bad<U+0007>name` | `path_forbidden_character` | – |

## 7. Symbolic-link targets

| Link path | Target | Resolution | Result |
|---|---|---|---|
| `a/b/l` | `../x` | `[a, b]` → `[a]` → `[a, x]` | accepted |
| `a/b/l` | `../../x` | `[a, b]` → `[a]` → `[]` → `[x]` | accepted |
| `a/b/l` | `../../../x` | `[a, b]` → `[a]` → `[]` → escape | `symlink_escape` |
| `l` | `.` | `[]` → `[]` (top of the tree) | accepted |
| `l` | `..` | `[]` → escape | `symlink_escape` |
| `l` | `/usr/bin` | – | `path_absolute` |

## 8. User code

`K7QM-2XZB` is a valid code: 8 characters of the alphabet, grouped 4-dash-4. A user may type `k7qm 2xzb`; the approval
page removes the space, maps the letters to uppercase and compares `K7QM2XZB`. `K7QM-2XZI` is invalid (`I` is not in the
alphabet), and `K7QM-2XZ` is invalid (7 characters).

## 9. A depot request

```
GET /depot/v2/<64 lowercase hex digits: manifest_id>/<64 lowercase hex digits: chunk_id> HTTP/1.1
Host: games.example.org
Authorization: Bearer <unpadded base64url of the ticket envelope>
Range: bytes=0-65535
```

The path has 10 + 64 + 1 + 64 = 139 bytes. A successful answer is `206` with
`Content-Range: bytes 0-65535/<chunk size>`, `Content-Length: 65536` and `ETag: "<chunk_id hex>"`.

## 10. JSON input rules

| Request body (fragment) | Result |
|---|---|
| `{"deviceId":"…","device_id":"…"}` in `CompleteSessionRequest` | `duplicate_field` (two names of one field) |
| `{"scope":"SESSION_SCOPE_API","sc\u006fpe":"SESSION_SCOPE_API"}` | `duplicate_json_key` (same name after escape resolution) |
| `{"scope":"SESSION_SCOPE_FUTURE"}` | reads as `SESSION_SCOPE_UNSPECIFIED`, then `unsupported_session_scope` |
| `{"scope":true}` | `json_type_mismatch` |
| `{"clientCapabilities":["events.v1",null]}` | `json_type_mismatch` |
| `{"pageSize":"1e2"}` in `ListLibraryRequest` | accepted as 100 (a standard ProtoJSON integer form) |
| `{"pageSize":"1.5"}` | `invalid_number` |

In a response, `{"platforms":["PLATFORM_LINUX","PLATFORM_FUTURE"]}` reads as `[PLATFORM_LINUX]`, while
`{"platforms":["PLATFORM_LINUX",true]}` is rejected (`json_type_mismatch`).

## 11. Event stream

```
retry: 1000

event: connected
data: {"epoch":"<24 characters: base64 of 16 bytes>","time":"2026-01-01T00:00:00Z","connected":{"headSeq":"41","resumed":true}}

id: <32 lowercase hex digits: epoch>-42
event: library_changed
data: {"epoch":"<…>","seq":"42","time":"2026-01-01T00:00:01Z","prevSeq":"41","libraryChanged":{"gameId":"<uuid>","game":{"gameId":"<uuid>","name":"Example","entryVersion":"42"}}}

id: <32 lowercase hex digits: epoch>-42
event: heartbeat
data: {"epoch":"<…>","seq":"42","time":"2026-01-01T00:00:31Z","heartbeat":{}}
```

The `retry` value is chosen by the server. `Connected` carries no `seq`, so ProtoJSON omits it. The client resumed
with `<epoch>-41`, so the stream chain starts at 41: event 42 carries `prevSeq` 41. A client whose last applied sequence
number was 41 applies it (`prevSeq` 41 ≤ 41 < 42) and then sees from the heartbeat (`seq` 42) that nothing was lost.

## 12. An error body

```json
{"code":"unauthenticated","message":"device revoked","details":[{"type":"impluvium.v2.ErrorInfo","value":"CAU"}]}
```

`CAU` is the unpadded base64 of the two bytes `08 05`: field 1 (`reason`) with value 5, `DEVICE_REVOKED`. A client that
received this body from the pinned server origin deletes the credentials of that trust context
([errors.md §7](errors.md#7-credential-cleanup)). The same JSON from another origin, or a `502` HTML page, never does.

For these responses to `ListDevices`, the success-body read limit is 1 048 576 bytes and the error-body read limit is
65 536 bytes ([limits.md §4–5](limits.md#4-rpc-bodies)). Header-size and content-coding checks pass. HTML pages have
`Content-Type: text/html`; JSON bodies have `Content-Type: application/json`. Any body read completes within its
deadline unless the read limit is exceeded. The transport checks precede the status mapping of
[errors.md §6](errors.md#6-responses-without-a-valid-error-body):

| Status | Body | Local result |
|---|---|---|
| 502 | HTML page | `server_unavailable` |
| 413 | Truncated JSON (`{"code":`) in a complete HTTP body | `request_too_large` |
| 500 | HTML page | `broken_error_body` |
| 302 | HTML page; RPC routes allow no redirects | `redirect_not_allowed` |
| 502 | JSON body exceeds 65 536 bytes; read aborted | `server_unavailable` |
| 200 | JSON body exceeds the success-body read limit of 1 048 576 bytes | `response_too_large` |

A `502` response with a JSON body above 65 536 bytes when opening the event stream is also `server_unavailable`;
the client abandons the body at the same error-body limit.

For a chunk GET with `Range: bytes=0-0`, the success-body limit is one byte. A `401` response with
`Content-Type: application/json` and a valid error body (`code` `unauthenticated`, `ErrorInfo.reason` `TICKET_EXPIRED`)
is read under the 65 536-byte error-body limit even though it exceeds one byte. After the pre-check, the client
handles the code and reason: it refreshes the ticket and continues the download
([tickets.md §5](tickets.md#5-transport-of-tickets)).

## 13. Competing request checks

HTTP header blocks below use CRLF line endings and end with an empty line. Requests have no body unless stated.
The host, tokens and cookie values are synthetic. The first failing check decides the result.

### 13.1 Document authentication before path syntax

```http
GET /api/v2/descriptors/not-a-uuid HTTP/1.1
Host: reference.invalid

```

The server is running, no limit is reached and no authorization header is present. The common GET checks pass;
[api.md §8](api.md#8-document-and-object-routes) step 2 returns 401, `unauthenticated`, without a wire reason,
with `WWW-Authenticate: Bearer error="invalid_token"`. The server never checks `not-a-uuid`. With a valid access
token instead, step 4 returns 404, `not_found`, `VERSION_NOT_FOUND`. The same authentication-first rule applies
to a malformed manifest id or object id. If the server is starting, the unchanged raw request returns 503,
`unavailable`, `SERVER_STARTING`, before either authentication or path validation.

### 13.2 Depot method before ticket checks

```http
POST /depot/v2/0000000000000000000000000000000000000000000000000000000000000000/0000000000000000000000000000000000000000000000000000000000000000 HTTP/1.1
Host: reference.invalid
Content-Length: 0

```

The server is running. In [tickets.md §7.2](tickets.md#72-processing-order), step 0 passes and step 0a returns 405,
`unimplemented`, without a wire reason. The absent ticket is not checked. If the server is starting, the same
request returns 503, `unavailable`, `SERVER_STARTING`, at step 0; if only the method is changed to `GET` while the
server is running, step 1 returns 401, `unauthenticated`, without a wire reason.

### 13.3 Client preflight before connection or credential access

```json
{
  "purpose": "chunk",
  "requestUrl": "http://reference.invalid/depot/v2/0000000000000000000000000000000000000000000000000000000000000000/0000000000000000000000000000000000000000000000000000000000000000",
  "insecureMode": false,
  "platformTrustAvailable": false,
  "allowedEndpoints": []
}
```

The URL is valid. In [transport.md §2.1](transport.md#21-request-preflight-order), step 2 fails with
`platform_trust_unavailable`; neither the forbidden `http` scheme nor the empty endpoint set changes that result.
No connection is opened and no credential is read. If only `platformTrustAvailable` becomes true, step 3 returns
`insecure_scheme_not_allowed`. If `insecureMode` also becomes true, step 4 returns `origin_not_allowed`.

With a permitted server URL, an available verifier and no stop state, a new server connection is initially
unverified. All static preflight checks pass; the client opens the connection without credentials and runs discovery
on it. It reads no access token and generates no proof until the applicable checks of
[trust.md §4](trust.md#4-checks-before-every-credential-transfer) pass on that connection.

### 13.4 Content coding before an authentication RPC body

The following header block is followed by the one-byte body `00` (hex):

```http
POST /api/v2/impluvium.v2.AuthService/CompleteSession HTTP/1.1
Host: reference.invalid
Content-Type: application/proto
Content-Encoding: gzip
Content-Length: 1

```

Body:

```text
00
```

[proofs.md §1.3](proofs.md#13-common-request-checks) step 1 returns 501, `unimplemented`, without a wire reason.
The server does not decompress, scan the `00` tag or check a proof. With `Content-Encoding: identity` instead,
step 3 rejects tag zero as `malformed_wire`, reported as 400, `invalid_argument`, without a wire reason.
The same coding-before-body order applies to `BeginSession`, `StartPairing` and `PollPairing`.

### 13.5 Ambiguous stream credentials do not select an owner

```http
GET /api/v2/events?topics=library&client=a HTTP/1.1
Host: reference.invalid
Authorization: Bearer invalid
Cookie: session=valid
Origin: https://reference.invalid

```

The server is running; the bearer token is unknown, the cookie session is valid, the topic is permitted and the
client table has room. [events.md §1](events.md#1-opening-the-stream) step 1a returns 401, `unauthenticated`,
without a wire reason, before validating either credential. It creates no client entry and supersedes no stream.
The result is unchanged if both credentials are valid. Removing the authorization header permits cookie
authentication, whose supersession owner is the cookie session id; removing the cookie permits only bearer
authentication, which fails here because the token is unknown. A valid bearer-only request uses its device id,
not its token or the cookie session, as owner.

### 13.6 Other competing checks

| Request and context | First result | Why |
|---|---|---|
| `POST /api/info`, server running, address rate limit reached | 405, `unimplemented`, no wire reason | Common GET method check precedes the discovery rate check. |
| `POST /api/v2/events`, server running, no credential, no `topics` | 405, `unimplemented`, no wire reason | Common GET method check precedes stream authentication and parameters. |
| `GET` to an RPC method with `Content-Encoding: gzip`, no token | 501, `unimplemented`, no wire reason | The RPC method check precedes content coding and authentication. |
| `StartPairing` with identity coding, body `00` (hex), address rate limit reached | 429, `resource_exhausted`, `RATE_LIMITED` | The address limit precedes the body scan in proofs.md §1.3. |
| Document GET with a malformed id and two authorization fields | 401, `unauthenticated`, no wire reason | Header cardinality is checked before token lookup and path syntax. |
| Stream GET with two occurrences of the session cookie, even with equal values | 401, `unauthenticated`, no wire reason | Credential selection rejects ambiguity before authentication. |
| Valid stream GET with an existing supersession key and a full client table | 200; supersede the old stream and reuse its entry | No new table entry is needed. A bearer stream without `client` also needs no entry. |
| A sixth chunk redirect whose target also has another origin | `redirect_limit` | The redirect-count check precedes target checks and the repeated preflight. |

## 14. Aliases and descriptor context

### 14.1 JSON occurrence precedence

This `ServerEvent` input is checked at the JSON-profile stage, before semantic field validation:

```json
{"resyncRequired":{},"resync_required":null}
```

The first member occupies `payload` through `resync_required`. At the second member, the resolved names differ,
so [encoding.md §3.10](encoding.md#310-check-order) step 2 passes. Both names select the same `oneof` member;
step 3 returns `oneof_conflict`, including when the second value is `null`. It never reaches the value-type check
or the missing semantic fields of `ServerEvent`.

The following fragments illustrate only JSON-profile scanning; a scan success does not assert complete message
validity. Each result is determined while processing the input in document order:

| Input | Profile result | First failing check |
|---|---|---|
| `{"resync_required":null,"resyncRequired":{}}` in `ServerEvent` | `oneof_conflict` | Step 3; `null` still counts as a `oneof` occurrence. |
| `{"resyncRequired":{},"resyncRequired":null}` in `ServerEvent` | `duplicate_json_key` | Step 2, before the `oneof` check. |
| `{"resyncRequired":{},"resync\u0052equired":null}` in `ServerEvent` | `duplicate_json_key` | Step 2; escape resolution makes the names equal. |
| `{"resyncRequired":{},"session_ended":false}` in `ServerEvent` | `oneof_conflict` | Step 3, before checking the second member's value type. |
| `{"resyncRequired":false,"resync_required":{}}` in `ServerEvent` | `json_type_mismatch` | Step 4 of the first member, before the second name is read. |
| `{"deviceId":"x","device_id":null}` in `CompleteSessionRequest` | `duplicate_field` | Step 3; a non-`oneof` field under both names, even with `null`. |
| `{"keyFingerprints":[],"key_fingerprints":[]}` in `Discovery` | `duplicate_field` | Step 3; aliases of a repeated field are not concatenated. |
| `{"protocol":{},"protocol":{}}` in `Discovery` | `duplicate_json_key` | Step 2; repeating a message-valued JSON key does not use the binary `split_message` rule. |
| `{"scope":null,"scope":null}` in `CompleteSessionRequest` | `duplicate_json_key` | Step 2; the schema name and ProtoJSON name are identical. |
| `{"future":{"resyncRequired":{},"resync_required":null}}` in `ServerEvent` | scan passes | The nested names are unknown members, not aliases of schema fields. |

Within a JSON object encoding a `map<string, string>`, `{"deviceId":"a","device_id":"b"}` has two distinct keys;
they are not field aliases. `{"a":"x","\u0061":"y"}` instead fails step 2 with `duplicate_json_key`, because
both keys resolve to `a`; `duplicate_map_key` is the binary map-entry result.

### 14.2 Windows-target descriptor on every receiving host

Consider a structurally valid descriptor with one Windows launch option whose executable is `bin/game` and whose
working directory is empty. Its verified manifest has these entries, in byte order:

| Path | Type |
|---|---|
| `A` | `DIRECTORY` (empty) |
| `a` | `DIRECTORY` (empty) |
| `bin/game` | `REGULAR` (valid file fields) |

Signatures, identities, hashes, sizes and [descriptor.md §6](descriptor.md#6-context-checks) checks 0a through 8
all pass. At check 9, a receiving client uses the installation variant of the Windows profile on Linux, macOS and
Windows alike. `A` has collision key `a` (`upper(A) = A`, `fold(A) = a`); `a` has the same key
(`upper(a) = A`, `fold(A) = a`). The second directory therefore rejects the descriptor with
`windows_case_collision`, before `bin/game` is considered. Receiving it through a descriptor GET or `OpenDownload`
does not change this context result.

Related cases, with all unmentioned checks passing:

| Context | Result |
|---|---|
| The same manifest, but only a Linux launch option | Descriptor context passes on every receiving host; no Windows target activates check 9. Installing on Windows still requires its separate installation check. |
| The same manifest with a Windows setup step instead of the launch option | `windows_case_collision` on every receiving host; setup steps also activate check 9. |
| A Windows command with environment keys `Path` and `PATH` | `duplicate_env_name` on every host during structural validation; both keys fold to `path`. |
| A Linux command with the same two environment keys | The keys are distinct bytes and pass that structural check, even on a Windows receiving host. |
| A command naming `BIN/game` when the manifest contains only `bin/game` | `executable_not_found` on every host at context check 8; entry names are compared as exact bytes, even if a native file system would treat them as equal. |
| A Windows-target descriptor with an otherwise valid, collision-free manifest containing a symbolic link | Every receiving client rejects with `unsupported_symlink`; the publisher's publication variant instead reports `windows_symlink` without rejecting. |

Every host uses the Unicode 16.0.0 mappings of [paths.md §4.4](paths.md#44-collisions) for these checks, including
non-ASCII names; the host's file system, Unicode version and native ability to create links do not select the variant.

### 14.3 StartPairing address limit before absent fields

The header block below uses CRLF line endings and ends with an empty line. It is followed by exactly the two body
bytes `7b7d` (hex), namely `{}`, with no trailing newline:

```http
POST /api/v2/impluvium.v2.AuthService/StartPairing HTTP/1.1
Host: reference.invalid
Content-Type: application/json
Content-Length: 2

```

The client-address rate limit is reached. The method is `POST`, and the absent `Content-Encoding` means identity,
so [proofs.md §1.3](proofs.md#13-common-request-checks) step 1 passes. Step 2 returns 429, `resource_exhausted`,
`RATE_LIMITED`, before reading `{}` or checking its absent fields. No pairing is created. If the address limit is
not reached, step 3 accepts the body size and JSON profile, then
[proofs.md §3.1](proofs.md#31-startpairing) field check 1 rejects the absent `device_name` as `invalid_text`
(400, `invalid_argument`, without a wire reason).
