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
