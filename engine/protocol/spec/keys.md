<!--
SPDX-FileCopyrightText: 2026 the impluvium contributors
SPDX-License-Identifier: 0BSD
-->

# Keys

## 1. Key roles

| Role | Holder of the private key | Public key known to | Signs | Rotation in this revision |
|---|---|---|---|---|
| Content | Server (derived from the instance key) | Paired devices (pinned at pairing) | Manifests, release descriptors | None |
| Depot ticket | Server (derived from the instance key, one key per generation) | The depot | Depot tickets | Generation switch (§4) |
| Device | The device (generated on the device) | The server (registered at pairing) | Session proofs, pairing proofs | None |

A key is trusted for exactly one role. A signature by a key of another role never validates a document: the trusted key
set passed to the verifier contains only keys of the role that the expected payload type requires
([envelope.md §2](envelope.md#2-payload-types)).

All keys are Ed25519 keys (RFC 8032). A private key is its 32-byte seed; the public key is derived from the seed as in
RFC 8032 §5.1.5.

## 2. Derivation of server keys

### 2.1 Instance key

Each server instance has one **instance key**: 32 bytes from a cryptographically secure random generator, created once
when the instance is initialised. Creating, storing and backing up the instance key is outside this specification. The
instance key is the single root of the server's signing keys.

### 2.2 Derivation function

A signing seed is derived as

```
seed = HKDF-SHA-256(IKM = instance_key, salt = "", info = label, L = 32)
```

using HKDF as defined in RFC 5869 with SHA-256: `PRK = HMAC-SHA-256(salt, IKM)`, `seed = T(1) = HMAC-SHA-256(PRK, info || 0x01)`.
The salt is the empty byte string; by RFC 5869 §2.2 this is equivalent to a salt of 32 zero bytes, and both give the
same `PRK`. `info` is the label's ASCII bytes without a terminator. The 32-byte output is the Ed25519 seed; the public
key follows from RFC 8032 §5.1.5 and the `key_id` from [envelope.md §8.2](envelope.md#82-key_id).

### 2.3 Label registry

| Role | Label | Notes |
|---|---|---|
| Content | `impluvium/content-signing-key/v1` | One fixed label. The content key does not rotate in this revision. |
| Depot ticket, generation `g` | `impluvium/depot-ticket-signing-key/v1/` followed by `g` in decimal | `g` is an integer from 1 to 4 294 967 295 written without leading zeros, for example `impluvium/depot-ticket-signing-key/v1/1`. |

Rules:

1. Labels are compared as bytes. Every label in this registry is distinct from every other label, and every ticket
   generation produces a distinct label.
2. Any other use of the instance key (for example server-internal encryption keys) MUST use labels that are not in this
   registry and that do not begin with `impluvium/content-signing-key/` or `impluvium/depot-ticket-signing-key/`.
3. The label version (`v1`) changes only with a specification change.

Derivation vectors (instance key bytes, label, seed, public key, `key_id`) are part of the known-answer vectors. This
specification contains no computed seeds or keys.

### 2.4 Device keys

A device generates its Ed25519 key pair locally from a cryptographically secure random seed. The private seed never
leaves the device. The server stores only the public key. Device key rotation is not part of this revision; a new key
requires a new pairing.

## 3. Trusted key sets

Every verification receives a trusted key set from its caller. The set comes from a trusted local source (the pinned
trust context of [trust.md](trust.md), the device registry, or the depot configuration). The envelope, discovery and
any other received data never add a key to it.

| Expected payload type | Role | Required size |
|---|---|---|
| manifest, release descriptor | content | exactly 1: the pinned content key |
| session proof, pairing proof | device | exactly 1: the public key registered for the expected device or pending pairing |
| depot ticket | depot ticket | 1 to 8 keys (§4) |

Before reading any input, the verifier checks the set. A failure is a **configuration error**: the verifier refuses to
verify, reports the configuration reason locally and never chooses a key arbitrarily. Configuration errors never appear
on the wire.

| Check | Configuration reason |
|---|---|
| The set has the required size for the role | `trusted_set_size` |
| Every key is 32 bytes and passes the point checks of [envelope.md §4.2](envelope.md#42-acceptance-profile) step 2 | `invalid_trusted_key` |
| Every key belongs to the role of the expected payload type | `role_mismatch` |
| No two keys share a `key_id` (this includes the same key listed twice) | `trusted_key_id_collision` |
| Depot ticket sets: at most one key has status *current*; every *retiring* key has a switch time | `invalid_ticket_key_set` |

These configuration errors identify diagnostic field `""`; the caller's key set is not a schema field. Applying the
same point check to a received schema field instead retains the field assigned by its chapter.

## 4. Depot-ticket key generations

### 4.1 Acceptance set

The depot verifies tickets against an **acceptance set** of 1 to 8 depot-ticket public keys. Each entry has:

- the public key of one generation;
- a status: *current* or *retiring*;
- for *retiring* keys, the switch time `t_switch` at which the key stopped issuing tickets.

The acceptance set has a **set generation**, an unsigned 64-bit counter that increases with every change of the set
(adding a key, retiring a key, removing a key, hard revocation).

### 4.2 Switching generations

A switch from generation `g` to a new generation `g'` at time `t_switch`:

1. derives the key of `g'` (§2.3) and adds it with status *current*;
2. changes the key of `g` to *retiring* with `t_switch`;
3. from `t_switch` on, only the current key issues tickets; no ticket with `iat ≥ t_switch` is ever issued under a
   retiring key;
4. keeps the retiring key in the acceptance set at least until `t_switch + max_ttl`, where `max_ttl` is the maximum
   ticket lifetime of 3 600 seconds ([tickets.md §3](tickets.md#3-claims-rules)). At or after that time it MAY be
   removed; every ticket it signed has expired by then, because `exp ≤ iat + max_ttl < t_switch + max_ttl`.

A further switch MUST NOT displace a retiring key before its `t_switch + max_ttl`. If the set already holds 8 keys, the
switch waits until a key may be removed, or it is executed explicitly as a hard revocation (§4.4) of the key it
displaces.

### 4.3 Verification rule

The generation rule applies to **every** signature of the ticket whose `key_id` belongs to the acceptance set and whose
signature is valid, not only to one of them. In envelope order, for each such signature by a *retiring* key, the
verifier requires `claims.iat < t_switch` of that key; the first signature that violates this rejects the whole ticket
(`ticket_key_retired`), even if another valid signature comes from the current key. This mirrors the envelope rule that
a known key with an invalid signature rejects the envelope ([envelope.md §5](envelope.md#5-multiple-signatures)). A
ticket is never accepted after its own `exp`, whatever the state of its keys.

The diagnostic field for `ticket_key_retired` is `DepotTicketClaims.iat`, as in tickets.md §4.

### 4.4 Hard revocation

Hard revocation removes a key from the acceptance set immediately, increases the set generation and invalidates every
cache entry that the key validated. Tickets signed only by a revoked key are rejected from then on
(`no_trusted_signature`).

### 4.5 Responsibilities outside this specification

Persisting generations and switch times, the operational switch, the atomic replacement of the acceptance set and the
issuance policy belong to the server. This specification defines the acceptance rules, the overlap rule and the cache
invariants; the vectors cover verification before, during and after an overlap, a ticket with `iat` after its key's
switch time, repeated switches with a full set, hard revocation and a cache hit after a set change.

## 5. Ticket verification cache

A depot MAY cache successful ticket verifications. A cache is an optimisation only: a cache hit MUST give exactly the
result that a full verification would give at the time of the hit.

1. An entry binds: the exact ticket bytes (the binary envelope), the `key_id`s of all signatures that were found valid
   under keys of the acceptance set, the role, and the set generation of the acceptance set used.
2. A hash of the ticket bytes MAY serve as the lookup index only. A hit requires byte-for-byte equality of the stored
   ticket bytes with the presented ticket bytes, equality of the role, and equality of the stored set generation with
   the current one. Equality of the hash alone is never a hit.
3. On every hit the verifier re-checks the time window of the cached claims ([tickets.md §4](tickets.md#4-verification))
   at the current time, including the retiring-key rule for every cached `key_id`, and re-applies the per-request
   checks (audience, subject and scope).
4. A change of the acceptance set (any increase of the set generation) invalidates every entry created under an older
   generation. A hard revocation invalidates at least the entries of the revoked key.
5. An entry is never used at or after the `exp` of its ticket.
