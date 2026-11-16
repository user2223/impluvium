<!--
SPDX-FileCopyrightText: 2026 the impluvium contributors
SPDX-License-Identifier: 0BSD
-->

# Encoding profile

This chapter defines which binary and JSON encodings of the schema a verifier accepts. The profile is an explicit
acceptance restriction on top of the Protocol Buffers encodings: a general-purpose decoder on its own is not a
conforming verifier, because it merges, overwrites or silently drops input that this profile rejects.

## 1. Where the profile applies

The profile applies to every protected input:

- the binary `Envelope` and every signed payload ([envelope.md](envelope.md));
- RPC request and response bodies in both codecs ([api.md](api.md));
- the discovery document, event data ([events.md](events.md)) and error bodies ([errors.md](errors.md));
- error detail messages carried inside error bodies.

Verifiers apply the profile **before** a general-purpose decoder sees the input, and they apply the size and element
limits of [limits.md](limits.md) during the same pass. The decoded model is checked again against the semantic rules of
the relevant chapter. Signature verification, `manifest_id` and stored baselines always use the original received bytes
([envelope.md §6](envelope.md#6-exact-bytes)).

## 2. Binary profile

### 2.1 Tags and wire types

1. A tag is a varint `(field_number << 3) | wire_type`. Field number 0 and field numbers above 536 870 911 are rejected
   (`malformed_wire`).
2. Accepted wire types are 0 (VARINT), 1 (I64), 2 (LEN) and 5 (I32). Wire types 3 and 4 (groups) and 6 and 7 are
   rejected (`malformed_wire`), also inside unknown fields.
3. A **known** field (one that the schema defines for the message being read) MUST use the wire type listed for it in
   [wire-types.md §2](wire-types.md#2-scalar-and-structural-types). A known field with another wire type is rejected
   (`wrong_wire_type`); it is never treated as unknown. The only alternative encoding that is accepted is the packed
   (LEN) or unpacked (VARINT) form of a repeated `uint32` or repeated enum field.

### 2.2 Varints and integers

1. A varint has at most 10 bytes. A 10-byte varint whose last byte is greater than `0x01` is rejected
   (`malformed_wire`). A varint that runs past the end of its enclosing bytes is rejected (`malformed_wire`).
2. Non-minimal varints (extra `0x80` continuation bytes) are accepted within the 10-byte limit.
3. Varint values are converted to the field type as the Protocol Buffers encoding specifies: a `uint32` field takes
   the value modulo 2^32; an enum field and `Timestamp.nanos` (`int32`) take the low 32 bits as a two's-complement
   value; `Timestamp.seconds` (`int64`) takes the 64 bits as a two's-complement value; a `bool` field is `true` for every
   non-zero value. Producers never write values that need this conversion (§2.7).
4. An enum value that the schema does not list, including a negative one, is an *unknown enum value* (§4).
5. The converted value is the value that all later rules see; for example a `Timestamp.nanos` that converts to a
   negative number fails the range check of §5.

### 2.3 Length-delimited fields

1. A LEN field consists of a varint length followed by exactly that many bytes. A length larger than the remaining bytes
   of the enclosing message is rejected (`malformed_wire`).
2. A message ends exactly at the end of its bytes. Truncated fields are rejected (`malformed_wire`).
3. `string` fields MUST contain well-formed UTF-8 (RFC 3629: no overlong forms, no surrogate code points, no code
   point above U+10FFFF). Otherwise `invalid_utf8`. This includes map keys and values.
4. The packed form of a repeated `uint32` or enum field is a LEN field whose content is a sequence of complete varints.
   A packed body that ends inside a varint is rejected (`malformed_wire`).

### 2.4 Occurrence rules

| Wire situation | Rule | Reason |
|---|---|---|
| A known singular scalar, string or bytes field occurs more than once, even with equal values | Reject. There is no "last one wins". This includes `Envelope.payload_type`, `Envelope.payload`, `Signature.key_id` and `Signature.sig`. | `duplicate_field` |
| A known singular message field occurs more than once, even if the parts set disjoint fields | Reject. Parts are never merged. | `split_message` |
| Members of one `oneof` occur more than once in total, whether the same member twice or two different members, including default values | Reject. | `oneof_conflict` |
| A repeated field occurs in several segments, in any mix of packed and unpacked form where both are allowed | Concatenate in wire order. All segments share one element counter and one encoded budget. | – |
| A map contains the same key in two entries | Reject before inserting. Never overwrite. | `duplicate_map_key` |
| A map entry contains its key or value field more than once | Reject. | `duplicate_field` |
| A map entry has no key field | The key is the empty string; the key rules of the map then apply. | – |
| An unknown field | Validate tag, wire type and length; skip it without interpreting its content; do not store it. | – |

An occurrence counts regardless of its value: an explicitly encoded default value is an occurrence. For a field that is
a member of a `oneof`, only the `oneof` rule applies: a second occurrence of the same message-typed member is
`oneof_conflict`, not `split_message`.

### 2.5 Unknown fields

Unknown fields are accepted in every message so that later revisions can add optional fields. They are skipped without
allocation proportional to their count, they are never stored, never re-emitted and never re-signed, and their bytes
count against the same input, work and size limits as known bytes. A LEN unknown field is skipped as opaque bytes; its
content is not parsed and does not count towards nesting depth. An unknown field can never switch on behaviour: new
mandatory behaviour is announced through `required_features` or a new payload type.

### 2.6 Nesting

The depth of a message is 1 for the top-level message and increases by 1 for each known message-typed field entered,
including map entries and `google.protobuf.Timestamp`. A depth above 64 is rejected (`nesting_too_deep`). The schema of
this revision does not exceed depth 4 (for example `ReleaseDescriptor` → `LaunchOption` → `Command` → an `env` map
entry), so this limit only matters for later revisions and for JSON (§3.9).

### 2.7 Producer encoding

Producers of binary messages MUST emit:

- known fields in ascending field-number order;
- no field whose value equals the proto3 default for a field without explicit presence (zero, empty string, empty
  bytes, `false`, enum value 0, empty repeated field, empty map);
- `true` as the varint 1, and integer values within the range of the field type;
- message-typed fields whenever they are present, even if all their fields are default;
- repeated `uint32` and repeated enum fields in packed form, as a single LEN field;
- no unknown fields.

Signed payloads additionally follow the canonical producer rules of their chapter (for example
[manifest.md §8](manifest.md#8-producer-rules-and-determinism)). Verifiers do not require the producer encoding: they
accept every encoding that §2.1 to §2.6 allow, because signatures cover the received bytes.

### 2.8 Scan order and precedence

Before the scan, the whole input is checked against its total limit (body, envelope or payload limit of
[limits.md](limits.md)). The scan then reads the fields of a message in wire order. For every field occurrence it
applies the following checks in this order; the first failing check of the first failing occurrence determines the
reason.

| Order | Check | Reason |
|---:|---|---|
| 1 | The tag is a well-formed varint, the field number is valid and the wire type is 0, 1, 2 or 5 (§2.1) | `malformed_wire` |
| 2 | A known field uses an allowed wire type (§2.1) | `wrong_wire_type` |
| 3 | The value is framed correctly: a well-formed varint, a length prefix that fits into the remaining bytes, or 8 or 4 remaining bytes (§2.2, §2.3) | `malformed_wire` |
| 4 | Occurrence rules (§2.4): the `oneof` rule for `oneof` members, otherwise the singular-field rule | `oneof_conflict`, `duplicate_field`, `split_message` |
| 5 | Limits of the field ([limits.md](limits.md)): element count across all segments, value limit of the decoded value, encoded budget of the field and of the enclosing message | `element_limit_exceeded`, `byte_limit_exceeded` |
| 6 | Value content: UTF-8 of strings (§2.3); for a packed body, each element in order with step 5 and this step for that element | `invalid_utf8`, `malformed_wire` |
| 7 | A message-typed value: depth (§2.6), then the embedded message is scanned completely at this point with the same procedure; a map entry is checked for a duplicate key when it is complete; a `Timestamp` is range-checked (§5) when it is complete | `nesting_too_deep`, reasons of the embedded scan, `duplicate_map_key`, `invalid_timestamp` |

Unknown fields pass steps 1 and 3 and count against the limits of the enclosing message. Rules about required fields,
lengths of fixed-size values, text syntax and cross-field relations are not part of the scan; they run afterwards in
the order of §7 or of the chapter that defines the message.

Diagnostic fields follow [errors.md §5](errors.md#5-local-reason-catalog). A complete tag with a known field number
identifies that field, including a forbidden-wire-type failure at step 1; an undecodable tag or an unknown, invalid
or zero field number identifies `""`. After a known tag, framing, packed-element and value failures identify the
known field. On entering a nested message, its own schema applies: an unidentifiable inner tag does not inherit the
outer field. Map-entry key and value checks identify the enclosing map field. A whole-input limit identifies `""`;
entering a known message field beyond the depth limit identifies that field.

## 3. JSON profile

The JSON profile applies to RPC bodies sent as `application/json`, to the discovery document and to event data. The
schema-to-JSON mapping is the standard Protocol Buffers JSON mapping (ProtoJSON). The profile accepts every ProtoJSON
input form and adds exactly these restrictions: the ambiguity rules of §3.1 to §3.3, the value-type rule of §3.4 for
enum containers, the timestamp range of §5 and the limits of §3.9. §3.5 to §3.7 state how the standard forms are read,
so that every implementation reads them the same way.

### 3.1 Document

1. Input MUST be UTF-8 JSON text (RFC 8259) whose top-level value is one object. A byte-order mark, invalid UTF-8,
   unpaired surrogate escapes (`\ud800` without a following low surrogate) and non-whitespace after the top-level value
   are rejected (`invalid_json`).
2. Object member names are compared after escape resolution. Two members with the same resolved name in one object are
   rejected (`duplicate_json_key`), also in unknown members and nested values.

### 3.2 Field names

1. A known field is accepted under exactly two names: its schema name (`snake_case`) and its ProtoJSON name
   (`lowerCamelCase`); these names may be identical. Any other spelling is an unknown member.
2. After the duplicate-JSON-key check (§3.10 step 2), a non-`oneof` field under both distinct names is rejected
   (`duplicate_field`), whatever the values. For a `oneof` member, only the `oneof` occurrence rule of §3.3 applies.
3. Unknown members are ignored after their value has been checked against §3.1 and the limits of §3.9.

### 3.3 Null

`null` as the value of a known field means "absent". `null` as an element of an array or as a map value is rejected
(`json_type_mismatch`). Two members of one `oneof`, including members with value `null` or a default value, are rejected
(`oneof_conflict`).

This includes the same `oneof` member under both accepted field names: it is `oneof_conflict`, not `duplicate_field`.
A repeated resolved JSON key fails earlier (§3.10 step 2: `duplicate_json_key`), also for a `oneof` member.

### 3.4 Value types

| Schema type | Accepted JSON value | Rule |
|---|---|---|
| `string` | string | – |
| `bytes` | string | §3.6 |
| `uint32` | number or string | §3.5, range 0 to 4 294 967 295 |
| `uint64` | string or number | §3.5, range 0 to 18 446 744 073 709 551 615 |
| `bool` | `true` or `false` | – |
| enum | string or number | §4; a number follows §3.5 with range −2 147 483 648 to 2 147 483 647 |
| message | object | – |
| `google.protobuf.Timestamp` | string | §3.7 |
| repeated field | array | each element follows the element type |
| map | object | member names are the keys; values follow the value type |

Any other JSON type is rejected (`json_type_mismatch`). This applies to every element of a repeated field and every map
value, including enum containers: a `true`, an object or an array inside a list of enum values is rejected, never
dropped.

### 3.5 Integers

As in ProtoJSON, an integer field accepts a JSON number or a JSON string. In both forms the text (the number literal,
or the complete content of the string) follows the JSON number grammar of RFC 8259 §6, so a sign, a fraction and an
exponent are allowed and surrounding whitespace inside a string is not. The value the text denotes MUST be an exact
integer: `100`, `"100"`, `1e2`, `"1E2"`, `100.0` and `-0` are accepted, `1.5` and `""` are not (`invalid_number`). An
integer outside the range of the field type is rejected (`integer_out_of_range`). The value is evaluated exactly from
the decimal text, never through a floating-point type; an exponent is evaluated without materialising the digits it
implies.

### 3.6 Bytes

As in ProtoJSON, a `bytes` value is a base64 string (RFC 4648) in the standard alphabet (`+`, `/`) or the URL-safe
alphabet (`-`, `_`), with or without padding. One value uses one alphabet. Following RFC 4648 §3.3, a character outside
the alphabet, including whitespace and line breaks, makes the value invalid; padding, if present, is complete
(`invalid_base64`). Bits of the last character beyond the encoded bytes are ignored. The decoded length counts against
the value limit of the field; the base64 text counts against the encoded budgets ([limits.md §1](limits.md#1-principles)).

### 3.7 Timestamps

As in ProtoJSON, a timestamp is an RFC 3339 `date-time` string: `YYYY-MM-DD`, the separator `T` or `t`, `hh:mm:ss`, an
optional fraction of 1 to 9 digits, and the offset `Z`, `z` or `±hh:mm`. The value is normalised to UTC and then MUST lie
within §5. A second value of 60, more than 9 fraction digits, a missing offset and any other form are rejected
(`invalid_timestamp`), because a `Timestamp` cannot represent them.

### 3.8 Producer encoding

Producers of JSON MUST emit the `lowerCamelCase` names, `uint64` values as strings, `uint32` values as numbers, `bytes`
in the standard alphabet with padding, enum values by name, timestamps with offset `Z` and 0, 3, 6 or 9 fraction
digits, and they MUST omit fields whose value is the proto3 default for a field without explicit presence.

### 3.9 Limits

A JSON value nested deeper than 64 levels (each object or array opens one level; the top-level object is level 1) is
rejected (`nesting_too_deep`), including inside unknown members. Element limits and value limits of [limits.md](limits.md) apply to
the decoded values: strings count with their decoded UTF-8 length, `bytes` with their decoded length. Quotes, escapes
and base64 text count only against the encoded budgets. Duplicate-key
detection uses bounded memory that counts against the body budget. The checks run on a bounded token stream; building
an unbounded generic JSON tree before them is not a conforming pre-check.

### 3.10 Check order

Before reading, the body is checked against its total limit. Members are then processed in document order, and nested
values depth-first at the position of their member. For each member the checks run in this order; the first failing
check determines the reason.

| Order | Check | Reason |
|---:|---|---|
| 1 | JSON syntax of the name and of each value token as it is read (§3.1) | `invalid_json` |
| 2 | The resolved name does not occur earlier in the same object | `duplicate_json_key` |
| 3 | For a known `oneof` member: no member of the same `oneof` occurs earlier (§3.3); for any other known field: its other name does not occur earlier (§3.2) | `oneof_conflict`, `duplicate_field`, respectively |
| 4 | The JSON type of the value fits the field type (§3.4); `null` only as a whole-field value (§3.3) | `json_type_mismatch` |
| 5 | Opening an object or array does not exceed the depth limit (§3.9) | `nesting_too_deep` |
| 6 | Element count and byte length of the field ([limits.md](limits.md)) | `element_limit_exceeded`, `byte_limit_exceeded` |
| 7 | Value content: integers (§3.5), bytes (§3.6), timestamps (§3.7 and §5), UTF-8 of strings | `invalid_number`, `integer_out_of_range`, `invalid_base64`, `invalid_timestamp` |

Unknown members pass steps 1, 2 and 5 and count against the body limit. Array elements and map members are checked in
document order with steps 4 to 7 for each element. For a map member, first apply steps 1 and 2 to its name. Map keys
are strings, not schema field aliases: equal resolved keys are `duplicate_json_key`, not the binary map-entry reason
`duplicate_map_key`.

For diagnostics, known JSON field names are translated back to their schema `Message.field`, including aliases and
repeated elements. Duplicate resolved keys and `oneof` conflicts identify `""`, even when the offending names are
known; an unknown member or a failure in its unknown subtree identifies `""`. A map-entry failure identifies the map
field, except that a duplicate resolved JSON key still identifies `""`. The rules do not add rejection of unknown names.

Syntax failures in a schema member's name or in a schema message's object framing identify `""`. After a known member
name, syntax failures in that field's value identify the field; entering a nested schema message resets this
association to its own members. Arrays and map entries retain their containing field for syntax and depth failures.

## 4. Enums

1. Every enum has the value 0 named `…_UNSPECIFIED`. Value 0 is never a valid selection for a field that the schema or a
   chapter marks as required.
2. An **unknown enum value** is a value that this revision does not list: a number in range that is not listed, or, in
   JSON, a string that is not a value name of the enum.
3. Effect of unknown values, unless a chapter states otherwise:
   - singular field: the field reads as value 0 (`…_UNSPECIFIED`);
   - element of a repeated field or map value: the element is removed;
   - member of a `oneof`: the `oneof` reads as set to value 0, which is never a valid selection.
4. A JSON enum string that is not a value name is never an error by itself. A JSON value of another type is an error
   (§3.4).
5. Requests that change server state contain no repeated enum field, no enum-valued map and no optional enum whose
   removal of an unknown element could change state or bypass a check. The per-field classification is in
   [api.md §9](api.md#9-enum-fields).

## 5. Timestamps

A `google.protobuf.Timestamp` is accepted if and only if `seconds` lies in `[-62135596800, 253402300799]`
(`0001-01-01T00:00:00Z` to `9999-12-31T23:59:59Z`) and `nanos` lies in `[0, 999999999]`. This is checked on input, in
both codecs, before the value is stored or forwarded (`invalid_timestamp`). Timestamps compare by `(seconds, nanos)`.

For `invalid_timestamp`, the diagnostic field is the schema field containing the whole timestamp, in both codecs
(for example `DepotTicketClaims.iat` or `ServerEvent.time`); with no containing schema field it is `""`. This range
or timestamp-text check does not select between `seconds` and `nanos` or introduce an order between their bounds.
Separate binary framing, occurrence and wire-type failures inside a timestamp still use `Timestamp.seconds` or
`Timestamp.nanos`, as applicable.

## 6. Identifiers and text

### 6.1 UUID text

Fields documented as *canonical lowercase UUID text* MUST be exactly 36 bytes: 32 lowercase hexadecimal digits
`[0-9a-f]` in groups of 8, 4, 4, 4 and 12, separated by `-` (RFC 9562 textual form). Uppercase digits, braces, the
`urn:uuid:` prefix and the nil UUID `00000000-0000-0000-0000-000000000000` are rejected (`invalid_uuid`). Version and
variant bits are not checked. Comparison is by bytes.

### 6.2 Display text

*Display text* is UTF-8 that contains no code point in U+0000 to U+001F and none in U+007F to U+009F
(`invalid_text`). It is shown to people and never interpreted.

### 6.3 Feature and capability names

A *feature name* or *capability name* is 1 to 128 bytes matching

```
name    = segment *( "." segment ) "." version
segment = 1*( %x61-7A / %x30-39 / "-" / "_" )      ; a-z 0-9 - _
version = "v" %x31-39 *%x30-39                      ; v1, v2, ..., v10, ...
```

for example `events.v1`. Other content is rejected (`invalid_name`). A name never changes its meaning; a changed
meaning gets a new version suffix.

### 6.4 Hexadecimal identifiers in URLs

32-byte identifiers in URL paths are written as exactly 64 lowercase hexadecimal digits.

### 6.5 base64url without padding

Where this specification says *unpadded base64url*, the value uses the URL-safe alphabet of RFC 4648 §5 without `=`
padding, and the unused bits of the last character MUST be zero. Any other form is rejected (`invalid_base64`).

## 7. Field tables

The field tables in the chapters use these columns:

- **No.**: field number.
- **Type**: schema type; `repeated` and `map` are written out.
- **Presence**: *required* means the field MUST carry a non-default value (non-empty string or bytes, non-zero number,
  non-`UNSPECIFIED` enum, present message, at least one element); *optional* means the default value or absence is
  allowed and has the stated meaning; *conditional* means the chapter states when it is required.
- **Limits**: element and byte limits and every check of the field; byte and element limits are repeated in
  [limits.md](limits.md), which is authoritative if the two ever differ.
- **Meaning**: explanation only. For a message validated by the generic order alone (§7.1), every rule that a verifier
  checks is written in the Presence or Limits column, never only in Meaning; for a message with a chapter order, that
  order lists every rule. A statement of what a producer writes (for example a value derived from server state) is a
  receive check only where a check order lists it.

### 7.1 Semantic check orders

After the scan (§2.8 or §3.10), every message is validated by exactly one **semantic check order**, which is complete:
no rule of the message is checked outside it.

- The messages in the table below have a chapter order. It lists every rule of those messages, each cross-field and
  context rule at exactly one step. Where it calls the generic field validation, only stage 1 of §7.2 runs at that
  step, for the fields it names and the messages nested in them; stages 2 and 3 of §7.2 never run inside a chapter
  order. A chapter order may restrict such a call further and then names the restriction at that step (for example
  "only presence").
- Every other message is validated by the generic order of this section alone. These are all RPC requests and
  responses not listed below and the messages nested in them: `StartPairingResponse`, `BeginSessionResponse`,
  `CompleteSessionResponse`, `RevokeSelfRequest`, `RevokeSelfResponse`, `ListDevicesRequest`, `ListDevicesResponse`,
  `Device`, `RevokeDeviceRequest`, `RevokeDeviceResponse`, `ListLibraryRequest`, `ListLibraryResponse`,
  `GameSummary` (outside events), `GetGameRequest`, `GetGameResponse`, `ListVersionsRequest`, `ListVersionsResponse`,
  `VersionSummary`, `GetVersionRequest`, `GetVersionResponse`, `OpenDownloadRequest`, `RefreshTicketRequest` and
  `RefreshTicketResponse`.

| Message | Chapter order |
|---|---|
| `Envelope`, `Signature` | [envelope.md §7](envelope.md#7-verification-procedure) |
| `Manifest`, `Header`, `Chunking`, `Entry`, `Chunk` | [manifest.md §10](manifest.md#10-validation-order) |
| `ReleaseDescriptor`, `LaunchOption`, `Command`, `EmulatorRef`, `SetupStep` | [descriptor.md §5–6](descriptor.md#5-structural-validation-order) |
| `DepotTicketClaims` | [tickets.md §4](tickets.md#4-verification) |
| `SessionProof`, `CompleteSessionRequest`, `BeginSessionRequest` | [proofs.md §2](proofs.md#2-sessions) |
| `PairingProof`, `StartPairingRequest`, `PollPairingRequest` | [proofs.md §3](proofs.md#3-pairing) |
| `PollPairingResponse` | [trust.md §2](trust.md#2-establishing-a-trust-context) step 3 |
| `Discovery`, `ProtocolVersion`, `KeyFingerprint` | [api.md §2.3](api.md#23-validation) |
| `OpenDownloadResponse`, `DepotEndpoint` | [api.md §6.4.1](api.md#641-client-checks-of-a-download-response) |
| `ServerEvent` and its payloads (`Connected`, `Heartbeat`, `ResyncRequired`, `SessionEnded`, `Superseded`, `LibraryChanged` with its `GameSummary`) | [events.md §7](events.md#7-receive-validation) |
| error bodies, `ErrorInfo` | [errors.md §3.2](errors.md#32-pre-check-of-error-bodies) |

### 7.2 The generic order

The generic order has three stages. The first failing rule determines the reason.

**Stage 1, field rules.** Fields are checked in ascending field number; the elements of a repeated field one by one
in order; the entries of a map in wire order (JSON: document order). A message-typed field is validated at the position
of its field, with stage 1 applied completely to the nested message. A *conditional* Presence is not a stage 1 rule; it
is checked where the chapter states the condition. For one field or element the rules of its Presence and Limits
columns apply in the order of this table:

| Order | Rule in the field table | Reason |
|---:|---|---|
| 1 | *required*: the field is present and not default; for a repeated field at least the minimum number of elements | the reason the Presence column names, otherwise `missing_field` |
| 2 | "exactly n bytes" | the reason the field table names, otherwise `invalid_length` |
| 3 | a syntax: "canonical UUID text" (`invalid_uuid`), "display text" including its minimum length (`invalid_text`), "capability name" or "feature name" (`invalid_name`), "absolute URL" or "base URL" (`invalid_url`), or another syntax the field table names together with its reason (for example the user code or unpadded base64url) | as listed |
| 4 | a numeric bound, a fixed value or a restriction to listed enum values ("≥ 1", "1–60", "MUST be 3") | the reason the field table names, otherwise `value_out_of_range` |
| 5 | "no duplicates", checked when an element equals an earlier one | the reason the field table names, otherwise `duplicate_element` |
| 6 | "strictly ascending", checked when an element is not greater than the previous one by unsigned bytes | the reason the field table names |

The diagnostic field is the declaring message and field of the failed row, not the root traversal path. For example,
a nested `Header.chunking` presence check identifies `Header.chunking`, a repeated capability element identifies
`CompleteSessionRequest.client_capabilities`, and a map key or value identifies its map field, not a synthetic entry.

**Stage 2, cross-field rules.** After the whole top-level message, including every nested message, has passed stage
1, the rules that relate two fields of one message ("present if and only if …", "equals field …") are checked in the
field order of the top-level message and then of the nested message, each with the reason its row names. The messages
validated by the generic order alone have no such rule in this revision; a message with such a rule has a chapter
order (§7.1).

**Stage 3, context rules.** After the whole top-level message, including every nested message, has passed stages 1 and
2, its context rules are checked in field order. A context rule compares a field with the request that the message
answers; it is marked "context:" in the Limits column and names its reason (for example `game.game_id` equals the
requested `game_id`, `response_identity_mismatch`). Context rules of nested messages run at this stage too, in the
field order of the top-level message and then of the nested message.

Byte and element maximums are not part of this order: the scan has already enforced them. A field that is *optional*
and absent passes all rules. Unknown enum values have been handled while reading, before this order starts (§4 and
[api.md §9](api.md#9-enum-fields)): a removed element is not part of its field, and a *required* singular field that
reads as value 0 fails rule 1. A Limits entry that names encoded content, such as "binary `Envelope`", is not a rule of
this order: a chapter order checks it where the message has one, and a client does not check depot tickets
([tickets.md §5](tickets.md#5-transport-of-tickets)).
