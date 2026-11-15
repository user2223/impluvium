import test from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import { cpSync, mkdirSync, readFileSync, renameSync, rmSync, symlinkSync, writeFileSync } from 'node:fs';
import { temporary } from './helpers.mjs';
import { pnpmGraph } from '../pnpm-licences.mjs';
import { traverse } from '../licence-graph.mjs';
import { policy } from '../spdx.mjs';

function fixture(t, change = () => {}) {
  const root = temporary(t), store = temporary(t);
  const put = (file, data) => { mkdirSync(path.dirname(file), { recursive: true }); writeFileSync(file, data); };
  const manifest = { name: 'types', version: '1.0.0', license: '0BSD', dependencies: { helper: '1.0.0' } };
  const helper = { name: 'helper', version: '1.0.0', license: 'MIT', dependencies: { renamed: 'npm:leaf@1.0.0' } };
  const leaf = { name: 'leaf', version: '1.0.0', license: 'Apache-2.0' };
  const helperPath = path.join(store, 'node_modules/.pnpm/helper@1.0.0/node_modules/helper');
  const leafPath = path.join(store, 'node_modules/.pnpm/leaf@1.0.0/node_modules/leaf');
  const evidence = [{ name: 'types', version: '1.0.0', path: path.join(root, 'engine/ts'),
    dependencies: { helper: { from: 'helper', version: '1.0.0', path: helperPath,
      dependencies: { renamed: { from: 'leaf', version: '1.0.0', path: leafPath } } } } }];
  let lock = "lockfileVersion: '9.0'\nimporters:\n  engine/ts:\n    dependencies:\n      helper:\n        specifier: 1.0.0\n        version: 1.0.0\npackages:\n  helper@1.0.0: {}\n  leaf@1.0.0: {}\nsnapshots:\n  helper@1.0.0:\n    dependencies:\n      renamed: leaf@1.0.0\n  leaf@1.0.0: {}\n";
  const state = { root, manifest, helper, leaf, evidence, helperPath, leafPath, lock };
  change(state); lock = state.lock;
  put(path.join(root, 'engine/ts/package.json'), JSON.stringify(manifest));
  put(path.join(helperPath, 'package.json'), JSON.stringify(helper));
  put(path.join(leafPath, 'package.json'), JSON.stringify(leaf));
  mkdirSync(path.join(root, 'engine/ts/node_modules'), { recursive: true });
  symlinkSync(helperPath, path.join(root, 'engine/ts/node_modules/helper'));
  mkdirSync(path.join(helperPath, 'node_modules'), { recursive: true });
  symlinkSync(leafPath, path.join(helperPath, 'node_modules/renamed'));
  put(path.join(root, 'pnpm-workspace.yaml'), 'packages:\n  - engine/ts\n  - libraries/helper\n');
  put(path.join(root, 'pnpm-lock.yaml'), lock);
  put(path.join(root, 'graph.json'), JSON.stringify(evidence));
  cpSync(new URL('../../../deny.toml', import.meta.url), path.join(root, 'deny.toml'));
  return { ...state, load: () => pnpmGraph(root, path.join(root, 'graph.json'), path.join(root, 'pnpm-lock.yaml')) };
}

test('pnpm adapter: installed manifests, alias identities and transitive lock edges agree', t => {
  const value = fixture(t), graph = value.load();
  assert.equal(graph.packages.size, 3);
  assert.deepEqual(traverse(graph, value.root, policy(path.join(value.root, 'deny.toml'))), []);
});
for (const change of [
  state => { state.evidence[0].dependencies.helper.from = 'wrong'; },
  state => { state.leaf.version = '2.0.0'; },
  state => { delete state.evidence[0].dependencies.helper.dependencies; },
  state => { state.lock = state.lock.replace('renamed: leaf@1.0.0', 'renamed: leaf@2.0.0'); },
  state => { state.lock = state.lock.replace('  leaf@1.0.0: {}\n', ''); },
  state => { state.evidence.length = 0; },
]) test(`pnpm adapter fails closed: ${change.toString()}`, t => assert.throws(() => fixture(t, change).load()));
test('pnpm adapter: missing installed package manifest fails', t => {
  const value = fixture(t);
  rmSync(path.join(value.leafPath, 'package.json'));
  assert.throws(value.load);
});
test('pnpm adapter: unversioned or unlicensed roots cannot bypass the traversal', t => {
  const value = fixture(t, state => { delete state.manifest.license; });
  assert.throws(() => traverse(value.load(), value.root, policy(path.join(value.root, 'deny.toml'))), /missing SPDX/);
});
test('pnpm adapter: external transitive copyleft and explicit MPL files fail', t => {
  const value = fixture(t, state => { state.leaf.license = 'MIT AND LGPL-2.1-or-later'; });
  assert.match(traverse(value.load(), value.root, policy(path.join(value.root, 'deny.toml'))).join('\n'), /not permitted/);
  const manifestFile = path.join(value.leafPath, 'package.json');
  const manifest = JSON.parse(readFileSync(manifestFile));
  manifest.license = 'MIT'; manifest.licenseFile = 'COPYING';
  writeFileSync(manifestFile, JSON.stringify(manifest));
  writeFileSync(path.join(value.leafPath, 'COPYING'), 'Mozilla Public License Version 2.0');
  assert.match(traverse(value.load(), value.root, policy(path.join(value.root, 'deny.toml'))).join('\n'), /MPL/);
});
test('pnpm adapter: permissive workspace links outside engine still fail', t => {
  const value = fixture(t);
  const directory = path.join(value.root, 'libraries/helper');
  mkdirSync(directory, { recursive: true });
  writeFileSync(path.join(directory, 'package.json'), JSON.stringify({ name: 'helper', version: '1.0.0', license: 'MIT' }));
  value.evidence[0].dependencies.helper = { from: 'helper', version: 'link:../../libraries/helper', path: directory };
  writeFileSync(path.join(value.root, 'graph.json'), JSON.stringify(value.evidence));
  rmSync(path.join(value.root, 'engine/ts/node_modules/helper'));
  symlinkSync(directory, path.join(value.root, 'engine/ts/node_modules/helper'));
  writeFileSync(path.join(value.root, 'pnpm-lock.yaml'), "lockfileVersion: '9.0'\nimporters:\n  engine/ts:\n    dependencies:\n      helper:\n        specifier: 1.0.0\n        version: link:../../libraries/helper\n  libraries/helper: {}\npackages: {}\nsnapshots: {}\n");
  assert.match(traverse(value.load(), value.root, policy(path.join(value.root, 'deny.toml'))).join('\n'), /crosses outside engine/);
});

test('pnpm adapter rejects broken links and peer contexts inconsistent with installed paths', t => {
  const value = fixture(t);
  rmSync(path.join(value.root, 'engine/ts/node_modules/helper'));
  assert.throws(value.load, /installed dependency link/);
  symlinkSync(value.helperPath, path.join(value.root, 'engine/ts/node_modules/helper'));
  const lockFile = path.join(value.root, 'pnpm-lock.yaml');
  const text = readFileSync(lockFile, 'utf8').replace('version: 1.0.0', 'version: 1.0.0(peer@1.0.0)').replace('  helper@1.0.0:\n', '  helper@1.0.0(peer@1.0.0):\n');
  writeFileSync(lockFile, text);
  assert.throws(value.load, /peer context/);
});

test('pnpm adapter rejects extra installed peer context absent from the lock', t => {
  const value = fixture(t), original = path.dirname(path.dirname(value.helperPath));
  const changed = original + '_peer@1.0.0', helperPath = path.join(changed, 'node_modules/helper');
  renameSync(original, changed);
  rmSync(path.join(value.root, 'engine/ts/node_modules/helper'));
  symlinkSync(helperPath, path.join(value.root, 'engine/ts/node_modules/helper'));
  value.evidence[0].dependencies.helper.path = helperPath;
  writeFileSync(path.join(value.root, 'graph.json'), JSON.stringify(value.evidence));
  assert.throws(value.load, /peer context/);
});
