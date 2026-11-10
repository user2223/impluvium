import { Readable } from 'node:stream';

export const sampleBytes = () => Uint8Array.from({ length: 16 }, (_, index) => index + 1);

// Factories keep consumable streams and mutable backing stores independent.
export const binaryValues = [
  { name: 'Blob', create: () => new Blob([sampleBytes()], { type: 'application/octet-stream' }) },
  { name: 'File', create: () => new File([sampleBytes()], 'sample.bin', { type: 'application/octet-stream', lastModified: 12345 }) },
  { name: 'ArrayBuffer', create: () => sampleBytes().buffer },
  { name: 'DataView', create: () => new DataView(sampleBytes().buffer, 2, 4) },
  { name: 'Buffer', create: () => Buffer.from(sampleBytes()) },
  { name: 'ReadableStream', create: () => new ReadableStream({ start(controller) { controller.enqueue(sampleBytes()); controller.close(); } }) },
];

for (const name of [
  'Int8Array', 'Uint8Array', 'Uint8ClampedArray', 'Int16Array', 'Uint16Array',
  'Int32Array', 'Uint32Array', 'Float16Array', 'Float32Array', 'Float64Array',
  'BigInt64Array', 'BigUint64Array',
]) {
  const Type = globalThis[name];
  if (typeof Type === 'function') binaryValues.push({ name, create: () => new Type(sampleBytes().buffer) });
}
if (typeof SharedArrayBuffer !== 'undefined') {
  binaryValues.push({ name: 'SharedArrayBuffer', create: () => {
    const buffer = new SharedArrayBuffer(16);
    new Uint8Array(buffer).set(sampleBytes());
    return buffer;
  } });
}

class IterableBytes {
  *[Symbol.iterator]() { yield sampleBytes(); }
}
class AsyncIterableBytes {
  async *[Symbol.asyncIterator]() { yield sampleBytes(); }
}

export const streamingBodies = [
  { name: 'Node Readable', create: () => Readable.from([sampleBytes()]) },
  { name: 'Iterable bytes', create: () => new IterableBytes() },
  { name: 'AsyncIterable bytes', create: () => new AsyncIterableBytes() },
];

class UnsupportedBody { field = 'sample'; }
export const unsupportedBody = () => new UnsupportedBody();

class JsonBody { toJSON() { return { name: 'sample' }; } }
class ArrayResult extends Array {}
export const ordinaryBodies = [
  { name: 'undefined', create: () => undefined },
  { name: 'null', create: () => null },
  { name: 'text', create: () => 'sample' },
  { name: 'empty text', create: () => '' },
  { name: 'number', create: () => 42 },
  { name: 'zero', create: () => 0 },
  { name: 'boolean', create: () => true },
  { name: 'false', create: () => false },
  { name: 'array', create: () => ['sample', 42, false] },
  { name: 'record', create: () => ({ name: 'sample' }) },
  { name: 'toJSON', create: () => new JsonBody() },
  { name: 'Date toJSON', create: () => new Date('2030-01-01T00:00:00.000Z') },
];

export const jsonResponses = [undefined, null, false, true, 0, 42, '', 'sample', [], { name: 'sample', values: [1, null, false] }];
export const opaqueResponses = [
  { name: 'Date', create: () => new Date('2030-01-01T00:00:00.000Z') },
  { name: 'Map', create: () => new Map([['sample', 1]]) },
  { name: 'Set', create: () => new Set([1]) },
  { name: 'BigInt', create: () => 1n },
  { name: 'NaN', create: () => Number.NaN },
  { name: 'infinity', create: () => Number.POSITIVE_INFINITY },
  { name: 'toJSON parser result', create: () => new JsonBody() },
  { name: 'cyclic object', create: () => { const value = {}; value.self = value; return value; } },
  { name: 'nested binary value', create: () => ({ bytes: sampleBytes() }) },
  { name: 'nested undefined', create: () => ({ missing: undefined }) },
  { name: 'sparse array', create: () => new Array(2) },
  { name: 'symbol key', create: () => ({ [Symbol('sample')]: 1 }) },
  { name: 'function', create: () => () => 'sample' },
  { name: 'symbol', create: () => Symbol('sample') },
  { name: 'Node stream parser result', create: () => Readable.from([sampleBytes()]) },
  { name: 'opaque proxy', create: () => new Proxy({}, { getPrototypeOf() { throw new Error('Opaque proxy'); } }) },
  { name: 'custom array prototype', create: () => ArrayResult.of(1, 2) },
  { name: 'null object prototype', create: () => Object.assign(Object.create(null), { name: 'sample' }) },
  { name: 'accessor result', create: () => Object.defineProperty({}, 'name', { enumerable: true, get() { return 'sample'; } }) },
  { name: 'FormData parser result', create: () => { const value = new FormData(); value.set('name', 'sample'); return value; } },
  { name: 'URLSearchParams parser result', create: () => new URLSearchParams({ name: 'sample' }) },
];
