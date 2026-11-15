<!--
SPDX-FileCopyrightText: 2026 the impluvium contributors
SPDX-License-Identifier: 0BSD
-->

# Device API

## 1. Surfaces

All paths are relative to the server's **base URL** ([transport.md §2](transport.md#2-base-url-and-origins)), which may
contain a path prefix.

| Surface | Method and path | Authentication | Chapter |
|---|---|---|---|
| Discovery | `GET {base}/api/info` | none | §2 |
| RPC | `POST {base}/api/v2/impluvium.v2.<Service>/<Method>` | per method (§5) | §5 to §7 |
| Manifest | `GET {base}/api/v2/manifests/{manifest_id_hex}` | access token | §8 |
| Descriptor | `GET {base}/api/v2/descriptors/{version_id}` | access token | §8 |
| Object | `GET {base}/api/v2/objects/{object_id}` | access token | §8 |
| Events | `GET {base}/api/v2/events` | access token | [events.md](events.md) |
| Depot | `GET {depot_base_url}/depot/v2/{manifest_id_hex}/{chunk_id_hex}` | depot ticket | [tickets.md §7](tickets.md#7-depot-http-api) |

A conforming client calls no other path of the server. Every other route of the server belongs to its own web
interface, is not versioned and may change in any release.

## 2. Discovery

### 2.1 Request and response

`GET {base}/api/info` needs no authentication, reveals no game or user data and is rate-limited by the server. The
response is `200` with `Content-Type: application/json` and the ProtoJSON encoding of `impluvium.v2.Discovery`. Its shape
is frozen: later revisions only add fields, and the document is never served under a version prefix.

### 2.2 Fields

`Discovery`:

| No. | Field | Type | Presence | Limits | Meaning |
|---|---|---|---|---|---|
| 1 | `discovery` | `uint32` | required | MUST be 1 | Document version. |
| 2 | `product` | `string` | required | MUST be `impluvium` | Product identification. |
| 3 | `server_name` | `string` | required | 1–128 bytes, display text | Operator-chosen name. |
| 4 | `server_version` | `string` | required | 1–64 bytes, display text | Server release as Semantic Versioning 2.0.0 text; for display. |
| 5 | `instance_id` | `string` | required | canonical UUID text | Instance identity ([trust.md](trust.md)). |
| 6 | `protocol` | `ProtocolVersion` | required | – | Protocol version (§3). |
| 7 | `manifest_formats` | repeated `uint32` | required | 1–8 elements, no duplicates | Supported manifest formats; contains 3. |
| 8 | `capabilities` | repeated `string` | optional | ≤ 64 elements, capability names, no duplicates | Server capabilities (§4). |
| 9 | `required_client_capabilities` | repeated `string` | optional | ≤ 64 elements, capability names, no duplicates | Capabilities a client must offer (§4). |
| 10 | `key_fingerprints` | repeated `KeyFingerprint` | optional | ≤ 16 elements | Informational key fingerprints ([trust.md §5](trust.md#5-discovery-fingerprints)). |
| 11 | `commit` | `string` | optional | 0–64 bytes, display text | Source revision of the server build. |
| 12 | `source_url` | `string` | required | 1–2 048 bytes, absolute `https` or `http` URL | Where the server's source code is offered. |

`ProtocolVersion`:

| No. | Field | Type | Presence | Limits | Meaning |
|---|---|---|---|---|---|
| 1 | `major` | `uint32` | required | ≥ 1 | Protocol major; 2 for this specification. |
| 2 | `revision` | `uint32` | required | ≥ 1 | Revision the server implements. |
| 3 | `min_client_revision` | `uint32` | required | ≥ 1 | Lowest client revision the server accepts. |

`KeyFingerprint`:

| No. | Field | Type | Presence | Limits | Meaning |
|---|---|---|---|---|---|
| 1 | `role` | `KeyRole` | required | `KEY_ROLE_CONTENT` or `KEY_ROLE_DEPOT_TICKET` | Role of the key. |
| 2 | `sha256` | `bytes` | required | 32 bytes | SHA-256 over the 32-byte public key. |

An entry of `key_fingerprints` with another role or another `sha256` length is ignored.

### 2.3 Validation

A client reads discovery with the response rules of [transport.md §6](transport.md#6-response-protection): no redirect,
`Content-Type` `application/json` (parameters ignored), body at most 65 536 bytes, the JSON profile of
[encoding.md §3](encoding.md#3-json-profile). It then checks, in this order:

| Order | Check | Result |
|---:|---|---|
| 1 | `discovery = 1` and `product = "impluvium"` | `not_an_impluvium_server` |
| 2 | The other fields of §2.2 in field-number order, each with the field rules (stage 1) of [encoding.md §7.2](encoding.md#72-the-generic-order): `server_name` and `server_version` (`missing_field`, `invalid_text`), `instance_id` (`invalid_uuid`), `protocol` and its three fields (`missing_field`, `value_out_of_range`), `manifest_formats` (`missing_field`, `duplicate_element`), `capabilities` and `required_client_capabilities` element by element (`invalid_name`, `duplicate_element`), `commit` (`invalid_text`), `source_url` (`missing_field`, `invalid_url`) | as listed |
| 3 | `manifest_formats` contains 3 | `protocol_unsupported` |

`key_fingerprints` entries are not validated beyond the scan: an entry with an unknown role or a `sha256` of another
length is ignored.

The comparisons against a stored trust context and the version check follow in the order of
[trust.md §4](trust.md#4-checks-before-every-credential-transfer).

## 3. Version check

A client has three compiled-in values: `major`, `revision` and `min_server_revision`. Clients of this specification use
`major = 2`, `revision = 1` and `min_server_revision = 1`. With the server's `protocol` from discovery, the client
decides in this order:

1. `server.major ≠ client.major` → `protocol_unsupported`;
2. `server.revision < client.min_server_revision` → `server_too_old`;
3. `client.revision < server.min_client_revision` → `client_too_old`;
4. otherwise the versions are compatible.

These states are typed and shown to the user. None of them signs the device out or deletes credentials. A server raises
`revision` with every release that changes the schema or a normative rule; in the 0.x series a breaking change raises
`min_client_revision`. This is a revision scheme, not a range of supported protocol versions.

A server MAY announce that a method or route will go away with the `Deprecation` (RFC 9745) and `Sunset` (RFC 8594)
response headers; clients log them and never change behaviour because of them.

## 4. Capabilities

1. A capability is a capability name ([encoding.md §6.3](encoding.md#63-feature-and-capability-names)). A name never
   changes its meaning.
2. The server lists its capabilities in discovery. The client offers its capabilities in `CompleteSession`; the server
   returns the accepted ones, and they are stored with the session only.
3. Unknown capabilities are ignored by both sides.
4. If discovery lists `required_client_capabilities` that the client does not implement, the client is too old
   (`client_too_old`); the server enforces the same in `CompleteSession` with `CLIENT_TOO_OLD`.

Registry of this revision:

| Name | Status | Meaning |
|---|---|---|
| `events.v1` | defined | The event stream of [events.md](events.md). |
| `saves.v1`, `playtime.v1`, `depot.range.v1`, `depot.bundle.v1`, `codec.zstd.v1` | reserved | No behaviour is defined in this revision; servers do not list them and clients do not offer them. Range requests are part of the base depot contract and need no capability. |

## 5. Connect unary calls

1. Every RPC is a Connect unary call: `POST {base}/api/v2/impluvium.v2.<Service>/<Method>` with
   `Content-Type: application/proto` (binary) or `application/json` (ProtoJSON). The response uses the same codec.
2. There are no streaming RPCs and no Connect `GET` requests.
3. Requests and responses carry no `Content-Encoding` other than `identity`, and clients send no `Accept-Encoding`
   other than `identity`. A server answers a compressed request with `unimplemented`.
4. Clients SHOULD send `Connect-Protocol-Version: 1`. A client MAY send `Connect-Timeout-Ms`.
5. A success is HTTP 200 with the response message. An error is a non-200 status with the error body of
   [errors.md §3](errors.md#3-error-bodies).
6. Authentication: `Authorization: Bearer <access_token>` ([proofs.md §2.3](proofs.md#23-access-tokens)) on every
   method except `StartPairing`, `PollPairing`, `BeginSession` and `CompleteSession`, which authenticate through their
   own data. A token-checked method answers per [proofs.md §2.4](proofs.md#24-token-use-and-device-state) when the token
   is missing or invalid.
7. Body limits: [limits.md §4](limits.md#4-rpc-bodies). A body above the request limit is rejected before decoding
   (`resource_exhausted`).
8. Hidden or age-restricted content, and content the user may not access, is reported as not found
   (`GAME_NOT_FOUND`, `VERSION_NOT_FOUND`), never as `permission_denied`.
9. A method that requires an access token processes a request in this order; the first failing step determines the
   result: the checks of items 3 and 7; the token checks of [proofs.md §2.4](proofs.md#24-token-use-and-device-state);
   decoding and the check order of the request message ([encoding.md §7.1](encoding.md#71-semantic-check-orders)),
   reported with the Connect code of the reason's class
   ([errors.md §4](errors.md#4-mapping-of-local-reasons-to-connect-codes)); the page token (§7 item 6: malformed or
   used with other request fields, then expired); existence and access (item 8). The pairing and session methods
   follow their own orders in [proofs.md](proofs.md).

## 6. Services

The request and response fields of the pairing and session methods are listed here; their processing order is in
[proofs.md](proofs.md).

### 6.1 `AuthService`

| Method | Auth | Request → response | Errors (Connect code, reason) |
|---|---|---|---|
| `StartPairing` | none | `StartPairingRequest` → `StartPairingResponse` | `invalid_argument`; `resource_exhausted` + `RATE_LIMITED` |
| `PollPairing` | pairing proof | `PollPairingRequest` → `PollPairingResponse` | see [proofs.md §3.4](proofs.md#34-pollpairing) |
| `BeginSession` | none | `BeginSessionRequest` → `BeginSessionResponse` | `invalid_argument`; `unauthenticated`; `resource_exhausted` + `RATE_LIMITED` |
| `CompleteSession` | session proof | `CompleteSessionRequest` → `CompleteSessionResponse` | see [proofs.md §2.2](proofs.md#22-completesession) |
| `RevokeSelf` | access token | `RevokeSelfRequest` → `RevokeSelfResponse` | token errors |

`StartPairingRequest`:

| No. | Field | Type | Presence | Limits | Meaning |
|---|---|---|---|---|---|
| 1 | `device_name` | `string` | required | 1–128 bytes, display text | Name shown at approval and in the device list. |
| 2 | `platform` | `string` | required | 1–32 bytes, display text | Operating-system label. |
| 3 | `arch` | `string` | required | 1–32 bytes, display text | Architecture label. |
| 4 | `public_key` | `bytes` | required | exactly 32 bytes, valid point | Device public key. |
| 5 | `client_version` | `string` | required | 1–64 bytes, display text | Client version. |

`StartPairingResponse`:

| No. | Field | Type | Presence | Limits | Meaning |
|---|---|---|---|---|---|
| 1 | `pairing_id` | `bytes` | required | exactly 16 bytes (`invalid_pairing_id_length`) | Pairing identifier, secret to the device. |
| 2 | `user_code` | `string` | required | exactly 9 bytes (`invalid_user_code`); user code syntax of [proofs.md §3.2](proofs.md#32-user-code): four alphabet characters, `-`, four alphabet characters (`invalid_user_code`) | Code to confirm. |
| 3 | `verification_uri` | `string` | required | 1–2 048 bytes, absolute URL | Approval page. |
| 4 | `verification_uri_complete` | `string` | required | 1–2 048 bytes, absolute URL | Approval page with the code. |
| 5 | `interval_seconds` | `uint32` | required | 1–60 | Minimum polling interval. |
| 6 | `expires_at` | `Timestamp` | required | – | Pairing expiry. |

`PollPairingRequest`:

| No. | Field | Type | Presence | Limits | Meaning |
|---|---|---|---|---|---|
| 1 | `pairing_id` | `bytes` | required | exactly 16 bytes | From `StartPairingResponse`. |
| 2 | `proof` | `bytes` | required | ≤ 4 096 bytes, binary `Envelope` | Pairing proof. |

`PollPairingResponse`:

| No. | Field | Type | Presence | Limits | Meaning |
|---|---|---|---|---|---|
| 1 | `device_id` | `string` | required | canonical UUID text | Assigned device identity. |
| 2 | `instance_id` | `string` | required | canonical UUID text | Instance identity. |
| 3 | `content_public_key` | `bytes` | required | exactly 32 bytes (`invalid_trusted_key`); point checks in [trust.md §2](trust.md#2-establishing-a-trust-context) step 3 | Content key to pin. |

`BeginSessionRequest`:

| No. | Field | Type | Presence | Limits | Meaning |
|---|---|---|---|---|---|
| 1 | `device_id` | `string` | required | canonical UUID text | Device asking for a challenge. |

`BeginSessionResponse`:

| No. | Field | Type | Presence | Limits | Meaning |
|---|---|---|---|---|---|
| 1 | `challenge` | `bytes` | required | exactly 32 bytes (`invalid_challenge_length`) | Single-use challenge. |
| 2 | `expires_at` | `Timestamp` | required | – | Issue time plus 60 seconds. |

`CompleteSessionRequest`:

| No. | Field | Type | Presence | Limits | Meaning |
|---|---|---|---|---|---|
| 1 | `device_id` | `string` | required | canonical UUID text | Device. |
| 2 | `challenge` | `bytes` | required | exactly 32 bytes | Challenge being answered. |
| 3 | `proof` | `bytes` | required | ≤ 4 096 bytes, binary `Envelope` | Session proof. |
| 4 | `client_capabilities` | repeated `string` | optional | ≤ 64 elements, capability names, no duplicates | Offered capabilities. |
| 5 | `scope` | `SessionScope` | required | MUST be `SESSION_SCOPE_API` | Token scope. |

`CompleteSessionResponse`:

| No. | Field | Type | Presence | Limits | Meaning |
|---|---|---|---|---|---|
| 1 | `access_token` | `string` | required | exactly 43 bytes (`invalid_base64`); unpadded base64url (`invalid_base64`) | Bearer token. |
| 2 | `expires_at` | `Timestamp` | required | – | Token expiry. |
| 3 | `accepted_capabilities` | repeated `string` | optional | ≤ 64 elements, capability names, no duplicates | Accepted capabilities. |
| 4 | `scope` | `SessionScope` | required (`unsupported_session_scope`) | MUST be `SESSION_SCOPE_API` (`unsupported_session_scope`) | Token scope. |

`SessionScope`:

| Value | Name | Meaning |
|---:|---|---|
| 0 | `SESSION_SCOPE_UNSPECIFIED` | Never valid. |
| 1 | `SESSION_SCOPE_API` | Device API access. |
| 2 | `SESSION_SCOPE_WEB` | Reserved; not issued in this revision (§10). |

`RevokeSelfRequest` and `RevokeSelfResponse` have no fields.

A client validates `StartPairingResponse`, `BeginSessionResponse` and `CompleteSessionResponse` with the generic order
of [encoding.md §7.2](encoding.md#72-the-generic-order), using the reasons named in their field tables.
`PollPairingResponse` is validated in the order of [trust.md §2](trust.md#2-establishing-a-trust-context) step 3.

### 6.2 `DeviceService`

| Method | Auth | Request → response | Errors |
|---|---|---|---|
| `ListDevices` | access token | `ListDevicesRequest` → `ListDevicesResponse` | token errors; `invalid_argument`; `failed_precondition` + `PAGE_TOKEN_EXPIRED` |
| `RevokeDevice` | access token | `RevokeDeviceRequest` → `RevokeDeviceResponse` | token errors; `invalid_argument`; `not_found` |

Both methods act on the devices of the calling user only. `RevokeDevice` revokes exactly one device; revoking an
already revoked device succeeds without change; a device of another user is reported as `not_found`.

`ListDevicesRequest`:

| No. | Field | Type | Presence | Limits | Meaning |
|---|---|---|---|---|---|
| 1 | `page_size` | `uint32` | optional (0: 50) | values above 200 count as 200 | Requested page size (§7). |
| 2 | `page_token` | `string` | optional (empty: first page) | ≤ 1 024 bytes | Continuation token (§7). |

`ListDevicesResponse`:

| No. | Field | Type | Presence | Limits | Meaning |
|---|---|---|---|---|---|
| 1 | `devices` | repeated `Device` | optional (empty page) | ≤ 200 elements | One page of the caller's devices, including revoked ones. |
| 2 | `next_page_token` | `string` | optional (empty: end of list) | ≤ 1 024 bytes | Continuation token (§7). |

`Device`:

| No. | Field | Type | Presence | Limits | Meaning |
|---|---|---|---|---|---|
| 1 | `device_id` | `string` | required | canonical UUID text | Device identity. |
| 2 | `name` | `string` | required | 1–128 bytes, display text | From pairing. |
| 3 | `platform` | `string` | required | 1–32 bytes, display text | From pairing. |
| 4 | `arch` | `string` | required | 1–32 bytes, display text | From pairing. |
| 5 | `client_version` | `string` | required | 1–64 bytes, display text | From pairing. |
| 6 | `created_at` | `Timestamp` | required | – | Pairing time. |
| 7 | `last_seen_at` | `Timestamp` | optional (absent: no session yet) | – | Last successful session. |
| 8 | `revoked_at` | `Timestamp` | optional (absent: not revoked) | – | Revocation time, set when the device was revoked. |
| 9 | `current` | `bool` | optional (false) | – | True for the calling device. |

`RevokeDeviceRequest`:

| No. | Field | Type | Presence | Limits | Meaning |
|---|---|---|---|---|---|
| 1 | `device_id` | `string` | required | canonical UUID text | Device to revoke. |

`RevokeDeviceResponse` has no fields.

### 6.3 `LibraryService`

| Method | Auth | Request → response | Errors |
|---|---|---|---|
| `ListLibrary` | access token | `ListLibraryRequest` → `ListLibraryResponse` | token errors; `invalid_argument`; `failed_precondition` + `PAGE_TOKEN_EXPIRED` |
| `GetGame` | access token | `GetGameRequest` → `GetGameResponse` | token errors; `invalid_argument`; `not_found` + `GAME_NOT_FOUND` |
| `ListVersions` | access token | `ListVersionsRequest` → `ListVersionsResponse` | token errors; `invalid_argument`; `not_found` + `GAME_NOT_FOUND`; `failed_precondition` + `PAGE_TOKEN_EXPIRED` |
| `GetVersion` | access token | `GetVersionRequest` → `GetVersionResponse` | token errors; `invalid_argument`; `not_found` + `VERSION_NOT_FOUND` |

Summaries list only content the calling user may access, and never contain storage paths or keys.

`ListLibraryRequest`:

| No. | Field | Type | Presence | Limits | Meaning |
|---|---|---|---|---|---|
| 1 | `page_size` | `uint32` | optional (0: 50) | values above 200 count as 200 | Requested page size (§7). |
| 2 | `page_token` | `string` | optional (empty: first page) | ≤ 1 024 bytes | Continuation token (§7). |

`ListLibraryResponse`:

| No. | Field | Type | Presence | Limits | Meaning |
|---|---|---|---|---|---|
| 1 | `games` | repeated `GameSummary` | optional (empty page) | ≤ 200 elements | One page. |
| 2 | `next_page_token` | `string` | optional (empty: end of list) | ≤ 1 024 bytes | Continuation token (§7). |
| 3 | `epoch` | `bytes` | required | exactly 16 bytes | Event epoch of the page ([events.md §8](events.md#8-library-snapshot-and-merge)). |
| 4 | `snapshot_seq` | `uint64` | optional (0: no event yet in the epoch) | – | The user's highest sequence number in that epoch when the page was produced. |

`GameSummary`:

| No. | Field | Type | Presence | Limits | Meaning |
|---|---|---|---|---|---|
| 1 | `game_id` | `string` | required | canonical UUID text | Game identity. |
| 2 | `name` | `string` | required | 1–256 bytes, display text | Display name. |
| 3 | `platforms` | repeated `Platform` | optional (empty: none shown) | ≤ 8 elements, no duplicates | Derived from current descriptors. Display only. |
| 4 | `entry_version` | `uint64` | optional (0: unchanged in the epoch) | – | Sequence number of the entry's last change in the current epoch. |
| 5 | `icon_object_id` | `string` | optional (empty: no icon) | empty or canonical UUID text | Icon image (§8.3). |
| 6 | `cover_object_id` | `string` | optional (empty: no cover) | empty or canonical UUID text | Cover image (§8.3). |

`GetGameRequest`:

| No. | Field | Type | Presence | Limits | Meaning |
|---|---|---|---|---|---|
| 1 | `game_id` | `string` | required | canonical UUID text | Requested game. |

`GetGameResponse`:

| No. | Field | Type | Presence | Limits | Meaning |
|---|---|---|---|---|---|
| 1 | `game` | `GameSummary` | required | context: `game.game_id` equals the requested `game_id` (`response_identity_mismatch`) | The game. |

`ListVersionsRequest`:

| No. | Field | Type | Presence | Limits | Meaning |
|---|---|---|---|---|---|
| 1 | `game_id` | `string` | required | canonical UUID text | Game whose versions are listed. |
| 2 | `page_size` | `uint32` | optional (0: 50) | values above 200 count as 200 | Requested page size (§7). |
| 3 | `page_token` | `string` | optional (empty: first page) | ≤ 1 024 bytes | Continuation token (§7). |

`ListVersionsResponse`:

| No. | Field | Type | Presence | Limits | Meaning |
|---|---|---|---|---|---|
| 1 | `versions` | repeated `VersionSummary` | optional (empty page) | ≤ 200 elements; context: each `game_id` equals the requested `game_id` (`response_identity_mismatch`) | One page. |
| 2 | `next_page_token` | `string` | optional (empty: end of list) | ≤ 1 024 bytes | Continuation token (§7). |

`VersionSummary`:

| No. | Field | Type | Presence | Limits | Meaning |
|---|---|---|---|---|---|
| 1 | `version_id` | `string` | required | canonical UUID text | Version identity. |
| 2 | `game_id` | `string` | required | canonical UUID text | Game identity. |
| 3 | `display_name` | `string` | optional (empty: no name) | ≤ 256 bytes, display text | Version name. |
| 4 | `kind` | `ReleaseKind` | required | – | From the current descriptor. |
| 5 | `platforms` | repeated `Platform` | optional (empty: none shown) | ≤ 8 elements, no duplicates | From the current descriptor. Display only. |
| 6 | `install_size` | `uint64` | optional (0: empty tree) | – | `Manifest.total_size`. |
| 7 | `manifest_size` | `uint64` | required | 1–536 870 912 | Manifest payload length. |
| 8 | `manifest_id` | `bytes` | required | exactly 32 bytes | Manifest identity. |
| 9 | `descriptor_revision` | `uint64` | required | ≥ 1 | Current descriptor revision. |

`GetVersionRequest`:

| No. | Field | Type | Presence | Limits | Meaning |
|---|---|---|---|---|---|
| 1 | `version_id` | `string` | required | canonical UUID text | Requested version. |

`GetVersionResponse`:

| No. | Field | Type | Presence | Limits | Meaning |
|---|---|---|---|---|---|
| 1 | `version` | `VersionSummary` | required | context: `version.version_id` equals the requested `version_id` (`response_identity_mismatch`) | The version. |

### 6.4 `DownloadService`

| Method | Auth | Request → response | Errors |
|---|---|---|---|
| `OpenDownload` | access token | `OpenDownloadRequest` → `OpenDownloadResponse` | token errors; `invalid_argument`; `not_found` + `VERSION_NOT_FOUND` |
| `RefreshTicket` | access token | `RefreshTicketRequest` → `RefreshTicketResponse` | token errors; `invalid_argument`; `not_found` + `VERSION_NOT_FOUND` |

`OpenDownload` runs the server's single entitlement check for the version (access rules, age rating, platform,
dependencies, deleted versions) and returns everything a client needs to fetch the version. `RefreshTicket` runs the same
check again and returns a new ticket; a version the user can no longer access yields `VERSION_NOT_FOUND`.

`OpenDownloadRequest` and `RefreshTicketRequest`:

| No. | Field | Type | Presence | Limits | Meaning |
|---|---|---|---|---|---|
| 1 | `version_id` | `string` | required | canonical UUID text | Requested version. |

`OpenDownloadResponse`:

| No. | Field | Type | Presence | Limits | Meaning |
|---|---|---|---|---|---|
| 1 | `descriptor` | `bytes` | required | ≤ 1 064 960 bytes, binary `Envelope` | Current release descriptor of the requested version. |
| 2 | `manifest_ids` | repeated `bytes` | required (1–8 elements) | each exactly 32 bytes; strictly ascending (`unsorted_manifest_ids`) | Resolved manifest set; the server issues the ticket for the same set. |
| 3 | `ticket` | `bytes` | required | ≤ 4 096 bytes; a binary `Envelope` that the client does not verify ([tickets.md §5](tickets.md#5-transport-of-tickets)) | Depot ticket. |
| 4 | `depot_endpoints` | repeated `DepotEndpoint` | required (1–8 elements) | – | Depot base URLs offered by the server. |

`DepotEndpoint`:

| No. | Field | Type | Presence | Limits | Meaning |
|---|---|---|---|---|---|
| 1 | `base_url` | `string` | required | 1–2 048 bytes; absolute URL (`http` or `https`) without user information, query or fragment, not ending in `/` | Depot base URL. |
| 2 | `priority` | `uint32` | optional (0: most preferred) | – | Lower values are preferred; equal values keep list order. |

`RefreshTicketResponse`:

| No. | Field | Type | Presence | Limits | Meaning |
|---|---|---|---|---|---|
| 1 | `ticket` | `bytes` | required | ≤ 4 096 bytes; a binary `Envelope` that the client does not verify ([tickets.md §5](tickets.md#5-transport-of-tickets)) | New depot ticket. |
| 2 | `manifest_ids` | repeated `bytes` | required (1–8 elements) | each exactly 32 bytes; strictly ascending (`unsorted_manifest_ids`) | Resolved manifest set; the server issues the ticket for the same set. |

#### 6.4.1 Client checks of a download response

A client processes an `OpenDownloadResponse` in this order and uses nothing from it before all checks have passed. This
is the complete check order of `OpenDownloadResponse` and `DepotEndpoint`
([encoding.md §7.1](encoding.md#71-semantic-check-orders)):

| Order | Check | Reason |
|---:|---|---|
| 1 | Field rules (stage 1 of the generic order, [encoding.md §7.2](encoding.md#72-the-generic-order)) of all fields with the reasons of the field tables, including `manifest_ids` strictly ascending; for `descriptor` and `depot_endpoints[i].base_url` only presence, because steps 2 and 5 check their content | as listed there; `unsorted_manifest_ids` |
| 2 | `descriptor` passes [descriptor.md §5](descriptor.md#5-structural-validation-order) (envelope with the pinned content key, then structure) | descriptor reasons |
| 3 | Descriptor context checks 0a and 0b of [descriptor.md §6](descriptor.md#6-context-checks): `version_id` equals the requested `version_id`, `game_id` equals the expected game if the client knows it | `descriptor_version_mismatch`, `descriptor_game_mismatch` |
| 4 | The descriptor's `manifest_id` is an element of `manifest_ids` | `manifest_set_mismatch` |
| 5 | Every `depot_endpoints[i].base_url` is a valid depot base URL ([transport.md §2](transport.md#2-base-url-and-origins)) | `invalid_url` |

It then fetches the manifest by `manifest_id` (§8.1), runs the remaining context checks of
[descriptor.md §6](descriptor.md#6-context-checks), and contacts a depot only under
[transport.md §5](transport.md#5-depot-origins-and-credentials). Descriptors of required versions are fetched with §8.2
and checked with that GET's `version_id` as the expected version.

A `RefreshTicketResponse` is validated with the generic order alone
([encoding.md §7.2](encoding.md#72-the-generic-order)), with the reasons of its field table; the new manifest set
replaces the old one for later depot requests.

## 7. Pagination

1. Every `List…` method takes `page_size` and `page_token` and returns `next_page_token`.
2. `page_size = 0` means 50. Values above 200 are treated as 200. Responses contain at most 200 elements.
3. `page_token` is empty for the first page. Otherwise it is a `next_page_token` returned by the same method for the
   same other request fields; the client passes it back unchanged. Tokens are opaque strings of at most 1 024 bytes.
4. `next_page_token` is empty if and only if the list has ended.
5. The order is stable: the server sorts by a fixed sort key and then by identifier. There are no offsets.
6. A token is valid for one hour after the server issued it: it is accepted while `now < issued + 3 600 s` and refused
   from then on with `failed_precondition` and `PAGE_TOKEN_EXPIRED`. A malformed token, or one used with other request
   fields, yields `invalid_argument` (`invalid_page_token`).

## 8. Document and object routes

All three routes require `Authorization: Bearer <access_token>`, answer token errors as in
[proofs.md §2.4](proofs.md#24-token-use-and-device-state), apply the server's entitlement check, report content the user
may not access as 404, use no content coding, and use the error body of [errors.md §3](errors.md#3-error-bodies) for
errors. A 401 response carries `WWW-Authenticate: Bearer error="invalid_token"`; 429 and 503 responses carry
`Retry-After`. Clients read them with the response rules of [transport.md §6](transport.md#6-response-protection).

### 8.1 Manifests

`GET {base}/api/v2/manifests/{manifest_id_hex}` returns the binary manifest envelope: `200`,
`Content-Type: application/octet-stream`, exact `Content-Length`, `ETag: "<manifest_id_hex>"`,
`Cache-Control: private, immutable`. The body is at most 536 870 912 + 16 384 bytes. A path segment that is not 64
lowercase hexadecimal digits, an unknown manifest and a manifest the user may not access yield 404. The client verifies
the envelope ([manifest.md](manifest.md)) and checks that `SHA-256(payload)` equals the requested `manifest_id`
(`manifest_id_mismatch`).

### 8.2 Descriptors

`GET {base}/api/v2/descriptors/{version_id}` returns the binary envelope of the current release descriptor of the
version: `200`, `Content-Type: application/octet-stream`, exact `Content-Length`, `Cache-Control: private, no-cache`.
The body is at most 1 048 576 + 16 384 bytes. A `version_id` that is not canonical UUID text, an unknown version and a
version the user may not access yield 404 with `VERSION_NOT_FOUND`. The client verifies the envelope
([descriptor.md §5](descriptor.md#5-structural-validation-order)) and runs the context checks of
[descriptor.md §6](descriptor.md#6-context-checks) with the `version_id` of the path as the expected version
(`descriptor_version_mismatch`).

### 8.3 Objects

`GET {base}/api/v2/objects/{object_id}` returns a stored object such as an image, with its stored `Content-Type` and an
exact `Content-Length`. Objects are not signed and are treated as untrusted data. An `object_id` that is not canonical
UUID text, or that is unknown or not accessible, yields 404. The client bounds the body with a limit chosen by the
caller for the object's purpose; this specification fixes no global object size.

## 9. Enum fields

For every enum-typed field: the meaning of absent, `UNSPECIFIED` and empty, the effect of an unknown value, and whether
the field is display, filter or state-changing. No request that changes state contains a repeated enum, an enum-valued
map or an enum whose removal could change state.

| Field | Direction | Class | Absent / `UNSPECIFIED` / empty | Unknown value |
|---|---|---|---|---|
| `CompleteSessionRequest.scope` | request | state-changing (singular) | rejected (`unsupported_session_scope`) | reads as `UNSPECIFIED`, rejected |
| `CompleteSessionResponse.scope` | response | display | response invalid (`unsupported_session_scope`) | reads as `UNSPECIFIED`, response invalid (`unsupported_session_scope`) |
| `GameSummary.platforms` | response | display | no platform shown | element removed |
| `VersionSummary.platforms` | response | display | no platform shown | element removed |
| `VersionSummary.kind` | response | display | response invalid | shown as unknown kind |
| `KeyFingerprint.role` | discovery | display | entry ignored | entry ignored |
| `ErrorInfo.reason` | error detail | client decision | fall back to the Connect code | fall back to the Connect code |
| `ResyncRequired.reason` | event | display | resynchronise | resynchronise |
| `SessionEnded.reason` | event | display | stream ended | stream ended |
| Signed documents (`Header.hash_algorithm`, `Chunking.algorithm`, `Entry.type`, `ReleaseDescriptor.kind`, `LaunchOption.platform`, `SetupStep.platform`) | signed binary | content | see the respective chapter | see the respective chapter |

Signed documents are binary only, so the JSON name rules do not apply to them; their unknown values are handled by the
rules of [manifest.md](manifest.md) and [descriptor.md](descriptor.md).

## 10. Reserved flows

The following flows are reserved. This revision defines no wire message for them, and a conforming server and client of
this revision do not implement them. A later revision that activates one of them keeps the properties stated here.

1. **Web-scope tokens.** `SESSION_SCOPE_WEB` is reserved and not issued.
2. **Pairing wake-up link.** The URL scheme `impluvium` is reserved. No link is defined or registered, and a link would
   never carry a credential.
3. **Store login ticket.** A model for opening the server's store pages in a separate client window with a restricted
   session. It is its own flow, separate from depot tickets: it uses neither the depot-ticket claims nor the depot
   revocation promise. Its properties:
   - single use, consumed atomically by the server;
   - short-lived (the model assumes a lifetime of at most 60 seconds);
   - bound to device and user, and refused after the device has been revoked;
   - stored by the server only as a hash, and rate-limited;
   - never placed in a URL query or fragment, and never visible to page script: the native client redeems it itself
     with an `https` `POST` that follows no redirect;
   - it yields a restricted session type that only the server determines; that session never gains administrative,
     account, token, multi-factor or pairing-approval rights, and it ends with the device's revocation.
