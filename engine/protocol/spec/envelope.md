<!--
SPDX-FileCopyrightText: 2026 the impluvium contributors
SPDX-License-Identifier: 0BSD
-->

# Signed envelope

Every signed document of the protocol (manifest, release descriptor, depot ticket, session proof and pairing proof)
uses one envelope: the payload bytes, their media type and one or more Ed25519 signatures over the DSSE
pre-authentication encoding of both. Schema: [`impluvium/signed/v1/envelope.proto`](../proto/impluvium/signed/v1/envelope.proto).

## 1. Messages

### 1.1 `impluvium.signed.v1.Envelope`

| No. | Field | Type | Presence | Limits | Meaning |
|---|---|---|---|---|---|
| 1 | `payload_type` | `string` | required | 1 to 128 bytes, each byte in `0x21`–`0x7E` | Registered payload type (§2). |
| 2 | `payload` | `bytes` | optional (empty only if the payload type allows it; decided by the checks of the payload chapter in steps 12 and 13 of §7) | per payload type (§2) | The exact signed bytes. |
| 3 | `signatures` | repeated `Signature` | required | 1 to 8 elements | Signatures over `PAE(payload_type, payload)`. |

Envelope metadata, that is the length of the whole binary envelope minus `len(payload)`, is at most 16 384 bytes.

### 1.2 `impluvium.signed.v1.Signature`

| No. | Field | Type | Presence | Limits | Meaning |
|---|---|---|---|---|---|
| 1 | `key_id` | `bytes` | required | exactly 8 bytes | First 8 bytes of SHA-256 over the 32-byte public key (§8.2). |
| 2 | `sig` | `bytes` | required | exactly 64 bytes | Ed25519 signature `R || S` (§4). |

### 1.3 `impluvium.signed.v1.KeyRole`

| Value | Name | Meaning |
|---|---|---|
| 0 | `KEY_ROLE_UNSPECIFIED` | Never valid. |
| 1 | `KEY_ROLE_CONTENT` | Signs manifests and release descriptors. |
| 2 | `KEY_ROLE_DEPOT_TICKET` | Signs depot tickets. |
| 3 | `KEY_ROLE_DEVICE` | Device key; signs session and pairing proofs. |

`KeyRole` appears on the wire only in `KeyFingerprint.role` ([api.md §2](api.md#2-discovery)); an unknown value there
makes the fingerprint entry ignored.

## 2. Payload types

The payload type registry of this revision. A payload type is compared byte for byte; there is no case folding and no
parameter syntax.

| Payload type | Bytes | Payload message | Signing role | Max payload | Max whole envelope |
|---|---:|---|---|---:|---:|
| `application/vnd.impluvium.manifest.v3+protobuf` | 46 | `impluvium.manifest.v3.Manifest` | content | 536 870 912 | payload + 16 384 |
| `application/vnd.impluvium.release.v1+protobuf` | 45 | `impluvium.manifest.v3.ReleaseDescriptor` | content | 1 048 576 | payload + 16 384 |
| `application/vnd.impluvium.depot-ticket.v1+protobuf` | 50 | `impluvium.v2.DepotTicketClaims` | depot ticket | – | 4 096 |
| `application/vnd.impluvium.session-proof.v1+protobuf` | 51 | `impluvium.v2.SessionProof` | device | – | 4 096 |
| `application/vnd.impluvium.pairing-proof.v1+protobuf` | 51 | `impluvium.v2.PairingProof` | device | – | 4 096 |

For the last three types the whole binary envelope, including the payload, is limited to 4 096 bytes.

Any other payload type is unknown. An envelope with an unknown payload type is rejected before its payload is read
(`unknown_payload_type`). In particular this revision defines no signed revocation list and no key-rotation statement;
such payload types are unknown. Adding a payload type is a specification change that adds a row to this table.

## 3. Pre-authentication encoding

The signed message is

```
PAE(type, body) = "DSSEv1" || SP || LEN(type) || SP || type || SP || LEN(body) || SP || body
```

where `type` is the bytes of `payload_type`, `body` is the payload bytes, `SP` is `0x20` and `LEN(x)` is the byte
length of `x` written as ASCII decimal digits without leading zeros (`0` for an empty value). Lengths count bytes, not
characters. This is the pre-authentication encoding of DSSE protocol version 1. A worked layout is in
[examples.md §1](examples.md#1-pre-authentication-encoding-of-a-session-proof).

## 4. Ed25519

### 4.1 Signing

Signatures are pure Ed25519 as defined in RFC 8032 §5.1 (no context string, no pre-hashing) over the message
`M = PAE(payload_type, payload)`. A signer computes `sig = R || S` with its 32-byte private seed.

### 4.2 Acceptance profile

A signature `sig` by public key `A` over `M` is **valid** if and only if all of the following hold. The same profile
applies to every implementation; equal library names do not imply equal behaviour, so implementations check these
conditions themselves where their library does not guarantee them.

Constants (RFC 8032): `p = 2^255 − 19`; `L = 2^252 + 27742317777372353535851937790883648493`; `d` and the base point `B`
as in RFC 8032 §5.1.

1. `len(sig) = 64`. `R_bytes` is `sig[0..32]`, `S_bytes` is `sig[32..64]`.
2. Point decoding, applied to `A` (32 bytes) and to `R_bytes`: let `y` be the little-endian integer of the 32 bytes with
   bit 255 cleared and `sign` the value of bit 255.
   1. `y < p`; otherwise the encoding is non-canonical.
   2. `x² = (y² − 1) / (d·y² + 1) mod p` has a square root; otherwise the bytes are not a curve point.
   3. If `x = 0`, then `sign = 0`; otherwise the encoding is a negative zero.
   4. The decoded point `P` satisfies `[8]P ≠ O` (the neutral element); that is, `P` is none of the eight points of order
      dividing 8.
3. `S`, the little-endian integer of `S_bytes`, satisfies `S < L`.
4. `k = SHA-512(R_bytes || A || M)`, read as a 64-byte little-endian integer, reduced modulo `L`.
5. The encoding of the point `[S]B − [k]A` equals `R_bytes` byte for byte (cofactorless verification).

A public key that fails step 2 is never accepted as a trusted key ([keys.md §3](keys.md#3-trusted-key-sets)); a device
public key that fails step 2 is rejected at pairing ([proofs.md §3.1](proofs.md#31-startpairing)).

## 5. Multiple signatures

1. An envelope carries 1 to 8 signatures. Zero signatures is rejected (`no_signatures`); more than 8 is rejected
   (`element_limit_exceeded` at `Envelope.signatures`).
2. Two signatures with the same `key_id` are rejected (`duplicate_signature_key_id`), even with equal signature bytes.
3. The caller supplies a **trusted key set** for exactly the role of the expected payload type
   ([keys.md §3](keys.md#3-trusted-key-sets)). The envelope never extends it: a key mentioned or implied by the envelope
   or by discovery is never added.
4. Signatures are evaluated in envelope order:
   - if the `key_id` equals the `key_id` of a key in the trusted set, the signature MUST be valid under that key (§4.2);
     a known `key_id` with an invalid signature rejects the whole envelope (`invalid_signature`), even if another
     signature is valid;
   - a `key_id` that matches no trusted key is ignored.
5. The envelope is accepted only if at least one signature is valid under a trusted key. Otherwise it is rejected
   (`no_trusted_signature`); envelopes whose signatures all carry unknown `key_id`s fall under this rule.
6. A verifier MAY stop at the first `invalid_signature`. It MUST NOT accept before every signature with a known `key_id`
   has been verified.
7. A matching `key_id` only selects the key to try. It never establishes trust by itself.
8. A payload chapter can add a condition that **every** valid signature by a trusted key must meet (depot tickets:
   [keys.md §4.3](keys.md#43-verification-rule)). Such a condition is never satisfied by picking one of several valid
   signatures.

## 6. Exact bytes

1. Verifiers sign and verify the received `payload` bytes. They never re-encode a payload, and they never verify a
   re-encoded payload.
2. `manifest_id`, every stored install baseline and every cache key use the original received bytes.
3. A decode view that omits unknown fields ([encoding.md §2.5](encoding.md#25-unknown-fields)) is used only for reading
   values after the signature check. It is never signed, hashed for an identifier or stored as a baseline.

## 7. Verification procedure

Inputs: the envelope bytes; the expected payload type; the trusted key set; for some types a context (time, expected
audience and so on, defined in the payload chapter). Configuration problems of the trusted key set are detected before
the input is read ([keys.md §3](keys.md#3-trusted-key-sets)) and are never reported as an input reason.

The checks run in this order; the first failing check determines the reason.

| Step | Check | Reason | Class |
|---:|---|---|---|
| 1 | `len(envelope)` is at most the maximum whole-envelope size of the expected payload type (§2) | `envelope_too_large` | size |
| 2 | The envelope bytes follow the binary profile ([encoding.md §2](encoding.md#2-binary-profile)) for `Envelope` and `Signature`, in the scan order of [encoding.md §2.8](encoding.md#28-scan-order-and-precedence), with the limits `payload_type` ≤ 128 bytes, `payload` ≤ max payload of the expected type, at most 8 signatures | reason of the profile, `byte_limit_exceeded`, `element_limit_exceeded` | structure or size |
| 3 | Envelope metadata ≤ 16 384 bytes | `byte_limit_exceeded` | size |
| 4 | At least one signature | `no_signatures` | structure |
| 5 | In signature order: `key_id` has 8 bytes and `sig` has 64 bytes | `malformed_signature_entry` | structure |
| 6 | `payload_type` is 1 to 128 bytes in `0x21`–`0x7E` | `invalid_payload_type` | structure |
| 7 | In signature order: no `key_id` equal to an earlier one | `duplicate_signature_key_id` | structure |
| 8 | `payload_type` is registered (§2) | `unknown_payload_type` | unsupported format |
| 9 | `payload_type` equals the expected payload type | `unexpected_payload_type` | structure |
| 10 | Signatures per §5 | `invalid_signature`, `no_trusted_signature` | signature |
| 11 | Payload: binary profile and limits of its message, in the scan order of [encoding.md §2.8](encoding.md#28-scan-order-and-precedence), including the range check of every timestamp | profile and limit reasons | structure or size |
| 12 | Payload: time window, if the payload type has one (tickets) | payload chapter | time |
| 13 | Payload: semantic rules in the order of the payload chapter | payload chapter | semantic |

Diagnostic fields under [errors.md §5](errors.md#5-local-reason-catalog) are:

| Check or reason | Diagnostic field |
|---|---|
| Whole-envelope size (step 1) or combined metadata budget (step 3) | `""` |
| Signature count, `no_signatures`, `malformed_signature_entry`, `duplicate_signature_key_id` | `Envelope.signatures` |
| `invalid_payload_type`, `unknown_payload_type`, `unexpected_payload_type` | `Envelope.payload_type` |
| `invalid_signature` | `Signature.sig` |
| `no_trusted_signature` | `""` |

The signature-entry length check labels the repeated element as a whole; it does not add an order between its two
length predicates. Binary-profile failures within a `Signature` instead identify its own known field. Payload checks
retain the payload schema's diagnostic field, or `""`, rather than inheriting `Envelope.payload`.

Step 12 comes immediately after the bounded read of the signed claims: expiry can only be read from the signed payload,
so it cannot be checked earlier, and it is checked before every other semantic rule of that payload. Timestamps that
step 12 compares have passed their range check in step 11.

The reason classes map to Connect codes in [errors.md §4](errors.md#4-mapping-of-local-reasons-to-connect-codes).

## 8. Identifiers

### 8.1 `manifest_id`

`manifest_id = SHA-256(payload)`, where `payload` is the `Envelope.payload` bytes of a manifest envelope. It is 32
bytes, independent of the content hash algorithm declared in the manifest, and independent of the signatures: re-signing
the same payload keeps the `manifest_id`.

### 8.2 `key_id`

`key_id = SHA-256(public_key)[0..8]`, the first 8 bytes of SHA-256 over the 32-byte Ed25519 public key encoding. It is
independent of the content hash algorithm.

## 9. Producer rules

1. A producer signs `PAE(payload_type, payload)` with every key it uses and emits one `Signature` per key. Servers of
   this revision emit exactly one signature per envelope.
2. The producer serialises the envelope with the producer encoding of [encoding.md §2.7](encoding.md#27-producer-encoding).
3. A producer never signs a payload that its own verifier would reject.
