import test from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { mkdirSync, writeFileSync, symlinkSync } from 'node:fs';
import { temporary } from './helpers.mjs';
import { referenceProductIssues, referenceContextExclusions } from '../reference-products.mjs';
import { referencePath } from '../reference-boundary.mjs';

const repository = fileURLToPath(new URL('../../../', import.meta.url));
function fixture(t) {
  const root = temporary(t), name = 'impluvium-protocol-reference';
  const put = (file, content) => { const target = path.join(root, file); mkdirSync(path.dirname(target), { recursive: true }); writeFileSync(target, content); };
  const json = (file, value) => put(file, JSON.stringify(value));
  put('.dockerignore', '*\n' + referenceContextExclusions.join('\n') + '\n');
  put('pnpm-workspace.yaml', `packages:\n  - server/\n  - desktop/main/\n  - ${referencePath}/\n`);
  put('pnpm-lock.yaml', "lockfileVersion: '9.0'\nimporters:\n  server: {}\nsnapshots: {}\n");
  json('package.json', { name: 'workspace' });
  json('server/package.json', { name: 'server' }); json('desktop/main/package.json', { name: 'client' });
  json(referencePath + '/package.json', { name, private: true }); put(referencePath + '/src/input.mjs', 'export const value = 1;');
  json('desktop/src-tauri/tauri.conf.json', { build: { frontendDist: '../.output' }, bundle: { externalBin: [] } });
  return { root, name, put, json };
}
test('reference package is absent from product source inputs and the server build context', () => {
  assert.deepEqual(referenceProductIssues(repository), []);
});
test('runtime boundary has a positive fixture and permits ordinary product-only dependencies', t => {
  const f = fixture(t); f.json('server/package.json', { dependencies: { ordinary: '1.0.0' } });
  assert.deepEqual(referenceProductIssues(f.root), []);
});
for (const [name, mutate, diagnostic] of [
  ['removed context exclusion', f => f.put('.dockerignore', '*\n'), /build-context exclusions/],
  ['overridden context exclusion', f => f.put('.dockerignore', '*\n' + referenceContextExclusions.join('\n') + '\n!engine/**\n'), /build-context exclusions/],
  ['workspace runtime dependency', f => f.json('server/package.json', { dependencies: { [f.name]: 'workspace:*' } }), /forbidden reference dependency/],
  ['aliased build dependency', f => f.json('desktop/main/package.json', { devDependencies: { helper: 'workspace:' + f.name + '@*' } }), /forbidden reference dependency/],
  ['file dependency', f => f.json('server/package.json', { dependencies: { helper: 'file:../engine/protocol/conformance/reference' } }), /reference dependency/],
  ['lock-only dependency', f => f.put('pnpm-lock.yaml', "importers:\n  server:\n    dependencies:\n      helper:\n        version: link:../engine/protocol/conformance/reference\nsnapshots: {}\n"), /pnpm-lock.yaml:server/],
  ['source import', f => f.put('server/src/entry.mjs', 'import "../../engine/protocol/conformance/reference/src/input.mjs";'), /reference source or dependency/],
  ['source symlink', f => symlinkSync(path.join(f.root, referencePath, 'src/input.mjs'), path.join(f.root, 'desktop/main/linked.mjs')), /reference source linked/],
  ['client resource escape', f => f.json('desktop/src-tauri/tauri.conf.json', { build: { frontendDist: '../.output' }, bundle: { resources: ['../../**/*'] } }), /client resource escapes/],
  ['client frontend escape', f => f.json('desktop/src-tauri/tauri.conf.json', { build: { frontendDist: '../../' }, bundle: {} }), /client frontend source escapes/],
]) test(`runtime reference boundary rejects ${name}`, t => {
  const f = fixture(t); assert.deepEqual(referenceProductIssues(f.root), []); mutate(f);
  assert(referenceProductIssues(f.root).some(issue => diagnostic.test(issue)));
});
