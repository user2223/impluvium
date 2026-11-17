// SPDX-License-Identifier: 0BSD
import { createHash, createHmac } from 'node:crypto';
export { seedPublicKey, ed25519KnownAnswer, ed25519Verify } from './ed25519.ts';

export function fromHex(value: string): Buffer {
  if (typeof value !== 'string' || !/^(?:[0-9a-f]{2})*$/.test(value)) throw Error('invalid_hex');
  return Buffer.from(value, 'hex');
}
export function digest(algorithm: 'sha256' | 'sha512' | 'md5', bytes: Uint8Array): Buffer {
  return createHash(algorithm).update(bytes).digest();
}
export function hkdfSha256(ikm: Uint8Array, salt: Uint8Array, info: Uint8Array, length: number): { prkHex: string; okmHex: string } {
  if (!Number.isInteger(length) || length < 0 || length > 255 * 32) throw Error('invalid_hkdf_length');
  const mac = (key: Uint8Array, message: Uint8Array) => createHmac('sha256', key).update(message).digest();
  const prk = mac(salt.length ? salt : Buffer.alloc(32), ikm);
  let last: Buffer = Buffer.alloc(0);
  const blocks: Buffer[] = [];
  for (let counter = 1; blocks.length * 32 < length; counter++) {
    last = mac(prk, Buffer.concat([last, info, Buffer.from([counter])]));
    blocks.push(last);
  }
  return { prkHex: prk.toString('hex'), okmHex: Buffer.concat(blocks).subarray(0, length).toString('hex') };
}
export function pae(type: Uint8Array, body: Uint8Array): Buffer {
  return Buffer.concat([Buffer.from('DSSEv1 ' + type.length + ' ', 'ascii'), type,
    Buffer.from(' ' + body.length + ' ', 'ascii'), body]);
}
export function ed25519EncodingSubstage(publicKey: Uint8Array, signature: Uint8Array): { encodingAccepted: boolean; failedCheck: string } {
  if (publicKey.length !== 32) return { encodingAccepted: false, failedCheck: 'A-length' };
  if (signature.length !== 64) return { encodingAccepted: false, failedCheck: 'signature-length' };
  const little = (bytes: Uint8Array) => { let value = 0n; for (let i = bytes.length - 1; i >= 0; i--) value = value * 256n + BigInt(bytes[i]); return value; };
  const p = (1n << 255n) - 19n, order = (1n << 252n) + 27742317777372353535851937790883648493n;
  for (const [name, point] of [['A', publicKey], ['R', signature.subarray(0, 32)]] as const) {
    const encoded = little(point), y = encoded & ((1n << 255n) - 1n);
    if (y >= p) return { encodingAccepted: false, failedCheck: name + '-canonical-y' };
    if ((y === 1n || y === p - 1n) && encoded >> 255n === 1n) return { encodingAccepted: false, failedCheck: name + '-negative-zero' };
  }
  if (little(signature.subarray(32)) >= order) return { encodingAccepted: false, failedCheck: 'S-bound' };
  return { encodingAccepted: true, failedCheck: '' };
}
