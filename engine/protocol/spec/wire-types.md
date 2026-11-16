<!--
SPDX-FileCopyrightText: 2026 the impluvium contributors
SPDX-License-Identifier: 0BSD
-->

# Wire type inventory

This chapter lists every Protocol Buffers type that the schema uses, the fields that use it, its accepted encodings and
the vector classes that every implementation and the reference verifier must pass for it. The base set is deliberately
small. Adding a type that is not listed here (a new scalar kind, a new well-known type, a new map key type) is a schema
change that MUST add a row here and the corresponding vectors in the same change.

## 1. Types that are not used

The schema of this revision does not use the following. A schema change that introduces one of them is incomplete until
this inventory and its vectors are extended:

- `sint32`, `sint64`, `fixed32`, `fixed64`, `sfixed32`, `sfixed64`, `float`, `double`, and `int32`/`int64` outside
  `google.protobuf.Timestamp`;
- `google.protobuf.Duration`, `Any`, `Struct`, `Value`, `ListValue`, `FieldMask`, `Empty` and the wrapper types;
- map keys other than `string`, maps with values other than `string`;
- proto2 syntax, groups, extensions, `required`/`optional` labels in proto2 sense, and proto3 `optional` scalars.

Wire types 1 (I64) and 5 (I32) therefore only occur in unknown fields.

## 2. Scalar and structural types

| Type | Wire type | JSON form | Fields |
|---|---|---|---|
| `uint32` | 0 (VARINT) | number (string accepted) | `Manifest.format`, `Entry.mode`, `Discovery.discovery`, `ProtocolVersion.major`, `ProtocolVersion.revision`, `ProtocolVersion.min_client_revision`, `StartPairingResponse.interval_seconds`, `ListDevicesRequest.page_size`, `ListLibraryRequest.page_size`, `ListVersionsRequest.page_size`, `DepotEndpoint.priority`, `ErrorInfo.retry_after_seconds` |
| repeated `uint32` | 0 (unpacked) or 2 (packed) | array of numbers | `Entry.chunk_indexes`, `Discovery.manifest_formats` |
| `uint64` | 0 (VARINT) | string (number accepted) | `Manifest.total_size`, `Chunking.min_size`, `Chunking.avg_size`, `Chunking.max_size`, `Entry.size`, `Chunk.size`, `ReleaseDescriptor.revision`, `ReleaseDescriptor.manifest_size`, `ReleaseDescriptor.install_size`, `ListLibraryResponse.snapshot_seq`, `GameSummary.entry_version`, `VersionSummary.install_size`, `VersionSummary.manifest_size`, `VersionSummary.descriptor_revision`, `ServerEvent.seq`, `ServerEvent.prev_seq`, `Connected.head_seq` |
| `bool` | 0 (VARINT); accepted: 0 is `false`, every other value is `true`; producers write only 1 (and omit `false`) | `true`/`false` | `SetupStep.idempotent`, `Device.current`, `Connected.resumed`, `LibraryChanged.deleted` |
| enum | 0 (VARINT) | name string (number accepted) | singular: `Header.hash_algorithm`, `Chunking.algorithm`, `Entry.type`, `ReleaseDescriptor.kind`, `LaunchOption.platform`, `SetupStep.platform`, `KeyFingerprint.role`, `ErrorInfo.reason`, `CompleteSessionRequest.scope`, `CompleteSessionResponse.scope`, `VersionSummary.kind`, `ResyncRequired.reason`, `SessionEnded.reason` |
| repeated enum | 0 (unpacked) or 2 (packed) | array of name strings | `GameSummary.platforms`, `VersionSummary.platforms` |
| `string` | 2 (LEN) | string | all `string` fields, including repeated ones and map keys and values |
| `bytes` | 2 (LEN) | base64 string | all `bytes` fields, including repeated ones |
| message | 2 (LEN) | object | all message-typed fields |
| `map<string, string>` | 2 (LEN), one entry message per pair: key field 1, value field 2 | object | `LaunchOption.hints`, `Command.env`, `ErrorInfo.metadata` |
| `oneof` | wire type of the member | member under its own name | `ServerEvent.payload` |
| `google.protobuf.Timestamp` | 2 (LEN); inside: field 1 `seconds` `int64` VARINT, field 2 `nanos` `int32` VARINT | RFC 3339 string | `Manifest.created_at`, `DepotTicketClaims.iat`, `DepotTicketClaims.exp`, `StartPairingResponse.expires_at`, `BeginSessionResponse.expires_at`, `CompleteSessionResponse.expires_at`, `Device.created_at`, `Device.last_seen_at`, `Device.revoked_at`, `ServerEvent.time` |

The two signed-document families use only the binary form: `Envelope`, `Manifest`, `ReleaseDescriptor`,
`DepotTicketClaims`, `SessionProof` and `PairingProof` are never accepted as JSON. Inside a JSON RPC body, an embedded
envelope is a `bytes` field and therefore a base64 string of the binary envelope.

## 3. Required vector classes

Each class below MUST have binary vectors and, where the type occurs in a JSON-capable message, JSON vectors. Each
class has positive vectors (accepted, with the expected decoded value) and negative vectors (rejected, with the expected
local reason). The column "Cross-codec" requires vectors that are written in one codec and read in the other.

| Type | Positive cases | Negative cases | Cross-codec |
|---|---|---|---|
| `uint32` | 0 (absent), 1, 4 294 967 295; a binary varint of 2^32 + 5, read as 5 (conversion of §2.2 of encoding.md); JSON number and string forms; JSON `1e2`, `"1E2"`, `100.0`, `-0` (all standard ProtoJSON integer forms) | 11-byte varint; JSON `-1` (`integer_out_of_range`), `1.5`, `""`, `" 1"`, `"01"` (`invalid_number`) | yes |
| repeated `uint32` | packed, unpacked, mixed segments in wire order; empty | truncated packed varint; element limit + 1 across segments | yes |
| `uint64` | 0, 2^53 − 1, 2^53, 2^53 + 1, 18 446 744 073 709 551 615; JSON string and number forms, including `"1.8446744073709551615e19"` (evaluated exactly) | 2^64 as JSON string; 10-byte varint with last byte `0x02`; JSON `"1e20"` (`integer_out_of_range`), `"1.5"` (`invalid_number`) | yes |
| `bool` | `false` (absent), `true`; binary varint 2, read as `true` | JSON `1`, `"true"` (`json_type_mismatch`) | yes |
| enum | every listed value; unknown number, including a negative one and a binary varint above 2^31 − 1 (low 32 bits as two's complement); unknown JSON name in singular field (reads as 0) and in repeated field (element removed) | JSON `true`, `{}`, `[]` as value or as list element (`json_type_mismatch`); JSON number 2^31 (`integer_out_of_range`) | yes |
| `string` | empty, ASCII, multi-byte UTF-8 at the byte limit | limit + 1 byte; overlong UTF-8; encoded surrogate; JSON unpaired `\ud800` | yes |
| `bytes` | empty, length 1, length at the field limit; JSON with and without padding, both alphabets, non-zero trailing bits (ignored) | wrong fixed length (where fixed); mixed alphabets; whitespace or line break inside the value; incomplete padding | yes |
| message | present with all fields default; absent | duplicate occurrence outside a `oneof` (`split_message` in binary; JSON name rules below); wrong wire type | yes |
| `map<string, string>` | empty; one entry; entry without key (key `""`, then key rules) | duplicate key (`duplicate_map_key` in binary, `duplicate_json_key` in JSON); key limit + 1; entry count limit + 1 | yes |
| `oneof` | each member alone, including a member with all fields default | two members; same member twice; one member plus a `null` member in JSON | yes |
| `Timestamp` | minimum, maximum, maximum with `nanos` 999 999 999; JSON with `Z`, with lowercase `t` and `z`, with a `+01:00` offset | `seconds` 253 402 300 800 in binary; `nanos` −1 and 1 000 000 000; JSON year 10000, second 60, 10 fraction digits, missing offset | yes |
| unknown field | each accepted wire type 0, 1, 2, 5 in each message; many unknown fields up to the body limit | wire type 3, 4, 6, 7; field number 0; truncated length | binary only |
| JSON names | `snake_case` alone; `lowerCamelCase` alone | both names of a non-`oneof` field (`duplicate_field`); aliases of one `oneof` member (`oneof_conflict`); duplicate resolved key (`duplicate_json_key`), also after escape resolution | JSON only |
| typed error detail | `ErrorInfo` in an error body with every listed reason and an unknown reason | detail value above 8 KiB decoded; second `ErrorInfo` detail | JSON error body |

Signed-document byte vectors (PAE, signatures, `manifest_id`, `key_id`, golden manifests and descriptors, tickets,
proofs) are described in their chapters and compared byte for byte. General messages are compared semantically where
the encoding leaves an order open (map entries).
