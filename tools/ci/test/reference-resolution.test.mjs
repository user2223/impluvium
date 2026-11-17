import test from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import { mkdirSync, symlinkSync, writeFileSync } from 'node:fs';
import { temporary } from './helpers.mjs';
import { resolveEdge, expectedPolicy } from '../reference-boundary.mjs';
function fixture(t) {
  const root = temporary(t), ref = path.join(root, 'reference'), pkg = path.join(root, 'store/noble');
  const put = (file, text = 'export {};') => { mkdirSync(path.dirname(file), { recursive: true }); writeFileSync(file, text); };
  const owner = path.join(ref, 'entry.ts'), local = path.join(ref, 'local.ts'), product = path.join(root, 'product/generated.ts');
  put(owner); put(local); put(product);
  put(path.join(ref, 'package.json'), JSON.stringify({ imports: { '#local': './local.ts', '#product': './link.ts' } }));
  symlinkSync(product, path.join(ref, 'link.ts'));
  const manifest = { name: '@noble/hashes', exports: { './blake3': { types: './blake3.d.ts', node: { import: './blake3.js', require: './blake3.cjs' } } } };
  for (const file of ['blake3.js', 'blake3.cjs', 'blake3.d.ts']) put(path.join(pkg, file));
  mkdirSync(path.join(ref, 'node_modules/@noble'), { recursive: true }); symlinkSync(pkg, path.join(ref, 'node_modules/@noble/hashes'));
  const dependencyOwner = path.join(ref, 'container/run.mjs'); put(dependencyOwner);
  const context = { root, ref, packages: new Map([[pkg, manifest]]), runtime: [pkg] };
  return { root, ref, pkg, owner, local, product, manifest, context, dependencyOwner };
}
test('resolved paths distinguish Node import/require and type export conditions', t => {
  const value = fixture(t), { owner, context, pkg, local, dependencyOwner } = value;
  assert.equal(resolveEdge('./local.ts', owner, context), local);
  assert.equal(resolveEdge('#local', owner, context, true), local);
  assert.equal(resolveEdge('@noble/hashes/blake3', dependencyOwner, context), path.join(pkg, 'blake3.js'));
  assert.equal(resolveEdge('@noble/hashes/blake3', dependencyOwner, context, true), path.join(pkg, 'blake3.d.ts'));
  assert.equal(resolveEdge('@noble/hashes/blake3', dependencyOwner, context, false, true), path.join(pkg, 'blake3.cjs'));
});
for (const specifier of ['../product/generated.ts', '#product']) test(`the real target of ${specifier} stays outside the allowlist`, t => {
  const { owner, context, product } = fixture(t);
  assert.throws(() => resolveEdge(specifier, owner, context, true), error => error.message.includes('forbidden resolved path') && error.message.includes(product));
});
test('a package alias is checked by installed target, not spelling', t => {
  const { owner, context, ref, product } = fixture(t);
  symlinkSync(path.dirname(product), path.join(ref, 'node_modules/renamed'));
  assert.throws(() => resolveEdge('renamed/generated.ts', owner, context, true), /forbidden resolved package.*product/);
});
test('array export fallbacks and unknown resolution are errors', t => {
  const { owner, context, manifest, dependencyOwner } = fixture(t);
  manifest.exports['./blake3'] = ['./blake3.js'];
  assert.throws(() => resolveEdge('@noble/hashes/blake3', dependencyOwner, context), /unanalyzable package/);
  assert.throws(() => resolveEdge('./missing.ts', owner, context), /unresolved file/);
  assert.throws(() => resolveEdge('data:text/javascript,export{}', owner, context), /forbidden URL/);
  assert.throws(() => resolveEdge('node:fs', owner, context), /forbidden builtin/);
});

test('the pinned package is confined to the container adapter and graph', t => {
  const { owner, context, ref, pkg } = fixture(t);
  const adapter = path.join(ref, 'container/run.mjs');
  mkdirSync(path.dirname(adapter), { recursive: true }); writeFileSync(adapter, 'export {};');
  const container = { ...context, graph: 'container' };
  assert.equal(resolveEdge('@noble/hashes/blake3', adapter, container), path.join(pkg, 'blake3.js'));
  assert.throws(() => resolveEdge('@noble/hashes/blake3', owner, container), /forbidden container dependency/);
  assert.throws(() => resolveEdge('@noble/hashes/blake3', adapter, { ...context, graph: 'local' }), /forbidden container dependency/);
  assert.throws(() => resolveEdge('./container/run.mjs', owner, { ...context, graph: 'contract' }), /forbidden container module/);
  assert.throws(() => resolveEdge('../store/noble/blake3.js', owner, { ...context, graph: 'local' }), /forbidden container dependency path/);
});

test('declaration .ts imports resolve to .d.ts but a missing declaration is an error', t => {
  const { pkg, context } = fixture(t), owner = path.join(pkg, '_md.d.ts'), declaration = path.join(pkg, 'utils.d.ts');
  writeFileSync(owner, 'import type { Hash } from "./utils.ts";');
  writeFileSync(declaration, 'export type Hash = string;');
  assert.equal(resolveEdge('./utils.ts', owner, context), declaration);
  assert.throws(() => resolveEdge('./missing.ts', owner, context), /unresolved file import/);
  assert.throws(() => resolveEdge('./utils.ts', path.join(pkg, 'runtime.js'), context), /unresolved file import/);
});
test('declaration extension substitution retains the real-path boundary', t => {
  const { pkg, context, product } = fixture(t);
  symlinkSync(product, path.join(pkg, 'outside.d.ts'));
  assert.throws(() => resolveEdge('./outside.ts', path.join(pkg, 'declaration.d.ts'), context), /forbidden resolved path/);
});
test('filesystem permission is per module and forbidden services have no module grant', t => {
  const { ref, context } = fixture(t);
  for (const file of ['src/loader.ts', 'tooling/output.mjs']) assert.equal(resolveEdge('node:fs/promises', path.join(ref, file), context), 'node:fs/promises');
  for (const file of ['src/gates.ts', 'tooling/check.mjs', 'container/run.mjs']) assert.throws(() => resolveEdge('node:fs/promises', path.join(ref, file), context), /forbidden builtin/);
  for (const specifier of ['node:fs', 'node:child_process', 'node:worker_threads', 'node:net', 'node:http', 'node:https', 'node:http2', 'node:dgram', 'node:tls', 'node:vm', 'node:module', 'node:inspector']) {
    for (const file of ['src/loader.ts', 'tooling/output.mjs', 'container/run.mjs']) assert.throws(() => resolveEdge(specifier, path.join(ref, file), context), /forbidden builtin/);
  }
  assert.throws(() => resolveEdge('node:crypto', path.join(ref, 'container/run.mjs'), context), /forbidden builtin/);
});

test('dependency standard-module grants are also confined to exact files', t => {
  const { pkg, context } = fixture(t);
  assert.equal(resolveEdge('node:crypto', path.join(pkg, 'esm/cryptoNode.js'), context), 'node:crypto');
  assert.throws(() => resolveEdge('node:crypto', path.join(pkg, 'esm/blake3.js'), context), /forbidden builtin/);
  assert.throws(() => resolveEdge('node:fs/promises', path.join(pkg, 'esm/cryptoNode.js'), context), /forbidden builtin/);
});

test('open flags are confined to the two I/O modules', t => {
  const { ref, context } = fixture(t);
  for (const file of ['src/loader.ts', 'tooling/output.mjs']) assert.equal(resolveEdge('node:constants', path.join(ref, file), context), 'node:constants');
  for (const file of ['src/gates.ts', 'tooling/options.mjs', 'container/run.mjs']) assert.throws(() => resolveEdge('node:constants', path.join(ref, file), context), /forbidden builtin/);
});

test('exclusive temporary output capabilities are confined to the writer module', t => {
  const { ref, context } = fixture(t), modules = expectedPolicy.runtime.modules;
  const writer = modules['tooling/output.mjs'], reader = modules['src/loader.ts'];
  assert.equal(resolveEdge('node:crypto', path.join(ref, 'tooling/output.mjs'), context), 'node:crypto');
  assert.deepEqual(writer.cryptoImports, ['randomBytes']);
  assert.deepEqual(modules['src/coverage.ts'].cryptoImports, ['createHash']);
  assert.deepEqual(modules['src/gates.ts'].cryptoImports, ['createPrivateKey', 'sign']);
  assert.deepEqual(writer.openFlags, ['O_CREAT', 'O_EXCL', 'O_WRONLY', 'O_NOFOLLOW']);
  assert.deepEqual(writer.handleMethods, ['writeFile', 'sync', 'close']);
  assert.equal(writer.mutableHandleName, 'handle');
  for (const name of ['stat', 'rename', 'unlink']) {
    assert(writer.filesystemImports.includes(name));
    assert(!reader.filesystemImports.includes(name));
  }
  assert.deepEqual(reader.openFlags, ['O_RDONLY', 'O_NOFOLLOW']);
  assert.equal(reader.mutableHandleName, undefined);
  for (const [file, policy] of Object.entries(modules)) {
    if (file !== 'tooling/output.mjs') assert.equal(policy.cryptoImports?.includes('randomBytes') ?? false, false);
  }
});
