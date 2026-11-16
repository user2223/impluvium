<!--
SPDX-FileCopyrightText: 2026 the impluvium contributors
SPDX-License-Identifier: 0BSD
-->

# HTTP rules for clients

These rules apply to every HTTP request that a desktop client, the engine or the command-line client sends to a server
or depot, on every HTTP stack the client uses. All stacks of one client take their TLS configuration, origin rules,
redirect rules and credentials from one shared rule source, so that no stack is more permissive than another.

## 1. Requests a client makes

| Purpose | Destination | Credential | Redirects |
|---|---|---|---|
| Discovery | `{base}/api/info` | none | none |
| RPC | `{base}/api/v2/impluvium.v2.*` | access token, except the four methods of [api.md §5](api.md#5-connect-unary-calls) item 6 | none |
| Manifest, descriptor, object | `{base}/api/v2/{manifests,descriptors,objects}/…` | access token | none |
| Events | `{base}/api/v2/events` | access token | none |
| Chunks | `{depot_base_url}/depot/v2/…` of an allowed depot endpoint (§5) | depot ticket | at most 5, same origin and allowed endpoint (§7) |

## 2. Base URL and origins

1. A **base URL** is an absolute URL with scheme `https` or `http`, without user information, query or fragment. Its path
   is either empty or a prefix that does not end with `/`; a client removes one trailing `/` from user input before it
   stores the base URL.
2. URLs are parsed with the WHATWG URL Standard. The **origin** of a URL is the triple (scheme, host, port) of that
   standard, with the port replaced by 443 for `https` and 80 for `http` when the URL has none. Hosts are compared in
   the serialised form that the standard produces (lowercase ASCII, internationalised names in their ASCII form,
   normalised IP addresses).
3. The **server origin** is the origin of the stored base URL.
4. URLs with user information, URLs with another scheme and URLs that do not parse are rejected before any connection
   (`invalid_url`).

### 2.1 Request preflight order

For every request, use only public configuration, the request purpose and existing trust state to check, in order:

1. The base URL and request URL satisfy §2 (`invalid_url`). The request URL may contain the query its route defines.
2. The platform certificate verifier is available (§4 item 4: `platform_trust_unavailable`), also for `http` requests.
3. The scheme is permitted (§3 item 2: `insecure_scheme_not_allowed`).
4. The destination matches the request purpose of §1 and the credential destinations of §5 item 1
   (`origin_not_allowed`); for a chunk request, it belongs to an allowed depot endpoint (§5 items 2 and 3).
5. For a request that would carry a proof, token or ticket, the trust context is not in a stop state
   ([trust.md §3](trust.md#3-states): `instance_mismatch` or `content_key_changed`).

The first failure determines the result. No connection is opened and no credential is read or proof generated until
all these checks pass and the expected response schema and limits are bound (§6 item 1).

Then proceed under §2.2.

### 2.2 Connection and credential transfer

After the preflight of §2.1 passes, proceed in this order:

1. Establish or reuse a connection without attaching credentials.
2. Check its verified state. An unverified server connection is pending, not a new rejection reason: before reading
   a credential or generating a proof for it, complete credential-free discovery and the checks of
   [trust.md §4](trust.md#4-checks-before-every-credential-transfer) on that connection. Pairing uses the discovery
   checks of [trust.md §2](trust.md#2-establishing-a-trust-context), with no pin comparison yet. Discovery itself needs
   no verified connection. A depot connection needs no pin checks, but its ticket must have the provenance required
   by [trust.md §4.2](trust.md#42-verified-connections) item 5; if not yet met, obtain an authenticated download response
   first, under these same request rules.
3. Only after these prerequisites pass, read the credential or generate the proof, attach it and send the request.

## 3. Encryption

1. `https` is the default.
2. `http` is allowed only (a) for a host that is `localhost`, ends in `.localhost`, or is an address in 127.0.0.0/8 or
   `::1`, or (b) when the user has explicitly enabled the insecure local-network mode for this server in the client
   configuration. In every other case a request to an `http` URL is refused before connecting
   (`insecure_scheme_not_allowed`). The same rule applies to depot origins.
3. Signatures protect the integrity of manifests and descriptors over `http`; access tokens and tickets are not
   confidential over `http`. [security.md](security.md) states this.

## 4. TLS

1. TLS 1.2 or 1.3. Application protocols HTTP/1.1 and HTTP/2.
2. Server certificates are verified by the platform's certificate verifier, configured once and shared by all stacks.
   Certificate authorities that the user added to the client configuration are verified by the same verifier. There is
   no option to accept invalid certificates.
3. Revocation semantics follow the platform verifier: on Windows a recognised revocation of the end-entity certificate
   rejects the connection and an unknown revocation status is tolerated; on Linux no revocation check takes place.
4. At §2.1 step 2, if the platform verifier is not available, the request fails before any connection and before any
   credential is read, with the typed error `platform_trust_unavailable`. The client then creates no files, starts no
   processes, changes no profiles and does not fall back to another trust source.

## 5. Depot origins and credentials

1. Credentials come from one credential source per request. The purpose of the request decides which credential is
   attached and where it may go. URLs are compared in the serialised form of the WHATWG URL Standard.
   - An **access token** is attached only to a request to the server origin whose path begins with the path of the base
     URL followed by `/api/v2/`, and only on a verified connection ([trust.md §4.2](trust.md#42-verified-connections)).
     Discovery (`{base}/api/info`) never carries a credential.
   - A **depot ticket** is attached only to a request whose URL is exactly
     `{depot_base_url}/depot/v2/{manifest_id_hex}/{chunk_id_hex}` for an allowed depot endpoint of the current download
     (item 2), where `{manifest_id_hex}` names a manifest of that download's manifest set, or to a redirect target that
     §7 allows. The path prefix of the endpoint's base URL is part of this comparison.
   A depot ticket is never attached to an API request, and an access token is never attached to a depot request, also
   when depot and server share an origin.
2. The **allowed depot endpoints** of a download are the `depot_endpoints` of its authenticated `OpenDownloadResponse`
   whose origin is one of the depot origins that the client configuration allows. By default the configuration allows
   only the server origin. A server response alone never extends the configuration.
3. Before the first connection to a depot origin and before a ticket is read or attached, the client checks that the
   request URL belongs to an allowed depot endpoint (§2.1 step 4). A request that does not is refused before connecting
   (`origin_not_allowed`).
4. Removing an origin from the configuration takes effect before the next request to it.
5. Among allowed endpoints, the client prefers lower `priority` values and keeps the list order for equal values.

## 6. Response protection

A client never hands a response body to a general-purpose decoder before these checks have passed.

1. **Binding before sending.** Before a request is sent, the client binds it to the expected response schema and the
   limits of [limits.md](limits.md) for that route or method. A method without an entry has no fallback limit and is not
   called. Response headers never widen a schema or a limit.
2. **Status and response head.** Each response head received before the body is at most 65 536 bytes under §6.1
   (`headers_too_large`, diagnostic field `""`).
   A `3xx` status on a route without redirects is an error (`redirect_not_allowed`).
3. **Content coding.** A `Content-Encoding` other than `identity` is rejected (`unsupported_content_encoding`). No
   decompression happens.
4. **Content type.** The `Content-Type` (without parameters) must match the route: `application/proto` or
   `application/json` for an RPC success in the codec of the request; `application/json` for errors and discovery;
   `application/octet-stream` for envelopes and chunks; `text/event-stream` for an opened event stream. An object
   response keeps its stored type and is checked by the caller. Otherwise the response is a broken body
   ([errors.md §6](errors.md#6-responses-without-a-valid-error-body)).
5. **Body.** The body is read under a deadline and the route's byte limit for its expected success status, or the
   65 536-byte error-body limit otherwise. Received bytes are counted; a `Content-Length` header neither replaces the
   count nor raises the limit. A body that exceeds its limit is abandoned: `response_too_large` for a success status,
   otherwise the status mapping of [errors.md §6](errors.md#6-responses-without-a-valid-error-body).
6. **Pre-check.** The body passes the encoding profile and the element, byte and nesting limits of its schema
   ([encoding.md](encoding.md)) before decoding. Error bodies use their own schema ([errors.md §3](errors.md#3-error-bodies)),
   independent of the codec of the request.
7. **Semantic checks.** Timestamps and the field rules of the schema are checked before the decoded value reaches the
   caller.

### 6.1 Response-head size

The 65 536-byte limit applies separately to every response head received before the body: each informational (`1xx`)
head and the final head. It is not a combined allowance for several heads. Exceeding it yields
`headers_too_large`, with diagnostic field `""`; neither the status nor an error body overrides this transport
failure ([errors.md §6](errors.md#6-responses-without-a-valid-error-body)).

- **HTTP/1.1 response head:** count every received octet from the first octet of the status line through the CRLF of
  the empty line terminating the head: the status line, all field lines and all line endings. Do not remove or
  normalise whitespace or line endings before counting. Including the status line prevents an unbounded status line
  and bounds the head the client must buffer before interpreting its fields.
- **HTTP/2:** use the header-list size of RFC 9113 §6.5.2 for each response-head block: for every decoded field line,
  add the name's octet length, the value's octet length and 32. Include pseudo-header fields and count repeated field
  lines separately, before any combination. A block includes all its HEADERS and CONTINUATION fragments; frame,
  compression and padding bytes do not enter this measure. Apply the same calculation to each informational or final
  response block received before the body.

*(Informative.) A client can advertise `SETTINGS_MAX_HEADER_LIST_SIZE = 65536`; advertising the setting does not
replace the local check.*

## 7. Redirects

1. Discovery, RPC, document and object routes and the event stream follow no redirect.
2. A chunk request follows at most 5 redirects; a sixth redirect is refused (`redirect_limit`). The `Location` value is
   resolved against the request URL. Every redirect target MUST have the same origin as the original chunk request
   (`redirect_cross_origin`), so a change from `https` to `http` is never followed.
3. A redirect target MUST also be the depot request URL of an allowed depot endpoint of the same origin, for the same
   `{manifest_id_hex}` and `{chunk_id_hex}` (§5 item 1); otherwise the redirect is refused before it is followed
   (`redirect_target_not_allowed`). The ticket is attached to the redirected request under §5.

For each redirect, check in this order before any new connection or ticket read:

| Order | Check | Rejection |
|---:|---|---|
| 1 | Redirect count (§7 item 2) | `redirect_limit` |
| 2 | Resolve and validate the target URL (§2) | `invalid_url` |
| 3 | Same origin (§7 item 2) | `redirect_cross_origin` |
| 4 | Allowed target (§7 item 3) | `redirect_target_not_allowed` |
| 5 | Request preflight (§2.1) for the target | the first preflight failure |

## 8. Proxies

Clients connect directly. They ignore proxy environment variables (`HTTP_PROXY`, `HTTPS_PROXY`, `ALL_PROXY`,
`NO_PROXY` and their lowercase forms), operating-system proxy settings and proxy auto-configuration. No request is
sent to a proxy.

## 9. Identification

Clients send `User-Agent: <token>/<version> (<os>; <arch>)`, where `<token>` identifies the client component (for
example `impluvium-cli`), `<version>` is its release version and `<os>` and `<arch>` are lowercase labels such as
`linux` and `x86_64`. Servers MUST NOT use the user agent for any authorisation or session-type decision.
