// SPDX-License-Identifier: 0BSD
// Independent pair-loop implementation of manifest.md 9.4.
export type GearTable = { gearU64: string[]; maskSmallU64: string; maskLargeU64: string };
export type ChunkSpan = { offsetU64: string; lengthU64: string };
export type CutWitness = { offsetU64: string; lengthU64: string; phase: string; fingerprintU64: string };
export function fastcdc(bytes: Uint8Array, constants: GearTable, trace?: CutWitness[]): ChunkSpan[] {
  if (constants.gearU64.length !== 256) throw Error('invalid_gear_table');
  const gear = constants.gearU64.map(value => BigInt(value));
  if (gear.some(value => value < 0n || value > 0xffffffffffffffffn)) throw Error('invalid_gear_value');
  const shifted = gear.map(value => BigInt.asUintN(64, value << 1n));
  const small = BigInt(constants.maskSmallU64), large = BigInt(constants.maskLargeU64);
  const minimum = 65536, average = 262144, maximum = 1048576;
  function cut(start: number): number {
    let n = bytes.length - start;
    if (n <= minimum) { trace?.push({ offsetU64: String(start), lengthU64: String(n), phase: 'remainder-at-or-below-minimum', fingerprintU64: '0' }); return n; }
    let center = average;
    if (n > maximum) n = maximum;
    else if (n < average) center = n;
    let h = 0n, i = minimum / 2;
    for (; i < Math.floor(center / 2); i++) {
      const a = 2 * i;
      h = BigInt.asUintN(64, (h << 2n) + shifted[bytes[start + a]]);
      if ((h & (small << 1n)) === 0n) { trace?.push({ offsetU64: String(start), lengthU64: String(a), phase: 'small-even', fingerprintU64: String(h) }); return a; }
      h = BigInt.asUintN(64, h + gear[bytes[start + a + 1]]);
      if ((h & small) === 0n) { trace?.push({ offsetU64: String(start), lengthU64: String(a + 1), phase: 'small-odd', fingerprintU64: String(h) }); return a + 1; }
    }
    for (; i < Math.floor(n / 2); i++) {
      const a = 2 * i;
      h = BigInt.asUintN(64, (h << 2n) + shifted[bytes[start + a]]);
      if ((h & (large << 1n)) === 0n) { trace?.push({ offsetU64: String(start), lengthU64: String(a), phase: 'large-even', fingerprintU64: String(h) }); return a; }
      h = BigInt.asUintN(64, h + gear[bytes[start + a + 1]]);
      if ((h & large) === 0n) { trace?.push({ offsetU64: String(start), lengthU64: String(a + 1), phase: 'large-odd', fingerprintU64: String(h) }); return a + 1; }
    }
    trace?.push({ offsetU64: String(start), lengthU64: String(n), phase: 'window-exhausted', fingerprintU64: String(h) }); return n;
  }
  if (!bytes.length) return [];
  if (bytes.length <= maximum) { trace?.push({ offsetU64: '0', lengthU64: String(bytes.length), phase: 'whole-small-file-bypass', fingerprintU64: '0' }); return [{ offsetU64: '0', lengthU64: String(bytes.length) }]; }
  const chunks: ChunkSpan[] = [];
  let position = 0;
  while (position < bytes.length) {
    const length = cut(position);
    if (length < 1 || length > maximum) throw Error('internal_invalid_cut');
    chunks.push({ offsetU64: String(position), lengthU64: String(length) });
    position += length;
  }
  return chunks;
}
