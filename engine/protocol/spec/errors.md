<!--
SPDX-FileCopyrightText: 2026 the impluvium contributors
SPDX-License-Identifier: 0BSD
-->

# Errors

There are two kinds of error names in this specification:

- **wire reasons** (`ErrorReason`), which a server sends inside an error body and which clients branch on;
- **local reasons** (`snake_case` names), which local verification functions return and test vectors expect. Some local
  reasons map to a wire reason when a server reports them; local trust, transport and configuration reasons never
  appear on the wire.

Clients branch on the Connect code and the wire reason, never on message text. Schema:
[`impluvium/v2/errors.proto`](../proto/impluvium/v2/errors.proto).

## 1. `ErrorInfo`

| No. | Field | Type | Presence | Limits | Meaning |
|---|---|---|---|---|---|
| 1 | `reason` | `ErrorReason` | optional | – | Machine-readable reason; 0 means none. |
| 2 | `metadata` | `map<string, string>` | optional | ≤ 32 entries; key 1–128 bytes of `0x21`–`0x7E`; value ≤ 1 024 bytes, display text | Registered keys (§1.1). Unknown keys are ignored. |
| 3 | `retry_after_seconds` | `uint32` | optional | 0–86 400 | Seconds to wait before retrying; 0 means no hint. |

### 1.1 Metadata keys

| Key | Value |
|---|---|
| `request_id` | Identifier of the request in the server's logs, 1–128 bytes of `0x21`–`0x7E`. Servers include it with `internal` errors. |

Servers never put stack traces, database messages, file paths or other internal details in `message` or `metadata`.

## 2. Wire reasons

Unknown reasons fall back to the Connect code. A new wire reason requires a schema and specification change. Elsewhere
in this specification wire reasons are written without the `ERROR_REASON_` prefix, for example `DEVICE_REVOKED`.

| Value | Reason | Connect code (HTTP status) | Meaning and client action |
|---:|---|---|---|
| 0 | `ERROR_REASON_UNSPECIFIED` | – | No reason; the client acts on the Connect code alone. |
| 1 | `ERROR_REASON_PROTOCOL_UNSUPPORTED` | `failed_precondition` (400) | The request uses a protocol major the server does not serve. Show "incompatible"; keep credentials. |
| 2 | `ERROR_REASON_CLIENT_TOO_OLD` | `failed_precondition` (400) | The client revision or capabilities are below the server's minimum. Show "update the client"; keep credentials. |
| 3 | `ERROR_REASON_SERVER_TOO_OLD` | – | Produced locally by clients from discovery ([api.md §3](api.md#3-version-check)); servers do not send it. |
| 4 | `ERROR_REASON_TOKEN_EXPIRED` | `unauthenticated` (401) | Mint a new access token through a new session. |
| 5 | `ERROR_REASON_DEVICE_REVOKED` | `unauthenticated` (401) | The device was revoked. Credential cleanup per §7. |
| 6 | `ERROR_REASON_DEVICE_EXPIRED` | `unauthenticated` (401) | The device exceeded the idle limit. Credential cleanup per §7. |
| 7 | `ERROR_REASON_MFA_REQUIRED` | `permission_denied` (403) | Multi-factor authentication is required. No method of this revision returns it; it is reserved for approval surfaces. |
| 8 | `ERROR_REASON_PAIRING_PENDING` | `failed_precondition` (400) | Keep polling at the interval. |
| 9 | `ERROR_REASON_SLOW_DOWN` | `resource_exhausted` (429) | Add 5 seconds to the polling interval. |
| 10 | `ERROR_REASON_PAIRING_DENIED` | `permission_denied` (403) | Stop; the user denied the pairing. |
| 11 | `ERROR_REASON_PAIRING_EXPIRED` | `failed_precondition` (400) | Stop; start a new pairing. |
| 12 | `ERROR_REASON_CHALLENGE_INVALID` | `unauthenticated` (401) | The device proof was not accepted. Start a new session; keep credentials. |
| 13 | `ERROR_REASON_TICKET_EXPIRED` | `unauthenticated` (401) | Refresh the ticket and continue. |
| 14 | `ERROR_REASON_TICKET_SCOPE` | `permission_denied` (403) | The manifest is not covered by the ticket. Open the download again. |
| 15 | `ERROR_REASON_VERSION_NOT_FOUND` | `not_found` (404) | Unknown or not accessible version. |
| 16 | `ERROR_REASON_GAME_NOT_FOUND` | `not_found` (404) | Unknown or not accessible game. |
| 17 | `ERROR_REASON_LIBRARY_CHANGED` | `aborted` (409) | The stored source changed after the manifest was built (depot only). Stop this download. |
| 18 | `ERROR_REASON_PAGE_TOKEN_EXPIRED` | `failed_precondition` (400) | Restart the listing. |
| 19 | `ERROR_REASON_RATE_LIMITED` | `resource_exhausted` (429) | Retry after `retry_after_seconds` or `Retry-After`. |
| 20 | `ERROR_REASON_SERVER_STARTING` | `unavailable` (503) | Retry with backoff. |
| 21 | `ERROR_REASON_RESYNC_REQUIRED` | – | Reserved. In this revision the event stream signals resynchronisation with the `ResyncRequired` event; no RPC returns this reason. |

Errors without a reason use the Connect code alone, for example `invalid_argument` for malformed input, `not_found` for a
missing object, `unimplemented` for an unknown method or a compressed request, and `internal` with a `request_id`.

## 3. Error bodies

### 3.1 Format

Every error response of the device API, of discovery, of the document and object routes, of the event stream at open
time and of the depot has `Content-Type: application/json` and the Connect error format:

```json
{
  "code": "unauthenticated",
  "message": "access token expired",
  "details": [
    { "type": "impluvium.v2.ErrorInfo", "value": "<base64 of the binary ErrorInfo>" }
  ]
}
```

- `code`: one of the Connect code names (`canceled`, `unknown`, `invalid_argument`, `deadline_exceeded`, `not_found`,
  `already_exists`, `permission_denied`, `resource_exhausted`, `failed_precondition`, `aborted`, `out_of_range`,
  `unimplemented`, `internal`, `unavailable`, `data_loss`, `unauthenticated`). A client treats an unlisted value as
  `unknown`.
- `message`: human-readable text for logs; never used for decisions.
- `details`: typed details; `type` is the fully qualified message name, `value` the base64 of its binary encoding.
  Servers emit `value` in the standard alphabet without padding; clients accept §3.6 of
  [encoding.md](encoding.md#36-bytes). A detail MAY carry a `debug` member, which clients ignore.

For RPC errors the HTTP status is the one the Connect protocol assigns to the code (§2 shows it for every code with a
wire reason). For the other routes the status is given by the route's chapter, for example
[tickets.md §7.2](tickets.md#72-processing-order), and the body's `code` is the matching Connect code: 400
`invalid_argument`, 401 `unauthenticated`, 403 `permission_denied`, 404 `not_found`, 405 `unimplemented`, 409
`aborted`, 416 `out_of_range`, 429 `resource_exhausted`, 500 `internal`, 503 `unavailable`.

### 3.2 Pre-check of error bodies

A client checks an error body before any decoder sees it, independently of the codec of the original request:

| Order | Check | Limit |
|---:|---|---|
| 1 | `Content-Type` is `application/json` | – |
| 2 | Body size | ≤ 65 536 bytes |
| 3 | JSON profile of [encoding.md §3](encoding.md#3-json-profile), including duplicate keys and nesting | depth ≤ 64 |
| 4 | Top level is an object with a string `code` of 1–64 bytes | – |
| 5 | `message`, if present, is a string | ≤ 4 096 bytes |
| 6 | `details`, if present, is an array of objects, each with a string `type` and a string `value` | ≤ 16 elements; `type` 1–256 bytes; `value` decodes to ≤ 8 192 bytes |
| 7 | At most one detail has `type` `impluvium.v2.ErrorInfo` | – |
| 8 | That detail's decoded value passes the binary profile, the limits and the field rules of the table in §1 (stage 1 of the generic order of [encoding.md §7.2](encoding.md#72-the-generic-order); a metadata key outside `0x21`–`0x7E` is `invalid_text`); the value syntax of a registered key in §1.1 is a server obligation that a client does not check | – |

Unknown members and details of other types are ignored within these limits. If any check fails, the body is a broken
body (§6): it never reaches the internal error decoder, and the client produces a local bounded error instead.

## 4. Mapping of local reasons to Connect codes

When a server reports a local reason, it uses the Connect code of the reason's class, and the wire reason where the
table names one.

| Class | Connect code | Wire reason |
|---|---|---|
| structure (malformed input, paths, signatures, bindings of documents) | `invalid_argument` | – |
| size (byte, element, nesting and body limits) | `resource_exhausted` | – |
| unsupported (unknown format, payload type, hash algorithm, release kind, required feature) | `failed_precondition` | – |
| state (revision, known manifest, dependencies) | `failed_precondition` | – |
| proof (any rejection of a device proof after its size check) | `unauthenticated` | `CHALLENGE_INVALID` |
| ticket time (`ticket_expired`) | `unauthenticated` | `TICKET_EXPIRED` |
| ticket other (all other ticket rejections except scope) | `unauthenticated` | – |
| ticket scope (`ticket_scope`) | `permission_denied` | `TICKET_SCOPE` |
| trust, transport, configuration, client | not sent | – |

## 5. Local reason catalog

Each reason lists its class and the chapter that defines when it occurs. A reason that a rule reports for a specific
field carries that field's name (for example `path_empty` on `symlink_target`).

### 5.1 Encoding and limits

| Reason | Class | Defined in |
|---|---|---|
| `malformed_wire` | structure | [encoding.md §2.1–2.3](encoding.md#2-binary-profile) |
| `wrong_wire_type` | structure | [encoding.md §2.1](encoding.md#21-tags-and-wire-types) |
| `integer_out_of_range` | structure | [encoding.md §3.5](encoding.md#35-integers) (JSON only) |
| `invalid_utf8` | structure | [encoding.md §2.3](encoding.md#23-length-delimited-fields) |
| `duplicate_field` | structure | [encoding.md §2.4](encoding.md#24-occurrence-rules), [§3.2](encoding.md#32-field-names) |
| `split_message` | structure | [encoding.md §2.4](encoding.md#24-occurrence-rules) |
| `oneof_conflict` | structure | [encoding.md §2.4](encoding.md#24-occurrence-rules), [§3.3](encoding.md#33-null) |
| `duplicate_map_key` | structure | [encoding.md §2.4](encoding.md#24-occurrence-rules) |
| `invalid_json` | structure | [encoding.md §3.1](encoding.md#31-document) |
| `duplicate_json_key` | structure | [encoding.md §3.1](encoding.md#31-document) |
| `json_type_mismatch` | structure | [encoding.md §3.3–3.4](encoding.md#34-value-types) |
| `invalid_number` | structure | [encoding.md §3.5](encoding.md#35-integers) |
| `invalid_base64` | structure | [encoding.md §3.6](encoding.md#36-bytes), [§6.5](encoding.md#65-base64url-without-padding) |
| `invalid_timestamp` | structure | [encoding.md §3.7](encoding.md#37-timestamps), [§5](encoding.md#5-timestamps) |
| `invalid_uuid` | structure | [encoding.md §6.1](encoding.md#61-uuid-text) |
| `invalid_text` | structure | [encoding.md §6.2](encoding.md#62-display-text) |
| `invalid_name` | structure | [encoding.md §6.3](encoding.md#63-feature-and-capability-names) |
| `invalid_url` | structure | [transport.md §2](transport.md#2-base-url-and-origins) |
| `missing_field` | structure | field tables |
| `nesting_too_deep` | size | [encoding.md §2.6](encoding.md#26-nesting), [§3.9](encoding.md#39-limits) |
| `element_limit_exceeded` | size | [limits.md](limits.md) |
| `byte_limit_exceeded` | size | [limits.md](limits.md) |
| `body_too_large` | size | [limits.md §4](limits.md#4-rpc-bodies) |
| `invalid_length` | structure | [encoding.md §7.2](encoding.md#72-the-generic-order) |
| `value_out_of_range` | structure | [encoding.md §7.2](encoding.md#72-the-generic-order) |
| `duplicate_element` | structure | [encoding.md §7.2](encoding.md#72-the-generic-order) |

### 5.2 Envelope and keys

| Reason | Class | Defined in |
|---|---|---|
| `envelope_too_large` | size | [envelope.md §7](envelope.md#7-verification-procedure) |
| `no_signatures` | structure | [envelope.md §5](envelope.md#5-multiple-signatures) |
| `malformed_signature_entry` | structure | [envelope.md §7](envelope.md#7-verification-procedure) |
| `invalid_payload_type` | structure | [envelope.md §7](envelope.md#7-verification-procedure) |
| `duplicate_signature_key_id` | structure | [envelope.md §5](envelope.md#5-multiple-signatures) |
| `unknown_payload_type` | unsupported | [envelope.md §2](envelope.md#2-payload-types) |
| `unexpected_payload_type` | structure | [envelope.md §7](envelope.md#7-verification-procedure) |
| `invalid_signature` | structure | [envelope.md §5](envelope.md#5-multiple-signatures) |
| `no_trusted_signature` | structure | [envelope.md §5](envelope.md#5-multiple-signatures) |
| `trusted_set_size`, `invalid_trusted_key`, `role_mismatch`, `trusted_key_id_collision`, `invalid_ticket_key_set` | configuration | [keys.md §3](keys.md#3-trusted-key-sets) |

### 5.3 Manifest and paths

| Reason | Class | Defined in |
|---|---|---|
| `unsupported_manifest_format`, `unsupported_hash_algorithm`, `unknown_required_feature` | unsupported | [manifest.md §10](manifest.md#10-validation-order) |
| `missing_header`, `missing_chunking`, `unsorted_required_features`, `invalid_chunk_id`, `chunk_size_zero`, `unsorted_chunks`, `unsorted_entries`, `path_parent_conflict`, `invalid_entry_type`, `invalid_entry_fields`, `invalid_hash_length`, `invalid_mode`, `empty_file_hash_mismatch`, `chunk_index_out_of_range`, `chunk_size_mismatch`, `single_chunk_hash_mismatch`, `total_size_mismatch`, `unreferenced_chunk` | structure | [manifest.md §10](manifest.md#10-validation-order) |
| `chunk_size_too_large`, `size_overflow` | size | [manifest.md §10](manifest.md#10-validation-order) |
| `path_empty`, `path_forbidden_character`, `path_absolute`, `path_drive_prefix`, `path_empty_component`, `path_dot_component`, `path_component_too_long`, `symlink_escape` | structure | [paths.md §2–3](paths.md#2-base-profile) |
| `windows_forbidden_character`, `windows_trailing_dot_or_space`, `windows_reserved_name`, `windows_case_collision`, `unsupported_symlink` | structure | [paths.md §4](paths.md#4-windows-profile) |
| `windows_symlink` | warning, never a rejection | [paths.md §4.5](paths.md#45-symbolic-links) |

### 5.4 Descriptor

| Reason | Class | Defined in |
|---|---|---|
| `unsupported_release_kind` | unsupported | [descriptor.md §5](descriptor.md#5-structural-validation-order) |
| `invalid_revision`, `invalid_manifest_id`, `invalid_manifest_size`, `duplicate_requirement`, `self_requirement`, `invalid_key`, `invalid_hint_key`, `invalid_argument_text`, `invalid_env_name`, `invalid_env_value`, `duplicate_env_name`, `duplicate_launch_key`, `duplicate_setup_id`, `setup_only_with_launch_options` | structure | [descriptor.md §5](descriptor.md#5-structural-validation-order) |
| `descriptor_version_mismatch`, `descriptor_game_mismatch`, `manifest_id_mismatch`, `manifest_size_mismatch`, `manifest_binding_mismatch`, `install_size_mismatch`, `executable_not_found`, `executable_not_regular`, `working_dir_not_found` | structure | [descriptor.md §6](descriptor.md#6-context-checks) |
| `manifest_changed_for_version`, `stale_revision`, `revision_conflict`, `dependency_not_found`, `dependency_cycle`, `ticket_set_too_large` | state | [descriptor.md §6–7](descriptor.md#6-context-checks) |

### 5.5 Tickets

| Reason | Class | Defined in |
|---|---|---|
| `invalid_validity_window`, `ticket_not_yet_valid`, `ticket_key_retired`, `wrong_audience`, `unsorted_manifest_ids`, `wrong_subject` | ticket other | [tickets.md §4](tickets.md#4-verification) |
| `ticket_expired` | ticket time | [tickets.md §4](tickets.md#4-verification) |
| `ticket_scope` | ticket scope | [tickets.md §4](tickets.md#4-verification) |

When a ticket fails an envelope or encoding check, the depot reports it as "ticket other" (401), whatever the class of
the underlying reason.

### 5.6 Proofs, sessions and pairing

| Reason | Wire result | Defined in |
|---|---|---|
| `unknown_device` | `unauthenticated` (BeginSession: no reason; CompleteSession: `CHALLENGE_INVALID`) | [proofs.md §2](proofs.md#2-sessions) |
| `invalid_challenge_length`, `unsupported_session_scope`, `duplicate_capability`, `invalid_public_key`, `invalid_pairing_id_length`, `invalid_user_code` | `invalid_argument` (inside a proof: `unauthenticated`, `CHALLENGE_INVALID`) | [proofs.md](proofs.md) |
| `client_too_old` | `failed_precondition`, `CLIENT_TOO_OLD` | [proofs.md §2.2](proofs.md#22-completesession) |
| `proof_instance_mismatch`, `proof_device_mismatch`, `proof_challenge_mismatch`, `proof_pairing_mismatch`, `challenge_unknown`, `challenge_expired`, `challenge_consumed` | `unauthenticated`, `CHALLENGE_INVALID` | [proofs.md](proofs.md) |
| `device_revoked` | `unauthenticated`, `DEVICE_REVOKED` | [proofs.md §2](proofs.md#2-sessions) |
| `device_expired` | `unauthenticated`, `DEVICE_EXPIRED` | [proofs.md §2](proofs.md#2-sessions) |
| `pairing_unknown`, `pairing_expired` | `failed_precondition`, `PAIRING_EXPIRED` | [proofs.md §3.4](proofs.md#34-pollpairing) |
| `pairing_denied` | `permission_denied`, `PAIRING_DENIED` | [proofs.md §3.4](proofs.md#34-pollpairing) |
| `slow_down` | `resource_exhausted`, `SLOW_DOWN` | [proofs.md §3.4](proofs.md#34-pollpairing) |
| `pairing_pending` | `failed_precondition`, `PAIRING_PENDING` | [proofs.md §3.4](proofs.md#34-pollpairing) |

### 5.7 API, trust, transport and events (local)

| Reason | Class | Defined in |
|---|---|---|
| `not_an_impluvium_server`, `protocol_unsupported`, `server_too_old`, `client_too_old` | client | [api.md §2–3](api.md#2-discovery) |
| `instance_mismatch`, `content_key_changed` | trust | [trust.md §3](trust.md#3-states) |
| `invalid_page_token` | structure (server) | [api.md §7](api.md#7-pagination) |
| `insecure_scheme_not_allowed`, `platform_trust_unavailable`, `origin_not_allowed`, `headers_too_large`, `redirect_not_allowed`, `redirect_cross_origin`, `redirect_limit`, `unsupported_content_encoding`, `response_too_large`, `redirect_target_not_allowed` | transport | [transport.md](transport.md) |
| `request_too_large`, `rate_limited`, `server_unavailable`, `broken_error_body` | transport | §6 |
| `invalid_client_id`, `client_table_full`, `invalid_topic`, `invalid_parameter` | events (server, HTTP 400 or 429) | [events.md §1](events.md#1-opening-the-stream) |
| `event_too_large`, `event_malformed`, `event_out_of_order`, `event_epoch_mismatch`, `event_name_mismatch`, `event_id_mismatch`, `event_sequence_invalid`, `event_payload_invalid`, `event_identity_mismatch` | events (client: stream broken) | [events.md §7](events.md#7-receive-validation) |
| `manifest_set_mismatch`, `response_identity_mismatch`, `unsorted_manifest_ids` (in responses) | client | [api.md §6](api.md#6-services) |

## 6. Responses without a valid error body

A response whose status is not 200 (for RPC) or not the expected success status (for other routes), and whose body
fails the pre-check of §3.2, is classified by status:

| Status | Local result | Client behaviour |
|---|---|---|
| 413 | `request_too_large` | The request is too large; do not retry it unchanged. |
| 429 | `rate_limited` | Retry after a valid `Retry-After` (item 2 below), otherwise after the client's backoff. |
| 502, 503, 504 | `server_unavailable` | Server not available; retry with bounded backoff. |
| any other, or an HTML page, truncated JSON, a wrong `Content-Type`, an over-long body | `broken_error_body` | Protocol or availability error; retry with bounded backoff. |

1. In every case of this table the client keeps its credentials and its trust context, never signs out and never
   treats the response as a device revocation.
2. `Retry-After` is valid if it is a decimal integer of 1 to 10 digits whose value is at most 86 400 (seconds). The
   HTTP-date form and other values are ignored.
3. Bounded backoff: the client waits with exponential growth and random jitter, starting at no less than 1 second and
   never waiting longer than 300 seconds between attempts, unless a valid `Retry-After` asks for longer.
4. A response with a valid error body is handled by its code and reason, whatever its status.

## 7. Credential cleanup

A client deletes the credentials of a trust context only if all of the following hold:

1. the response came from the server origin of that trust context, over a connection that met
   [transport.md](transport.md);
2. the error body passed the pre-check of §3.2;
3. its code is `unauthenticated` and its single `ErrorInfo` has reason `DEVICE_REVOKED` or `DEVICE_EXPIRED`.

Nothing else deletes credentials automatically: no broken body, no proxy page, no version state, no trust state, no
depot response and no `SessionEnded` event. After a `SessionEnded` event the client confirms the device state with its
next RPC.
