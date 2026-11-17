import test from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import { mkdirSync, writeFileSync, symlinkSync } from 'node:fs';
import { stripTypeScriptTypes } from 'node:module';
import { temporary } from './helpers.mjs';
import { admitReferenceImport } from '../reference-boundary.mjs';

// These tests exercise the production edge admission, not the compiler's AST
// extraction. Node's built-in parser validates the syntax without executing it.
function fixture(t) {
  const root = temporary(t), ref = path.join(root, 'reference');
  const product = path.join(root, 'product/generated.ts'), owner = path.join(ref, 'entry.ts');
  const put = (file, source) => { mkdirSync(path.dirname(file), { recursive: true }); writeFileSync(file, source); };
  put(product, 'export const value = 1; export type Message = string;');
  put(owner, 'export {};'); put(path.join(ref, 'local.ts'), 'export const value = 1;');
  put(path.join(ref, 'package.json'), JSON.stringify({ type: 'module', imports: { '#product': './linked.ts' } }));
  symlinkSync(product, path.join(ref, 'linked.ts'));
  return { root, ref, product, owner, put, context: { root, ref, runtime: [], packages: new Map() } };
}
const valid = source => assert.doesNotThrow(() => stripTypeScriptTypes(source), 'valid fixture syntax is a prerequisite');
for (const [name, source, specifier, type, mode] of [
  ['direct import', 'import "../product/generated.ts";', '../product/generated.ts', false, 'static'],
  ['package imports alias', 'import { value } from "#product";', '#product', false, 'static'],
  ['pure type import', 'import type { Message } from "../product/generated.ts";', '../product/generated.ts', true, 'static'],
  ['aliased type import', 'import type { Message as Local } from "#product";', '#product', true, 'static'],
  ['literal dynamic import', 'await import("../product/generated.ts");', '../product/generated.ts', false, 'dynamic'],
  ['CommonJS import', 'const value = require("../product/generated.ts");', '../product/generated.ts', false, 'require'],
]) test(`reference admission rejects ${name} at the resolved product path`, t => {
  const f = fixture(t); valid(source); f.put(f.owner, source);
  assert(source.includes(JSON.stringify(specifier)));
  assert.throws(() => admitReferenceImport(specifier, f.owner, f.context, type, mode), error =>
    error.name !== 'SyntaxError' && error.message.includes('forbidden resolved path') && error.message.includes(f.product));
});
for (const type of [false, true]) test(`reference admission follows ${type ? 'type ' : ''}reexports before rejecting the product path`, t => {
  const f = fixture(t), bridge = path.join(f.ref, 'bridge.ts');
  const source = `export ${type ? 'type { Message }' : '{ value }'} from "./bridge.ts";`;
  const reexport = `export ${type ? 'type { Message }' : '{ value }'} from "../product/generated.ts";`;
  valid(source); valid(reexport); f.put(f.owner, source); f.put(bridge, reexport);
  assert.equal(admitReferenceImport('./bridge.ts', f.owner, f.context, type), bridge);
  assert.throws(() => admitReferenceImport('../product/generated.ts', bridge, f.context, type), error =>
    error.name !== 'SyntaxError' && error.message.includes('forbidden resolved path') && error.message.includes(f.product));
});
test('reference admission rejects a computed unknown target without pretending it resolved', t => {
  const f = fixture(t); valid('const target = "../product/" + process.argv[2]; await import(target);');
  assert.throws(() => admitReferenceImport(undefined, f.owner, f.context, false, 'dynamic'), error =>
    error.name !== 'SyntaxError' && error.message.includes('unanalyzable target') && error.message.includes(f.owner));
});
test('reference admission rejects an unanalyzable package export after resolving its installed target', t => {
  const f = fixture(t), installed = path.join(f.root, 'store/noble'), adapter = path.join(f.ref, 'container/run.mjs');
  valid('import { blake3 } from "@noble/hashes/blake3";');
  f.put(adapter, 'import { blake3 } from "@noble/hashes/blake3";');
  f.put(path.join(installed, 'hash.js'), 'export const blake3 = () => 0;');
  mkdirSync(path.join(f.ref, 'node_modules/@noble'), { recursive: true }); symlinkSync(installed, path.join(f.ref, 'node_modules/@noble/hashes'));
  const context = { ...f.context, graph: 'container', runtime: [installed], packages: new Map([[installed, { name: '@noble/hashes', exports: { './blake3': ['./hash.js'] } }]]) };
  assert.throws(() => admitReferenceImport('@noble/hashes/blake3', adapter, context), error =>
    error.name !== 'SyntaxError' && /unanalyzable package/.test(error.message));
});
test('reference admission keeps an allowed import and harmless comment control, but rejects runtime loading of it', t => {
  const f = fixture(t), target = path.join(f.ref, 'local.ts');
  for (const source of ['import "./local.ts";', '// Informational comment.\nimport "./local.ts";']) {
    valid(source); assert.equal(admitReferenceImport('./local.ts', f.owner, f.context), target);
  }
  for (const mode of ['dynamic', 'require']) assert.throws(() => admitReferenceImport('./local.ts', f.owner, f.context, false, mode), error =>
    error.message.includes('forbidden') && error.message.includes(target));
});
