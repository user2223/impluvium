<!--
SPDX-FileCopyrightText: 2026 the impluvium contributors
SPDX-License-Identifier: 0BSD
-->

# Limits

This chapter is the complete inventory of size, element and nesting limits. Every repeated field, every map, every
`string` and every `bytes` field of the schema has an entry with an element limit (for containers), a value limit per
element and an encoded budget. A schema change that adds such a field without an entry here is incomplete.

## 1. Principles

1. Limits are enforced **before** the allocation they bound: a verifier counts elements and bytes while it scans the
   input ([encoding.md §2.8](encoding.md#28-scan-order-and-precedence), [§3.10](encoding.md#310-check-order)) and rejects
   at the first element or byte that exceeds a limit. The same limits hold again for the decoded model.
2. A total size limit never replaces an element limit, and an element limit never replaces a value limit.
3. A length or count read from the input never causes a reservation of memory before it has been checked against the
   remaining input and the applicable limit.
4. Repeated fields count across all their wire segments, packed and unpacked together.
5. The **value limit** of a field bounds the decoded length of one value, identically in both codecs: the UTF-8 bytes of
   a string (in JSON after escape resolution), the bytes of a `bytes` value (in JSON after base64 decoding), and for a
   map entry its key and its value separately. A 128-byte value is allowed where the limit is 128, whatever its
   encoding costs.
6. The **encoded budget** of a field bounds the encoded bytes of all its occurrences in one message: in binary, tags,
   length prefixes, packed segments and the unknown fields inside message elements; in JSON, the member's value text
   with quotes, escapes and base64 text. "enclosing" means that the field has no budget of its own: its encoded bytes,
   including the framing of a value that is at its value limit, count only against the budget of the enclosing message
   and finally against the body, envelope or payload limit, which is checked before the scan. A named budget such as
   "manifest payload" or "response" is such an enclosing limit, stated explicitly.
7. **Semantic sums** (the command bytes of a descriptor, [descriptor.md §2](descriptor.md#2-limits-for-command-strings))
   add decoded value lengths only. They are checked after the scan and are independent of encoded budgets.
8. Unknown fields are skipped without being stored. They count against the encoded budget of the message that contains
   them and against the work a verifier performs, which stays linear in the input size.
9. Nesting depth is at most 64 ([encoding.md §2.6](encoding.md#26-nesting), [§3.9](encoding.md#39-limits)).
10. **Fixed-length fields** (UUID text, ids, keys, challenges and other fields whose table says "exactly n bytes") have a
    value limit of 128 bytes in the scan. Their exact length is a semantic rule of their chapter, so a value of the wrong
    length is reported with the semantic reason (for example `invalid_uuid`), not as `byte_limit_exceeded`. In the tables
    they are written "fixed n".
11. Exceeding an element limit is `element_limit_exceeded`; exceeding a value limit or an encoded budget is
    `byte_limit_exceeded`. Both carry the field name.
12. For a container of messages, the value limit "(none)" means that an element has no limit of its own; its encoded
    bytes count against the container's budget.

Names used for enclosing limits:

| Name | Value in bytes |
|---|---:|
| manifest payload | 536 870 912 |
| descriptor payload | 1 048 576 |
| small envelope (ticket, session proof, pairing proof) | 4 096 |
| envelope metadata | 16 384 |
| request | 65 536 (every RPC) |
| response | per service, §4 |
| discovery | 65 536 |
| event | 65 536 |

## 2. Envelope

| Message | Field | Elements | Value limit per element (bytes) | Encoded budget |
|---|---|---:|---:|---:|
| `Envelope` | whole envelope | – | – | per payload type: [envelope.md §2](envelope.md#2-payload-types) |
| `Envelope` | `payload_type` | – | 128 | enclosing |
| `Envelope` | `payload` | – | per payload type | enclosing |
| `Envelope` | `signatures` | 8 | (none) | envelope metadata (16 384) |
| `Envelope` | all fields except `payload` (metadata) | – | – | 16 384 |
| `Signature` | `key_id` | – | fixed 8 | enclosing |
| `Signature` | `sig` | – | fixed 64 | enclosing |

## 3. Signed documents

| Message | Field | Elements | Value limit per element (bytes) | Encoded budget |
|---|---|---:|---:|---:|
| `Manifest` | whole payload | – | – | 536 870 912 |
| `Manifest` | `game_id`, `version_id` | – | fixed 36 | enclosing |
| `Manifest` | `required_features` | 64 | 128 | manifest payload |
| `Manifest` | `entries` | 2 000 000 | (none) | manifest payload |
| `Manifest` | `chunks` | 4 000 000 | (none) | manifest payload |
| `Entry` | `path` | – | 4 096 | enclosing |
| `Entry` | `hash` | – | fixed 32 | enclosing |
| `Entry` | `symlink_target` | – | 4 096 | enclosing |
| `Entry` | `chunk_indexes` | 16 000 000 for all entries of one manifest together | varint | manifest payload |
| `Chunk` | `id` | – | fixed 32 | enclosing |
| `Chunk` | `size` (value) | – | 1 to 16 777 216 | – |
| `ReleaseDescriptor` | whole payload | – | – | 1 048 576 |
| `ReleaseDescriptor` | `game_id`, `version_id` | – | fixed 36 | enclosing |
| `ReleaseDescriptor` | `manifest_id` | – | fixed 32 | enclosing |
| `ReleaseDescriptor` | `requires` | 8 | fixed 36 | descriptor payload |
| `ReleaseDescriptor` | `launch_options` | 128 | (none) | descriptor payload |
| `ReleaseDescriptor` | `setup_steps` | 128 | (none) | descriptor payload |
| `ReleaseDescriptor` | `required_features` | 64 | 128 | descriptor payload |
| `ReleaseDescriptor` | command bytes ([descriptor.md §2](descriptor.md#2-limits-for-command-strings)) | – | – | 262 144 |
| `LaunchOption` | `key`, `name` | – | 128 | enclosing |
| `LaunchOption` | `hints` | 64 | key 128, value 4 096 | descriptor payload |
| `LaunchOption` | `legacy_command` | – | 4 096 | enclosing |
| `LaunchOption` | `legacy_launch_id` | – | 128 | enclosing |
| `Command` | `executable`, `working_dir` | – | 4 096 | enclosing |
| `Command` | `args` | 256 | 4 096 | descriptor payload |
| `Command` | `env` | 128 | key 128, value 4 096 | descriptor payload |
| `EmulatorRef` | `game_id` | – | fixed 36 | enclosing |
| `EmulatorRef` | `launch_key` | – | 128 | enclosing |
| `EmulatorRef` | `suggestions` | 32 | 256 | descriptor payload |
| `SetupStep` | `id` | – | 128 | enclosing |
| `SetupStep` | `legacy_command` | – | 4 096 | enclosing |
| `SetupStep` | `legacy_setup_id` | – | 128 | enclosing |
| `DepotTicketClaims` | whole envelope | – | – | 4 096 |
| `DepotTicketClaims` | `aud`, `sub`, `user_id` | – | fixed 36 | enclosing |
| `DepotTicketClaims` | `manifest_ids` | 8 | fixed 32 | small envelope |
| `SessionProof` | whole envelope | – | – | 4 096 |
| `SessionProof` | `instance_id`, `device_id` | – | fixed 36 | enclosing |
| `SessionProof` | `challenge` | – | fixed 32 | enclosing |
| `PairingProof` | whole envelope | – | – | 4 096 |
| `PairingProof` | `instance_id` | – | fixed 36 | enclosing |
| `PairingProof` | `pairing_id` | – | fixed 16 | enclosing |

## 4. RPC bodies

Body limits apply to the complete request or response body in wire bytes, in either codec, also when the body arrives
in chunks; the larger size of JSON counts. A request body above the limit is rejected before decoding with
`resource_exhausted` (`body_too_large`); a success response body above the response limit is abandoned by the client
(`response_too_large`). The response column below applies to success bodies; error bodies use the limit in §5.

| Service | Methods | Request | Response |
|---|---|---:|---:|
| `AuthService` | `StartPairing`, `PollPairing`, `BeginSession`, `CompleteSession`, `RevokeSelf` | 65 536 | 65 536 |
| `DeviceService` | `ListDevices`, `RevokeDevice` | 65 536 | 1 048 576 |
| `LibraryService` | `ListLibrary`, `GetGame`, `ListVersions`, `GetVersion` | 65 536 | 4 194 304 |
| `DownloadService` | `OpenDownload`, `RefreshTicket` | 65 536 | 2 097 152 |

Servers and the reverse proxies in front of them configure their own request limits to at least these values for the
respective paths, so that an oversized request is answered by the service and not cut off by the proxy.

Field inventory of the RPC messages ("request" and "response" mean the body limit of the message's method):

| Message | Field | Elements | Value limit per element (bytes) | Encoded budget |
|---|---|---:|---:|---:|
| `StartPairingRequest` | `device_name` | – | 128 | enclosing |
| `StartPairingRequest` | `platform`, `arch` | – | 32 | enclosing |
| `StartPairingRequest` | `public_key` | – | fixed 32 | enclosing |
| `StartPairingRequest` | `client_version` | – | 64 | enclosing |
| `StartPairingResponse` | `pairing_id` | – | fixed 16 | enclosing |
| `StartPairingResponse` | `user_code` | – | fixed 9 | enclosing |
| `StartPairingResponse` | `verification_uri`, `verification_uri_complete` | – | 2 048 | enclosing |
| `PollPairingRequest` | `pairing_id` | – | fixed 16 | enclosing |
| `PollPairingRequest` | `proof` | – | 4 096 | enclosing |
| `PollPairingResponse` | `device_id`, `instance_id` | – | fixed 36 | enclosing |
| `PollPairingResponse` | `content_public_key` | – | fixed 32 | enclosing |
| `BeginSessionRequest` | `device_id` | – | fixed 36 | enclosing |
| `BeginSessionResponse` | `challenge` | – | fixed 32 | enclosing |
| `CompleteSessionRequest` | `device_id` | – | fixed 36 | enclosing |
| `CompleteSessionRequest` | `challenge` | – | fixed 32 | enclosing |
| `CompleteSessionRequest` | `proof` | – | 4 096 | enclosing |
| `CompleteSessionRequest` | `client_capabilities` | 64 | 128 | request |
| `CompleteSessionResponse` | `access_token` | – | fixed 43 | enclosing |
| `CompleteSessionResponse` | `accepted_capabilities` | 64 | 128 | response |
| `ListDevicesRequest` | `page_token` | – | 1 024 | enclosing |
| `ListDevicesResponse` | `devices` | 200 | (none) | response |
| `ListDevicesResponse` | `next_page_token` | – | 1 024 | enclosing |
| `Device` | `device_id` | – | fixed 36 | enclosing |
| `Device` | `name` | – | 128 | enclosing |
| `Device` | `platform`, `arch` | – | 32 | enclosing |
| `Device` | `client_version` | – | 64 | enclosing |
| `RevokeDeviceRequest` | `device_id` | – | fixed 36 | enclosing |
| `ListLibraryRequest` | `page_token` | – | 1 024 | enclosing |
| `ListLibraryResponse` | `games` | 200 | (none) | response |
| `ListLibraryResponse` | `next_page_token` | – | 1 024 | enclosing |
| `ListLibraryResponse` | `epoch` | – | fixed 16 | enclosing |
| `GameSummary` | `game_id` | – | fixed 36 | enclosing |
| `GameSummary` | `name` | – | 256 | enclosing |
| `GameSummary` | `platforms` | 8 | varint | response (in events: event) |
| `GameSummary` | `icon_object_id`, `cover_object_id` | – | empty or fixed 36 | enclosing |
| `GetGameRequest` | `game_id` | – | fixed 36 | enclosing |
| `ListVersionsRequest` | `game_id` | – | fixed 36 | enclosing |
| `ListVersionsRequest` | `page_token` | – | 1 024 | enclosing |
| `ListVersionsResponse` | `versions` | 200 | (none) | response |
| `ListVersionsResponse` | `next_page_token` | – | 1 024 | enclosing |
| `VersionSummary` | `version_id`, `game_id` | – | fixed 36 | enclosing |
| `VersionSummary` | `display_name` | – | 256 | enclosing |
| `VersionSummary` | `platforms` | 8 | varint | response |
| `VersionSummary` | `manifest_id` | – | fixed 32 | enclosing |
| `GetVersionRequest` | `version_id` | – | fixed 36 | enclosing |
| `OpenDownloadRequest` | `version_id` | – | fixed 36 | enclosing |
| `OpenDownloadResponse` | `descriptor` | – | 1 064 960 | enclosing |
| `OpenDownloadResponse` | `manifest_ids` | 8 | fixed 32 | response |
| `OpenDownloadResponse` | `ticket` | – | 4 096 | enclosing |
| `OpenDownloadResponse` | `depot_endpoints` | 8 | (none) | response |
| `DepotEndpoint` | `base_url` | – | 2 048 | enclosing |
| `RefreshTicketRequest` | `version_id` | – | fixed 36 | enclosing |
| `RefreshTicketResponse` | `ticket` | – | 4 096 | enclosing |
| `RefreshTicketResponse` | `manifest_ids` | 8 | fixed 32 | response |
| `ErrorInfo` | `metadata` | 32 | key 128, value 1 024 | decoded detail value (8 192) |

`RevokeSelfRequest`, `RevokeSelfResponse`, `RevokeDeviceResponse`, `GetGameResponse`, `GetVersionResponse` and
`ProtocolVersion` contain no repeated, map, `string` or `bytes` field of their own; their message-typed fields are bounded
by the response limit.

## 5. HTTP bodies outside RPC

| Body | Limit |
|---|---:|
| Discovery response (`Discovery` as ProtoJSON) | 65 536 |
| Manifest envelope (`GET …/manifests/…`) | 536 887 296 (536 870 912 + 16 384) |
| Descriptor envelope (`GET …/descriptors/…`) | 1 064 960 (1 048 576 + 16 384) |
| Object (`GET …/objects/…`) | chosen by the caller for the object's purpose |
| Chunk (`GET /depot/v2/…`) | the chunk's size from the manifest (at most 16 777 216), or the requested range length |
| Error body (all surfaces, including event-stream open) | 65 536 |
| One event of the event stream, all its lines | 65 536 |
| Response header section | 65 536 |
| `Authorization` header value carrying a ticket | 5 469 |
| Event stream resume point (`last_event_id`, `Last-Event-ID`) | 53 |

The error-body limit here and in §6 is the read limit for every response whose status is not the expected success
status of the requested route or method. It applies when opening the event stream and to chunk and object GETs,
independently of their success-body limits, even when those limits are smaller. Exceeding it aborts the read and
fails the body-size check of [errors.md §3.2](errors.md#32-pre-check-of-error-bodies); the status mapping of
[errors.md §6](errors.md#6-responses-without-a-valid-error-body) applies.

Discovery field inventory:

| Message | Field | Elements | Value limit per element (bytes) | Encoded budget |
|---|---|---:|---:|---:|
| `Discovery` | `product` | – | 128 | enclosing |
| `Discovery` | `server_name` | – | 128 | enclosing |
| `Discovery` | `server_version` | – | 64 | enclosing |
| `Discovery` | `instance_id` | – | fixed 36 | enclosing |
| `Discovery` | `manifest_formats` | 8 | number | discovery |
| `Discovery` | `capabilities` | 64 | 128 | discovery |
| `Discovery` | `required_client_capabilities` | 64 | 128 | discovery |
| `Discovery` | `key_fingerprints` | 16 | (none) | discovery |
| `Discovery` | `commit` | – | 64 | enclosing |
| `Discovery` | `source_url` | – | 2 048 | enclosing |
| `KeyFingerprint` | `sha256` | – | fixed 32 | enclosing |

Event field inventory:

| Message | Field | Elements | Value limit per element (bytes) | Encoded budget |
|---|---|---:|---:|---:|
| `ServerEvent` | `epoch` | – | fixed 16 | enclosing |
| `LibraryChanged` | `game_id` | – | fixed 36 | enclosing |
| `LibraryChanged` | `game` | – | `GameSummary` as in §4 | event |

## 6. Error bodies

| Element | Limit |
|---|---:|
| Body | 65 536 bytes |
| `message` | 4 096 bytes |
| `details` | 16 elements; all together within the body |
| Detail `type` | 256 bytes |
| Detail `value`, decoded | 8 192 bytes |
| `ErrorInfo.metadata` | 32 entries; key 128 bytes, value 1 024 bytes; all within the decoded detail value |

## 7. Values that are not byte limits

| Value | Limit | Defined in |
|---|---|---|
| Page size | default 50, at most 200 | [api.md §7](api.md#7-pagination) |
| Page token validity | 3 600 s | [api.md §7](api.md#7-pagination) |
| Depot redirects | 5, same origin and allowed endpoint | [transport.md §7](transport.md#7-redirects) |
| Ticket lifetime | 3 600 s maximum, 300 s configurable minimum | [tickets.md §2](tickets.md#2-issuance) |
| Challenge lifetime | 60 s | [proofs.md §2.1](proofs.md#21-beginsession) |
| Pairing lifetime | 600 s | [proofs.md §3.1](proofs.md#31-startpairing) |
| Access token lifetime | 15 min default | [proofs.md §2.3](proofs.md#23-access-tokens) |
| Device idle limit | 15 552 000 s (180 days) default | [proofs.md §2.4](proofs.md#24-token-use-and-device-state) |
| Depot-ticket acceptance set | 1 to 8 keys | [keys.md §4](keys.md#4-depot-ticket-key-generations) |
| Manifests per ticket and per dependency closure | 8 | [tickets.md](tickets.md), [descriptor.md §7](descriptor.md#7-publication-checks) |
| Event topics per stream | 8 | [events.md §1](events.md#1-opening-the-stream) |
| Client identifiers per user | 256 | [events.md §9](events.md#9-supersession) |
| `retry_after_seconds`, `Retry-After` | 86 400 s | [errors.md](errors.md) |

## 8. Decoder resources (informative)

Implementations configure the memory budget of their Protocol Buffers runtime so that the largest valid manifest decodes;
this is an implementation profile, not a protocol limit, and it never replaces the pre-decode limits above. The
reference measurements for decoding time and peak memory of large valid manifests are published with the benchmark
material.
