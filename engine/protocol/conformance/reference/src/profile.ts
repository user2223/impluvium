// SPDX-License-Identifier: 0BSD
export class Rejection extends Error {
  reason: string;
  field: string;
  constructor(reason: string, field = '') { super(reason + (field ? ':' + field : '')); this.reason = reason; this.field = field; }
}
export function reject(reason: string, field = ''): never { throw new Rejection(reason, field); }
export type Schema = { messages: Record<string, { fields: any[] }>; enums: Record<string, Record<string, number>> };
export function defaults(message: string, schema: Schema): Record<string, any> {
  const definition = schema.messages[message];
  if (!definition) throw Error('Unknown schema: ' + message);
  const model: Record<string, any> = {};
  for (const field of definition.fields) {
    if (field.repeated) model[field.name] = [];
    else if (['uint64', 'int64'].includes(field.type)) model[field.name] = '0';
    else if (['uint32', 'int32'].includes(field.type) || schema.enums[field.type]) model[field.name] = 0;
    else if (['string', 'bytes'].includes(field.type)) model[field.name] = '';
    else if (field.type === 'bool') model[field.name] = false;
  }
  return model;
}
export function enumValue(value: number | string, field: any, message: string, schema: Schema): unknown {
  const values = schema.enums[field.type];
  if (!values) throw Error('Not enum');
  const known = typeof value === 'string' ? Object.hasOwn(values, value) ? values[value] : undefined : Object.values(values).includes(value) ? value : undefined;
  if (known !== undefined) return known;
  if (message === 'VersionSummary' && field.name === 'kind') return typeof value === 'string' ? { unknownName: value } : value;
  if (['Header', 'Chunking', 'Entry', 'ReleaseDescriptor', 'LaunchOption', 'SetupStep'].includes(message)) return value;
  return field.repeated ? undefined : 0;
}
export function timestampRange(model: Record<string, any>, field = ''): void {
  const seconds = BigInt(model.seconds), nanos = model.nanos;
  if (seconds < -62135596800n || seconds > 253402300799n || nanos < 0 || nanos > 999999999) reject('invalid_timestamp', field);
}
export function utf8(bytes: Uint8Array, reason: string, field = ''): string {
  try { return new TextDecoder('utf-8', { fatal: true, ignoreBOM: true }).decode(bytes); }
  catch { return reject(reason, field); }
}
export function scanBinary(bytes: Uint8Array, message: string, schema: Schema,
  options: { limit?: number; totalReason?: string; envelopePayloadLimit?: number; collect?: boolean } = {}): Record<string, any> {
  if (bytes.length > (options.limit ?? 65536)) reject(options.totalReason ?? 'body_too_large');
  const globalCounts = new Map<string, number>();
  function scan(data: Uint8Array, name: string, depth: number, path: string, mapField?: string): Record<string, any> {
    if (depth > 64) reject('nesting_too_deep', path);
    const definition = schema.messages[name];
    if (!definition) throw Error('Unknown schema: ' + name);
    const model = defaults(name, schema), seen = new Set<string>(), groups = new Set<string>();
    const counts = new Map<string, number>(), budgets = new Map<string, number>();
    let position = 0;
    const varint = (end = data.length, field = ''): bigint => {
      let value = 0n;
      for (let count = 0; count < 10; count++) {
        if (position >= end) reject('malformed_wire', field);
        const byte = data[position++];
        if (count === 9 && byte > 1) reject('malformed_wire', field);
        value |= BigInt(byte & 127) << BigInt(7 * count);
        if (byte < 128) return value;
      }
      return reject('malformed_wire', field);
    };
    const append = (field: any, value: any) => {
      if (value === undefined) return;
      if (options.collect === false && field.repeated && !field.map) return;
      if (field.repeated) model[field.name].push(value);
      else model[field.name] = value;
    };
    const count = (field: any, fieldPath: string) => {
      if (!field.repeated) return;
      const shared = name === 'Entry' && field.name === 'chunk_indexes';
      const counter = shared ? globalCounts : counts, key = shared ? 'Manifest.chunk_indexes' : field.name;
      const next = (counter.get(key) ?? 0) + 1;
      if (next > field.maxElements) reject('element_limit_exceeded', shared ? '' : fieldPath);
      counter.set(key, next);
    };
    const scalar = (value: bigint, field: any): any => {
      if (field.type === 'bool') return value !== 0n;
      if (field.type === 'uint32') return Number(BigInt.asUintN(32, value));
      if (field.type === 'uint64') return value.toString();
      if (field.type === 'int64') return BigInt.asIntN(64, value).toString();
      if (field.type === 'int32') return Number(BigInt.asIntN(32, value));
      return enumValue(Number(BigInt.asIntN(32, value)), field, name, schema);
    };
    while (position < data.length) {
      const start = position, tag = varint(), no = tag >> 3n, wire = Number(tag & 7n);
      const field = no > 0n && no <= 536870911n ? definition.fields.find(field => field.no === Number(no)) : undefined;
      const fieldPath = field ? mapField ?? name + '.' + field.name : '';
      if (no === 0n || no > 536870911n || ![0, 1, 2, 5].includes(wire)) reject('malformed_wire', fieldPath);
      const numeric = field && (['uint32', 'uint64', 'int32', 'int64', 'bool'].includes(field.type) || schema.enums[field.type]);
      const packed = field?.repeated && (field.type === 'uint32' || schema.enums[field.type]) && wire === 2;
      if (field && wire !== (numeric ? 0 : 2) && !packed) reject('wrong_wire_type', fieldPath);
      let value: bigint | undefined, bodyStart = position, end = position;
      if (wire === 0) { value = varint(data.length, fieldPath); end = position; }
      else if (wire === 1 || wire === 5) {
        end = position + (wire === 1 ? 8 : 4);
        if (end > data.length) reject('malformed_wire', fieldPath);
        position = end;
      } else {
        const size = varint(data.length, fieldPath);
        if (size > BigInt(data.length - position)) reject('malformed_wire', fieldPath);
        bodyStart = position; end = position + Number(size); position = end;
      }
      if (!field) continue;
      if (field.oneof) {
        if (groups.has(field.oneof)) reject('oneof_conflict');
        groups.add(field.oneof);
      } else if (!field.repeated && seen.has(field.name)) {
        reject(schema.messages[field.type] ? 'split_message' : 'duplicate_field', fieldPath);
      }
      seen.add(field.name);
      const encoded = (budgets.get(field.name) ?? 0) + position - start;
      budgets.set(field.name, encoded);
      if (name === 'Envelope' && field.name === 'signatures' && encoded > 16384) reject('byte_limit_exceeded');
      if (packed) {
        position = bodyStart;
        while (position < end) { count(field, fieldPath); append(field, scalar(varint(end, fieldPath), field)); }
        continue;
      }
      count(field, fieldPath);
      const maximum = name === 'Envelope' && field.name === 'payload' ? options.envelopePayloadLimit : field.maxBytes;
      if (wire === 2 && maximum !== undefined && end - bodyStart > maximum) reject('byte_limit_exceeded', fieldPath);
      if (wire === 0) { append(field, scalar(value!, field)); continue; }
      const body = data.subarray(bodyStart, end);
      if (field.type === 'string') append(field, utf8(body, 'invalid_utf8', fieldPath));
      else if (field.type === 'bytes') append(field, Buffer.from(body).toString('hex'));
      else if (field.map) {
        const mapName = name + '.' + field.name + '.Entry';
        const mapSchema = { fields: [
          { no: 1, name: 'key', type: 'string', maxBytes: field.keyMaxBytes },
          { no: 2, name: 'value', type: 'string', maxBytes: field.valueMaxBytes }
        ] };
        schema.messages[mapName] = mapSchema;
        let pair;
        try { pair = scan(body, mapName, depth + 1, fieldPath, fieldPath); }
        finally { delete schema.messages[mapName]; }
        if (model[field.name].some((previous: any) => previous.key === pair!.key)) reject('duplicate_map_key', fieldPath);
        model[field.name].push(pair);
      } else append(field, scan(body, field.type, depth + 1, fieldPath));
    }
    if (name === 'Timestamp') timestampRange(model, path);
    return model;
  }
  return scan(bytes, message, 1, '');
}
