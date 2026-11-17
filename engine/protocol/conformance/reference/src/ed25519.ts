// SPDX-License-Identifier: 0BSD
// Independent implementation of the approved mathematical definitions, not RFC program code.
import { createHash } from 'node:crypto';

const p = (1n << 255n) - 19n;
const order = (1n << 252n) + 27742317777372353535851937790883648493n;
const mod = (v: bigint): bigint => (v % p + p) % p;
function power(value: bigint, exponent: bigint): bigint {
  let result = 1n, base = mod(value);
  for (let remaining = exponent; remaining; remaining >>= 1n) {
    if (remaining & 1n) result = mod(result * base);
    base = mod(base * base);
  }
  return result;
}
function inverse(value: bigint): bigint {
  if (mod(value) === 0n) throw Error('ed25519_zero_denominator');
  return power(value, p - 2n);
}
const d = mod(-121665n * inverse(121666n));
const sqrtMinusOne = power(2n, (p - 1n) / 4n);
type Point = { x: bigint; y: bigint; z: bigint; t: bigint };
const neutral: Point = { x: 0n, y: 1n, z: 1n, t: 0n };
function point(x: bigint, y: bigint): Point { return { x, y, z: 1n, t: mod(x * y) }; }
const base = point(15112221349535400772501151409588531511454012693041857206046113283949847762202n,
  46316835694926478169428394003475163141307993866256225615783033603165251855960n);
function add(left: Point, right: Point): Point {
  const a = mod((left.y - left.x) * (right.y - right.x));
  const b = mod((left.y + left.x) * (right.y + right.x));
  const c = mod(2n * d * left.t * right.t), dd = mod(2n * left.z * right.z);
  const e = mod(b - a), f = mod(dd - c), g = mod(dd + c), h = mod(b + a);
  return { x: mod(e * f), y: mod(g * h), z: mod(f * g), t: mod(e * h) };
}
function multiply(value: Point, scalar: bigint): Point {
  let sum = neutral, term = value;
  for (let remaining = scalar; remaining; remaining >>= 1n) {
    if (remaining & 1n) sum = add(sum, term);
    term = add(term, term);
  }
  return sum;
}
function isNeutral(value: Point): boolean { return mod(value.x) === 0n && mod(value.y - value.z) === 0n; }
function integer(bytes: Uint8Array): bigint {
  let result = 0n;
  for (let i = bytes.length - 1; i >= 0; i--) result = result * 256n + BigInt(bytes[i]);
  return result;
}
function little(value: bigint): Buffer {
  const bytes = Buffer.alloc(32);
  for (let i = 0; i < bytes.length; i++, value >>= 8n) bytes[i] = Number(value & 255n);
  return bytes;
}
function encode(value: Point): Buffer {
  const zi = inverse(value.z), x = mod(value.x * zi), y = mod(value.y * zi);
  return little(y | ((x & 1n) << 255n));
}
type Decoded = { accepted: true; value: Point } | { accepted: false; failedCheck: string };
function decode(bytes: Uint8Array): Decoded {
  if (bytes.length !== 32) return { accepted: false, failedCheck: 'length' };
  const encoded = integer(bytes), y = encoded & ((1n << 255n) - 1n), sign = encoded >> 255n;
  if (y >= p) return { accepted: false, failedCheck: 'canonical-y' };
  const yy = mod(y * y), squared = mod((yy - 1n) * inverse(d * yy + 1n));
  let x = power(squared, (p + 3n) / 8n);
  if (mod(x * x) !== squared) x = mod(x * sqrtMinusOne);
  if (mod(x * x) !== squared) return { accepted: false, failedCheck: 'not-on-curve' };
  if (x === 0n && sign === 1n) return { accepted: false, failedCheck: 'negative-zero' };
  if ((x & 1n) !== sign) x = mod(-x);
  const value = point(x, y);
  if (isNeutral(multiply(value, 8n))) return { accepted: false, failedCheck: 'small-order' };
  return { accepted: true, value };
}
export function ed25519PointProfile(bytes: Uint8Array): { accepted: boolean; failedCheck: string } {
  const result = decode(bytes);
  return { accepted: result.accepted, failedCheck: result.accepted ? '' : result.failedCheck };
}
export function ed25519VerifyProfile(publicKey: Uint8Array, message: Uint8Array, signature: Uint8Array): { accepted: boolean; failedCheck: string } {
  if (signature.length !== 64) return { accepted: false, failedCheck: 'signature-length' };
  const a = decode(publicKey);
  if (!a.accepted) return { accepted: false, failedCheck: 'A-' + a.failedCheck };
  const rBytes = signature.subarray(0, 32), r = decode(rBytes);
  if (!r.accepted) return { accepted: false, failedCheck: 'R-' + r.failedCheck };
  const s = integer(signature.subarray(32));
  if (s >= order) return { accepted: false, failedCheck: 'S-bound' };
  const k = integer(createHash('sha512').update(rBytes).update(publicKey).update(message).digest()) % order;
  const ka = multiply(a.value, k);
  const recovered = add(multiply(base, s), { x: mod(-ka.x), y: ka.y, z: ka.z, t: mod(-ka.t) });
  const accepted = encode(recovered).equals(Buffer.from(rBytes));
  return { accepted, failedCheck: accepted ? '' : 'cofactorless-equation' };
}
export function ed25519Verify(publicKey: Uint8Array, message: Uint8Array, signature: Uint8Array): boolean {
  return ed25519VerifyProfile(publicKey, message, signature).accepted;
}
function secret(seed: Uint8Array): { scalar: bigint; prefix: Buffer } {
  if (seed.length !== 32) throw Error('invalid_seed_length');
  const hash = createHash('sha512').update(seed).digest(), low = Buffer.from(hash.subarray(0, 32));
  low[0] &= 248; low[31] = (low[31] & 127) | 64;
  return { scalar: integer(low), prefix: hash.subarray(32) };
}
export function seedPublicKey(seed: Uint8Array): Buffer { return encode(multiply(base, secret(seed).scalar)); }
export function ed25519KnownAnswer(seed: Uint8Array, message: Uint8Array): { publicKeyHex: string; signatureHex: string } {
  const { scalar, prefix } = secret(seed), publicKey = encode(multiply(base, scalar));
  const r = integer(createHash('sha512').update(prefix).update(message).digest()) % order;
  const rBytes = encode(multiply(base, r));
  const k = integer(createHash('sha512').update(rBytes).update(publicKey).update(message).digest()) % order;
  return { publicKeyHex: publicKey.toString('hex'), signatureHex: Buffer.concat([rBytes, little((r + k * scalar) % order)]).toString('hex') };
}
