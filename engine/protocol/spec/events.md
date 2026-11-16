<!--
SPDX-FileCopyrightText: 2026 the impluvium contributors
SPDX-License-Identifier: 0BSD
-->

# Event stream

The server pushes changes to devices and to its web interface over one Server-Sent Events (SSE) channel. Events carry an
epoch and a sequence number so that a client can detect gaps, resume from the state it has actually applied, and
rebuild its state through RPC when resuming is not possible. Schema:
[`impluvium/v2/events.proto`](../proto/impluvium/v2/events.proto).

This chapter defines the wire contract and the rules a client needs to stay consistent. The server's ring buffer,
queue sizes, retention times and the browser-side coordination are implementation choices within these rules.

## 1. Opening the stream

```
GET {base}/api/v2/events?topics=library[&client=<id>][&last_event_id=<epoch>-<seq>]
Authorization: Bearer <access_token>        (devices)
Cookie: …                                    (the server's web interface)
```

The query string is parsed as `application/x-www-form-urlencoded` (WHATWG URL Standard). Parameters:

| Parameter | Rule | Error |
|---|---|---|
| `topics` | Required. Comma-separated list of 1 to 8 topics from §4, without empty items, whitespace or repetitions | 400 (`invalid_topic`) |
| `client` | Client identifier: 1 to 64 characters from `A`–`Z`, `a`–`z`, `0`–`9`, `_`, `-`. Required for cookie-authenticated streams, optional for bearer streams (§9) | 400 (`invalid_client_id`) |
| `last_event_id` | Resume point (§6) | a malformed value leads to `ResyncRequired(MALFORMED)`, not to an error status |

Each of these parameters occurs at most once; a repeated parameter is refused with 400 (`invalid_parameter`). Other
parameters are ignored. The **resume point** is the value of `last_event_id` if that parameter is present; otherwise the
value of the `Last-Event-ID` request header if exactly one such header field is present; otherwise there is none. More
than one `Last-Event-ID` header field is refused with 400 (`invalid_parameter`).

The server checks, in this order; the first failing step determines the result:

| Order | Condition | Status |
|---:|---|---|
| 0 | Common GET dispatch checks ([api.md §1.1](api.md#11-common-get-dispatch-checks)) | as defined there |
| 1a | Missing or ambiguous credential form (rules below) | 401 (`unauthenticated`, no wire reason) |
| 1b | Invalid, expired or revoked credential; for cookie streams also an `Origin` other than the server's own origin | 401 |
| 2a | A repeated parameter, or more than one `Last-Event-ID` header field | 400 (`invalid_parameter`) |
| 2b | `topics` missing or not valid (table above) | 400 (`invalid_topic`) |
| 2c | `client` not valid, or missing on a cookie-authenticated stream | 400 (`invalid_client_id`) |
| 3 | A topic the caller may not read | 403 |
| 4 | `client` is present, its supersession key is new, and the client-identifier table is full (§9) | 429 |

At step 1a, select the credential form by header presence, before validating either credential:

1. Reject more than one `Authorization` header field, more than one occurrence of the server's session cookie, or an
   `Authorization` header together with that cookie, even if either credential is invalid. Unrelated cookies do not
   count as a session credential.
2. With exactly one `Authorization` header and no session cookie, require the Bearer scheme and apply
   [proofs.md §2.4](proofs.md#24-token-use-and-device-state) at step 1b; never fall back to a cookie.
3. With no `Authorization` header and exactly one session cookie, apply the cookie-session and `Origin` checks at
   step 1b. With neither form, reject at step 1a.

No rejected open request creates a client-identifier entry or supersedes a stream. Only after all opening checks
pass is the accepted credential form used for the owner in §9 item 2 and supersession performed.

Error responses use the error body of [errors.md §3](errors.md#3-error-bodies). Session cookies used for the stream
carry `SameSite`. On success the response is `200` with `Content-Type: text/event-stream`, `Cache-Control: no-cache`,
`X-Accel-Buffering: no` and no `Content-Encoding`.

## 2. Framing

1. The server first sends a `retry:` field with its reconnection delay in milliseconds.
2. Each event consists of: an `id:` line for data events and heartbeats only (§3); an `event:` line whose value is the
   **event name**; exactly one `data:` line with the ProtoJSON encoding of `ServerEvent` on a single line; and an empty
   line.
3. The event name identifies the payload. It is the schema name of the `payload` member of `ServerEvent`:
   `connected`, `heartbeat`, `resync_required`, `session_ended` and `superseded` are **control events**,
   `library_changed` is a **data event**. Any other name is an **unknown event**, a kind added by a later revision
   (§7). In the JSON of `data` there is no `payload` object: the payload member appears at the top level under either of
   its two JSON names (for example `libraryChanged` or `library_changed`).
4. Lines starting with `:` are comments; the server MAY send them, clients ignore them.
5. One event, all its lines included, is at most 65 536 bytes (`event_too_large`).
6. The `data` JSON follows [encoding.md §3](encoding.md#3-json-profile).

## 3. Epochs and sequence numbers

1. The **epoch** is 16 bytes from a cryptographically secure random generator, chosen when the server process starts.
   A restarted server has a new epoch.
2. **User sequence numbers** are per (user, epoch). The first data event of a user in an epoch has sequence 1; every
   further data event of that user, in any topic, has the next integer. Sequence numbers never wrap. When the sequence
   would exceed 18 446 744 073 709 551 615, the server emits no further data event for that user and performs no state
   change that would require one.
3. The SSE `id` is the lowercase hexadecimal epoch (32 digits), `-`, and the sequence number in decimal without leading
   zeros: `<epoch_hex>-<seq>`.
4. **Stream chain.** For each open stream the server keeps `last_written`: the accepted resume point if the stream was
   resumed, otherwise `Connected.head_seq`. A data event written to the stream carries `seq` = its user sequence number
   and `prev_seq` = `last_written`; then `last_written` becomes its `seq`. Data events are written in ascending `seq`.
   Events that the stream does not receive (other topics, update events for entries the caller may no longer read,
   §5 item 2) do not enter the chain, so they never cause a gap.
5. **Heartbeats** carry `seq = last_written` and `prev_seq = 0`, and an `id` line with that `seq`. While the stream is
   backlogged, `last_written` may be behind the user's head.
6. Other control events carry `seq = 0`, `prev_seq = 0` and no `id` line.

## 4. Topics and payloads

| Topic | Payload | Who may subscribe |
|---|---|---|
| `library` | `LibraryChanged` | any authenticated user, for their own library |
| `notifications`, `devices`, `tasks`, `system` | reserved; no payload is defined in this revision | requests for them are refused with 400 (`invalid_topic`) |

### 4.1 `ServerEvent`

| No. | Field | Type | Presence | Limits | Meaning |
|---|---|---|---|---|---|
| 1 | `epoch` | `bytes` | required | exactly 16 bytes | Epoch of the producing server process. |
| 2 | `seq` | `uint64` | conditional (§3): ≥ 1 for data events; `last_written` for heartbeats; 0 otherwise | – | Sequence number. |
| 3 | `time` | `Timestamp` | required | – | Server time of the event. |
| 4 | `prev_seq` | `uint64` | conditional (§3): `last_written` before the event for data events; 0 otherwise | – | Previous entry of the stream chain. |
| 10 | `connected` | `Connected` | one member of `payload` | – | First event of every stream. |
| 11 | `heartbeat` | `Heartbeat` | one member of `payload` | – | Liveness and progress. |
| 12 | `resync_required` | `ResyncRequired` | one member of `payload` | – | Rebuild state through RPC. |
| 13 | `session_ended` | `SessionEnded` | one member of `payload` | – | Terminal: the credential ended. |
| 14 | `superseded` | `Superseded` | one member of `payload` | – | Terminal: replaced by a newer stream. |
| 20 | `library_changed` | `LibraryChanged` | one member of `payload` | – | Data event of topic `library`. |

Each event carries exactly one member of `payload`, the one named by its event name (§2 item 3). Since the members
appear at the top level of the JSON, the rules for recognising them are part of the receive validation (§7 step 5).

### 4.2 Control payloads

`Connected`:

| No. | Field | Type | Presence | Limits | Meaning |
|---|---|---|---|---|---|
| 1 | `head_seq` | `uint64` | optional (0: no data event yet in this epoch) | – | The user's highest sequence number in this epoch when the stream opened. |
| 2 | `resumed` | `bool` | optional (false: not resumed) | – | True if the resume point was accepted and the missed events follow. |

`Heartbeat` and `Superseded` have no fields.

`ResyncRequired`:

| No. | Field | Type | Presence | Limits | Meaning |
|---|---|---|---|---|---|
| 1 | `reason` | `ResyncReason` | optional (unknown or `UNSPECIFIED`: resynchronise anyway) | – | Why a rebuild is needed. |

`ResyncReason`:

| Value | Name | Meaning | Stream afterwards |
|---:|---|---|---|
| 0 | `RESYNC_REASON_UNSPECIFIED` | No reason given. | open |
| 1 | `RESYNC_REASON_TOO_OLD` | The resume point is older than the retained events. | open |
| 2 | `RESYNC_REASON_AHEAD` | The resume point is beyond the user's head. | open |
| 3 | `RESYNC_REASON_FOREIGN_EPOCH` | The resume point belongs to another epoch. | open |
| 4 | `RESYNC_REASON_MALFORMED` | The resume point could not be parsed. | open |
| 5 | `RESYNC_REASON_OVERFLOW` | The server dropped events for this stream. | closed |
| 6 | `RESYNC_REASON_ACCESS_CHANGED` | The caller lost permission for a subscribed topic (§5 item 2). Reopening the stream with that topic is refused with 403. | closed |

`SessionEnded`:

| No. | Field | Type | Presence | Limits | Meaning |
|---|---|---|---|---|---|
| 1 | `reason` | `SessionEndReason` | optional (unknown or `UNSPECIFIED`: the stream ended) | – | Why the credential ended. |

`SessionEndReason`:

| Value | Name | Meaning |
|---:|---|---|
| 0 | `SESSION_END_REASON_UNSPECIFIED` | No reason given. |
| 1 | `SESSION_END_REASON_REVOKED` | The credential or its device was revoked. |
| 2 | `SESSION_END_REASON_EXPIRED` | The credential expired. |

### 4.3 `LibraryChanged`

| No. | Field | Type | Presence | Limits | Meaning |
|---|---|---|---|---|---|
| 1 | `game_id` | `string` | required | canonical UUID text | Changed library entry. |
| 2 | `deleted` | `bool` | optional (false: the entry exists) | – | True: the entry left the user's library (deletion marker). |
| 3 | `game` | `GameSummary` | conditional: present if and only if `deleted` is false, checked in §7 step 10 | [api.md §6.3](api.md#63-libraryservice); `game.game_id` equals `game_id` and `game.entry_version` equals the event's `seq`, checked in §7 step 10 | New state of the entry. |

## 5. Server obligations

1. The server checks the stream's credential before every wait and immediately before writing every event, replayed or
   live. When the credential has been revoked or has expired, it sends `SessionEnded` with the reason, writes nothing
   else and closes the stream.
2. Immediately before writing a data event, replayed or live, the server checks the caller's current permissions:
   1. The caller may still read every topic of the stream. If not, the server sends `ResyncRequired(ACCESS_CHANGED)`
      and closes the stream; a new stream with that topic is refused with 403.
   2. For a `library_changed` event with `deleted = false`, the caller may still read the entry. If not, the event is
      not written and does not enter the stream chain (§3 item 4).
   3. A deletion marker (`library_changed` with `deleted = true`) needs no permission on the entry: it is written to
      every stream of its user whose topics include `library`. It carries only `game_id` and `deleted`, the identity
      needed to remove the entry, and never `game`.
3. Every change of a user's library is recorded as data events in that user's sequence, whatever causes it: an entry
   that leaves the library (deletion, loss of access through access rules, age rating, group membership or any other
   rule) gets a deletion marker; an entry that enters the library or changes gets a `library_changed` event with
   `game`. The events are recorded in the same state change that changes the library, so no reader sees the new state
   without them. Therefore every update event that item 2.2 suppresses is followed, later in the sequence, by a deletion
   marker for the same entry. There is no unsequenced resynchronisation for library changes: a change of many entries
   produces many events, and the retention and queue limits then lead to `ResyncRequired(TOO_OLD)` on resume or
   `ResyncRequired(OVERFLOW)` on a live stream (§6, item 5), which force a rebuild.
4. It sends a heartbeat at least every 30 seconds while the stream is open.
5. Each stream has a bounded queue. If the server has to drop an event for a stream, it sends
   `ResyncRequired(OVERFLOW)` and closes that stream.
6. It replays only events of the current epoch and only for the stream's topics, subject to item 2 (deletion markers
   included).

## 6. Resuming

A client resumes only from its contiguously **applied** state (§8), never from the last id it has merely received. The
resume point is `<epoch_hex>-<seq>` of the last applied data event, or `<epoch_hex>-0` if it applied none in that epoch.

A resume point is **well-formed** if it is at most 53 bytes, matches `[0-9a-f]{32}-(0|[1-9][0-9]*)` and its decimal
part is at most 18 446 744 073 709 551 615. The server checks length and syntax on the text before it converts the
number; a longer text, other syntax and a larger number are all `MALFORMED`.

| Resume point | Server reaction |
|---|---|
| none | `Connected(resumed = false)`, then live events. |
| not well-formed | `Connected(resumed = false)`, `ResyncRequired(MALFORMED)`, then live events. |
| other epoch | `Connected(resumed = false)`, `ResyncRequired(FOREIGN_EPOCH)`, then live events. |
| `seq` greater than the user's head | `Connected(resumed = false)`, `ResyncRequired(AHEAD)`, then live events. |
| some event with a sequence number greater than `seq` is no longer retained | `Connected(resumed = false)`, `ResyncRequired(TOO_OLD)`, then live events. |
| otherwise | `Connected(resumed = true)`, the retained events with sequence greater than `seq` that §5 lets the stream receive, in order, then live events. |

After a failed resume the stream stays open for live events; the client rebuilds its state through RPC (§8) and merges
the live events. Only `OVERFLOW`, a lost topic permission (§5 item 2) and the terminal events close the stream. Clients
reconnect themselves with a jittered backoff and do not rely on an automatic reconnection of their SSE library, because
such a reconnection would resume from the last received id.

## 7. Receive validation

A client validates every event before it uses it, in this order; this order is complete for `ServerEvent` and its
payloads. A failure makes the stream **broken**: the client closes it, reconnects with backoff and resumes from its
applied state. A broken stream never deletes credentials.

| Order | Check | Reason |
|---:|---|---|
| 1 | Event size (§2 item 5) | `event_too_large` |
| 2 | Framing: exactly one `event:` line with a non-empty name and exactly one `data:` line | `event_malformed` |
| 3 | The `data` JSON passes the JSON profile and the limits of `ServerEvent` ([encoding.md §3.10](encoding.md#310-check-order)); members that `ServerEvent` does not define are unknown and skipped | profile reasons, including `oneof_conflict` for two known payload members |
| 4 | The first event of the stream is `connected`, and only the first | `event_out_of_order` |
| 5 | Payload member: for a known event name, the member of that name is present (`missing_field`) and no other known payload member is present (`event_name_mismatch`); for an unknown event name, no known payload member is present (`event_name_mismatch`) | as listed |
| 6 | Field rules only (stage 1 of the generic order, [encoding.md §7.2](encoding.md#72-the-generic-order)) of the fields `epoch`, `seq`, `time`, `prev_seq` and of the known payload, including the messages nested in it, in this field order; the conditions on `seq`, `prev_seq` and `game` are left to steps 8 to 10 | as listed there |
| 7 | `epoch` equals the epoch of the stream's `connected` event | `event_epoch_mismatch` |
| 8 | `id:` line: present for `library_changed` and `heartbeat`, absent for the other known control events, optional for unknown events; when present, its epoch and number equal `epoch` and `seq` | `event_id_mismatch` |
| 9 | Data events (`library_changed`, and unknown events with an `id:` line): `seq ≥ 1`, `prev_seq < seq`, and `seq` greater than the `seq` of every earlier data event of the stream; heartbeats: `seq` not lower than the `seq` of the last data event received on the stream | `event_sequence_invalid` |
| 10 | `library_changed`: `game` present if and only if `deleted` is false (`event_payload_invalid`); `game.game_id = game_id` and `game.entry_version = seq` (`event_identity_mismatch`) | as listed |

Steps 8 and 9 are the only receive checks of `seq` and `prev_seq`. The other values that §3 items 4 to 6 prescribe are
server obligations that a receiver does not check: `prev_seq = last_written` of a data event (the receiver uses
`prev_seq` only in §8 item 2), `prev_seq = 0` of a heartbeat, and `seq = 0` and `prev_seq = 0` of the other control
events, whose `seq` and `prev_seq` do not affect the receiver's state.

**Unknown events.** An unknown event with an `id:` line is a data event of a kind the client does not know: after
checks 1 to 9 it advances the client's applied state under §8 item 2 and has no other effect. An unknown event without
an `id:` line is a control event the client does not know: it is ignored after checks 1 to 8. In both cases the member
that carries the unknown payload is an unknown JSON member and is skipped by check 3.

## 8. Library snapshot and merge

`ListLibrary` pages carry `epoch` and `snapshot_seq`; each `GameSummary` carries `entry_version`, the user sequence number
of the entry's last change in that epoch (0 if it has not changed in the epoch).

A client that maintains a library model follows these rules:

1. **Stream first.** It opens the stream before it loads pages, and it buffers events while it builds.
2. **Applied state.** It keeps the epoch of the stream and the last applied sequence number `a`. A data event with
   `seq ≤ a` is a duplicate and is ignored. A data event with `prev_seq ≤ a < seq` is applied, and its `seq` becomes
   the new `a`. A data event with `prev_seq > a` reveals a gap: an earlier event of the stream chain has not been
   applied. The client holds it in a bounded reorder buffer until the gap closes.
3. **Heartbeat.** A heartbeat with `seq` above `a` reveals that an event of the stream chain was lost.
4. **Rebuild.** A gap that does not close, a lost event revealed by a heartbeat, `ResyncRequired`, and the start without
   a baseline all trigger the same rebuild: load all pages into a new model, then merge the buffered and further events.
   The rebuild works on separate data and replaces the current model only after it completed; on failure the old model
   stays in place, is marked stale, and the rebuild is retried with backoff. Each request and the whole rebuild have
   time limits, and the number of pages and buffered events is bounded; these budgets are client policy.
5. **Versioned merge.** For each entry the client keeps the highest version it has seen, including for deleted entries
   (deletion marker with version). A page entry is applied only if its `entry_version` is higher than the known version
   of that entry; an event is applied only if its `seq` is higher. A deletion marker hides the entry until a later
   version re-creates it.
6. **Epochs.** The stream epoch and the snapshot epoch are tracked separately. A page whose `epoch` differs from the
   stream epoch invalidates the running rebuild, which restarts. A `Connected` event with a new epoch invalidates a
   running rebuild and the applied state; the client then rebuilds in the new epoch. Without a baseline, the client
   connects without a resume point and adopts the epoch of the first `Connected`.
7. The first page's `snapshot_seq` becomes `a` of the new model, so that the merged events continue from it.

## 9. Supersession

1. The server limits the number of streams per client identifier: a new stream with the same **supersession key**
   replaces the older stream, which receives `Superseded` and is closed. This also interrupts a replay in progress and
   any pause between writes.
2. The supersession key is the pair (owner, client identifier). For a bearer stream the owner is the `device_id` of the
   device the access token belongs to, whatever token of that device is used; for a cookie stream the owner is the
   server's session id of the cookie session.
3. Consequently two bearer streams of the same device with the same client identifier supersede each other, also when
   they use different access tokens; two tabs of one cookie session with the same identifier supersede each other.
   Streams of different devices, of different cookie sessions, of a device and a cookie session, and of different users
   never supersede each other, whatever their identifiers.
4. A bearer stream without `client` is never superseded and supersedes nothing.
5. The server keeps at most 256 client-identifier entries per user; when the table is full, a new supersession key is
   refused with 429. An existing key reuses its entry; a bearer stream without `client` needs no entry. An entry is
   removed only by the stream that currently owns it, so a superseded stream never removes the entry of its successor.
6. The client identifier limits connections. It never grants a permission.

## 10. Client obligations

1. On `SessionEnded` the client closes the stream itself and does not reconnect with the same credential. It confirms
   its device state with its next RPC; the event alone never deletes credentials ([errors.md §7](errors.md#7-credential-cleanup)).
2. On `Superseded` the client closes the stream and does not reconnect with the same client identifier until its own
   coordination makes it the active stream again.
3. On `ResyncRequired(ACCESS_CHANGED)`, and when opening a stream is refused with 403 for a topic, the client discards
   the state it keeps for that topic; it builds that state again only after a stream for the topic is accepted.
4. A browser client coordinates its tabs so that one tab holds the stream (for example with Web Locks where available,
   otherwise with an election over `BroadcastChannel` with a lease). After `Superseded` a tab gives up leadership and
   waits at least two lease periods, growing with repeated supersession, before it competes again. *(informative)*
