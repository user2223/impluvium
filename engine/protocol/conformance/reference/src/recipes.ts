// SPDX-License-Identifier: 0BSD
import { createHash } from 'node:crypto';

export const recipeLimits = Object.freeze({ nodes: 8192, reachable: 256, visits: 4096, depth: 32, children: 128,
  literalBytes: 65536, designatedBytes: 1073741824n, materializedBytes: 33554432n, blockBytes: 65536 });
export type Recipe = { id: string; op: string; byteLengthU64: string; sha256Hex: string; [field: string]: any };
const decimal = (value: unknown): bigint => {
  if (typeof value !== 'string' || value.length > 20 || !/^(0|[1-9][0-9]*)$/.test(value)) throw Error('recipe_invalid_integer');
  const n = BigInt(value); if (n > 18446744073709551615n) throw Error('recipe_integer_overflow'); return n;
};
export class RecipeBook {
  nodes = new Map<string, Recipe>();
  lengths = new Map<string, bigint>();
  verified = new Set<string>();
  patterns = new Map<string, Buffer>();
  constructor(entries: Recipe[]) {
    if (!Array.isArray(entries) || entries.length > recipeLimits.nodes) throw Error('recipe_node_limit');
    for (const entry of entries) {
      if (!entry || !/^bytes-[0-9]{6}$/.test(entry.id) || this.nodes.has(entry.id)) throw Error('recipe_invalid_id');
      if (!/^[a-f0-9]{64}$/.test(entry.sha256Hex)) throw Error('recipe_invalid_digest');
      decimal(entry.byteLengthU64);
      const copy = structuredClone(entry); if (Array.isArray(copy.parts)) Object.freeze(copy.parts);
      this.nodes.set(entry.id, Object.freeze(copy));
    }
    for (const entry of entries) this.measure(entry.id, [], new Set(), { visits: 0 });
  }
  children(node: Recipe): string[] {
    if (node.op === 'repeat' || node.op === 'slice') return [node.source];
    if (node.op === 'concat') return node.parts;
    if (node.op === 'insert') return [node.source, node.insert];
    return [];
  }
  private measure(id: string, ancestors: string[], reachable: Set<string>, work: { visits: number }): bigint {
    if (++work.visits > recipeLimits.visits) throw Error('recipe_traversal_limit');
    if (ancestors.includes(id)) throw Error('recipe_cycle');
    if (ancestors.length + 1 > recipeLimits.depth) throw Error('recipe_depth_limit');
    reachable.add(id); if (reachable.size > recipeLimits.reachable) throw Error('recipe_reachable_limit');
    const node = this.nodes.get(id); if (!node) throw Error('recipe_missing_reference');
    const fields: Record<string, string[]> = {
      literal: ['hex'], repeat: ['source', 'countU64'], concat: ['parts'], insert: ['source', 'insert', 'atU64'],
      slice: ['source', 'fromU64', 'takeU64'], counter: ['startU64', 'stepU64', 'width', 'endian', 'outputBytesU64']
    };
    if (!Object.hasOwn(fields, node.op)) throw Error('recipe_unknown_operation');
    const allowed = new Set(['id', 'op', 'byteLengthU64', 'sha256Hex', ...fields[node.op]]);
    if (Object.keys(node).some(key => !allowed.has(key)) || [...allowed].some(key => !Object.hasOwn(node, key))) throw Error('recipe_invalid_fields');
    if (node.op === 'concat' && (!Array.isArray(node.parts) || node.parts.length > recipeLimits.children)) throw Error('recipe_child_limit');
    const childLengths = this.children(node).map(child => this.measure(child, [...ancestors, id], reachable, work));
    let length: bigint;
    if (node.op === 'literal') {
      if (typeof node.hex !== 'string' || !/^(?:[a-f0-9]{2})*$/.test(node.hex)) throw Error('recipe_invalid_hex');
      if (node.hex.length / 2 > recipeLimits.literalBytes) throw Error('recipe_literal_limit');
      length = BigInt(node.hex.length / 2);
    } else if (node.op === 'repeat') length = childLengths[0] * decimal(node.countU64);
    else if (node.op === 'concat') length = childLengths.reduce((sum, child) => sum + child, 0n);
    else if (node.op === 'slice') {
      const from = decimal(node.fromU64), take = decimal(node.takeU64);
      if (from + take > childLengths[0]) throw Error('recipe_slice_bounds'); length = take;
    } else if (node.op === 'insert') {
      if (decimal(node.atU64) > childLengths[0]) throw Error('recipe_insert_bounds'); length = childLengths[0] + childLengths[1];
    } else {
      if (![1, 2, 4, 8].includes(node.width) || !['little', 'big'].includes(node.endian)) throw Error('recipe_counter_parameters');
      const modulus = 1n << BigInt(node.width * 8);
      if (decimal(node.startU64) >= modulus || decimal(node.stepU64) >= modulus) throw Error('recipe_counter_range');
      length = decimal(node.outputBytesU64);
    }
    if (length > recipeLimits.designatedBytes) throw Error('recipe_length_limit');
    if (length !== decimal(node.byteLengthU64)) throw Error('recipe_length_mismatch');
    this.lengths.set(id, length); return length;
  }
  private fill(id: string, from: bigint, target: Buffer): void {
    const node = this.nodes.get(id)!;
    if (!target.length) return;
    if (node.op === 'literal') { Buffer.from(node.hex, 'hex').copy(target, 0, Number(from), Number(from) + target.length); return; }
    if (node.op === 'counter') {
      const width = node.width, modulus = 1n << BigInt(width * 8), start = decimal(node.startU64), step = decimal(node.stepU64);
      let written = 0, position = from;
      while (written < target.length) {
        const word = (start + step * (position / BigInt(width))) % modulus, first = Number(position % BigInt(width));
        for (let byte = first; byte < width && written < target.length; byte++, written++, position++) {
          const shift = node.endian === 'little' ? byte : width - 1 - byte;
          target[written] = Number((word >> BigInt(shift * 8)) & 255n);
        }
      }
      return;
    }
    if (node.op === 'slice') { this.fill(node.source, decimal(node.fromU64) + from, target); return; }
    if (node.op === 'repeat') {
      const childLength = this.lengths.get(node.source)!;
      if (childLength === 0n) throw Error('recipe_internal_zero_repeat');
      if (childLength <= 4096n) {
        let pattern = this.patterns.get(node.source);
        if (!pattern) { pattern = Buffer.alloc(Number(childLength)); this.fill(node.source, 0n, pattern); this.patterns.set(node.source, pattern); }
        const offset = Number(from % childLength), prefix = Math.min(target.length, pattern.length - offset);
        pattern.copy(target, 0, offset, offset + prefix);
        if (prefix < target.length) target.fill(pattern, prefix);
        return;
      }
      let written = 0, position = from;
      while (written < target.length) {
        const offset = position % childLength, amount = Math.min(target.length - written, Number(childLength - offset));
        this.fill(node.source, offset, target.subarray(written, written + amount)); written += amount; position += BigInt(amount);
      }
      return;
    }
    const spans: { id: string; from: bigint; length: bigint }[] = node.op === 'concat'
      ? node.parts.map((part: string) => ({ id: part, from: 0n, length: this.lengths.get(part)! }))
      : [{ id: node.source, from: 0n, length: decimal(node.atU64) },
        { id: node.insert, from: 0n, length: this.lengths.get(node.insert)! },
        { id: node.source, from: decimal(node.atU64), length: this.lengths.get(node.source)! - decimal(node.atU64) }];
    let position = 0n, written = 0;
    for (const span of spans) {
      const begin = from > position ? from : position, end = from + BigInt(target.length) < position + span.length ? from + BigInt(target.length) : position + span.length;
      if (end > begin) {
        const amount = Number(end - begin); this.fill(span.id, span.from + begin - position, target.subarray(written, written + amount)); written += amount;
      }
      position += span.length;
    }
    if (written !== target.length) throw Error('recipe_internal_range');
  }
  private *render(id: string): Generator<Buffer> {
    const length = this.lengths.get(id); if (length === undefined) throw Error('recipe_missing_reference');
    for (let offset = 0n; offset < length; offset += BigInt(recipeLimits.blockBytes)) {
      const remaining = length - offset, block = Buffer.alloc(Number(remaining < BigInt(recipeLimits.blockBytes) ? remaining : BigInt(recipeLimits.blockBytes)));
      this.fill(id, offset, block); yield block;
    }
  }
  verify(id: string): void {
    if (this.verified.has(id)) return;
    const node = this.nodes.get(id); if (!node) throw Error('recipe_missing_reference');
    for (const child of this.children(node)) this.verify(child);
    const hash = createHash('sha256'); let length = 0n;
    for (const bytes of this.render(id)) { hash.update(bytes); length += BigInt(bytes.length); }
    if (length !== decimal(node.byteLengthU64)) throw Error('recipe_length_mismatch');
    if (hash.digest('hex') !== node.sha256Hex) throw Error('recipe_digest_mismatch');
    this.verified.add(id);
  }
  *chunks(id: string): Generator<Buffer> { this.verify(id); yield* this.render(id); }
  buffer(id: string): Buffer {
    const length = this.lengths.get(id); if (length === undefined) throw Error('recipe_missing_reference');
    if (length > recipeLimits.materializedBytes) throw Error('recipe_materialization_limit');
    this.verify(id); return Buffer.concat([...this.render(id)], Number(length));
  }
}
export function materialize(value: any, book: RecipeBook): any {
  if (Array.isArray(value)) return value.map(item => materialize(item, book));
  if (value && typeof value === 'object') {
    const keys = Object.keys(value);
    if (keys.length === 1 && keys[0] === '$bytes') return book.buffer(value.$bytes).toString('hex');
    if (keys.length === 1 && keys[0] === '$utf8') return new TextDecoder('utf-8', { fatal: true, ignoreBOM: true }).decode(book.buffer(value.$utf8));
    return Object.fromEntries(Object.entries(value).map(([key, item]) => [key, materialize(item, book)]));
  }
  return value;
}
export function canonicalSpans(spans: { offsetU64: string; lengthU64: string }[]): Buffer {
  let next = 0n; const lines: string[] = [];
  for (const span of spans) {
    const offset = decimal(span.offsetU64), length = decimal(span.lengthU64);
    if (offset !== next || length === 0n || offset + length > 18446744073709551615n) throw Error('invalid_canonical_spans');
    lines.push(offset + ':' + length + '\n'); next += length;
  }
  return Buffer.from(lines.join(''), 'ascii');
}
