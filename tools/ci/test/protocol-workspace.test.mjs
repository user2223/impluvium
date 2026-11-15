import test from 'node:test';
import assert from 'node:assert/strict';
import { workspacePaths, engineCoverage } from '../protocol-workspace.mjs';
import { mkdirSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { attest } from './protocol-source-fixture.mjs';
import { temporary } from './helpers.mjs';
const data = { profiles: {}, enginePackages: {} };

function fixture(withEngine = false) {
  const root = '/synthetic/source';
  const packages = [
    ['legacy', 'desktop/client/Cargo.toml'], ['engine', 'engine/crates/client/Cargo.toml'],
    ['left', 'registry/left/Cargo.toml'], ['right', 'registry/right/Cargo.toml'],
    ['ring', 'registry/ring/Cargo.toml'], ['harmless', 'registry/harmless/Cargo.toml'],
  ].filter(([id]) => withEngine || id !== 'engine').map(([id, manifest]) => ({ id, name: id, version: '1.0.0', source: id === 'legacy' || id === 'engine' ? null : 'registry', manifest_path: `${root}/${manifest}` }));
  const dep = (pkg, name, kind = null, target = null) => ({ pkg, name, dep_kinds: [{ kind, target }] });
  const nodes = [
    { id: 'legacy', features: [], deps: [dep('left', 'build_alias', 'build', 'cfg(windows)'), dep('right', 'runtime_alias'), dep('harmless', 'harmless')] },
    { id: 'engine', features: [], deps: [dep('ring', 'ring')] },
    { id: 'left', features: [], deps: [dep('ring', 'crypto', null, 'cfg(unix)')] },
    { id: 'right', features: [], deps: [dep('ring', 'test_crypto', 'dev')] },
    { id: 'ring', features: [], deps: [] },
    { id: 'harmless', features: [], deps: [] },
  ];
  return { root, source: { files: [] }, metadata: { workspace_root: root, workspace_members: withEngine ? ['legacy', 'engine'] : ['legacy'], packages, resolve: { nodes: nodes.filter(node => withEngine || node.id !== 'engine') } } };
}

test('workspace report preserves every inherited path and edge', () => {
  const target = 'all';
  const { root, metadata, source } = fixture(), paths = workspacePaths(metadata, target, root, data, source);
  assert.equal(paths.length, 2);
  assert.deepEqual(paths.map(item => item.path.map(pkg => pkg.name)).sort(), [['legacy', 'left', 'ring'], ['legacy', 'right', 'ring']]);
  for (const item of paths) {
    assert.equal(item.root, 'desktop/client/Cargo.toml');
    assert.equal(item.target, target);
    assert.equal(item.reason, 'package:ring');
    assert.equal(item.removalOwner, 'desktop client migration');
    assert.equal(item.path.at(-1).source, 'registry');
    assert.equal(item.path.at(-1).version, '1.0.0');
  }
  const left = paths.find(item => item.path[1].name === 'left');
  assert.deepEqual(left.edges[0], { alias: 'build_alias', kind: 'build', target: 'cfg(windows)' });
  assert.equal(left.edges[1].target, 'cfg(unix)');
  assert.equal(paths.find(item => item.path[1].name === 'right').edges[1].kind, 'dev');
});

test('workspace path enumeration terminates cycles without losing distinct paths', () => {
  const { root, metadata, source } = fixture();
  metadata.resolve.nodes.find(node => node.id === 'left').deps.push({ pkg: 'legacy', name: 'cycle', dep_kinds: [{ kind: 'dev', target: null }] });
  assert.equal(workspacePaths(metadata, 'all', root, data, source).length, 2);
});

test('workspace reports reject a different export and incomplete raw dependency graphs', () => {
  const { root, metadata, source } = fixture();
  assert.throws(() => workspacePaths(metadata, 'all', '/other/source', data, source), /different export/);
  delete metadata.resolve.nodes.find(node => node.id === 'harmless').deps;
  assert.throws(() => workspacePaths(metadata, 'all', root, data, source), /incomplete workspace/);
});

test('an engine root reaching ring outside every product profile is rejected, not skipped', () => {
  const { root, metadata, source } = fixture(true);
  assert.throws(() => workspacePaths(metadata, 'all', root, data, source), /unassigned engine package: engine/);
});
test('only the explicit tool package can bypass product profile assignment', () => {
  const { root, metadata, source } = fixture(true), pkg = metadata.packages.find(pkg => pkg.id === 'engine');
  const classified = { profiles: {}, enginePackages: { engine: { manifest: 'engine/crates/client/Cargo.toml', kind: 'tool' } } };
  assert.throws(() => workspacePaths(metadata, 'all', root, classified, source), /unassigned/);
  pkg.name = 'impluvium-protocol-xtask'; pkg.manifest_path = `${root}/engine/protocol/xtask/Cargo.toml`;
  classified.enginePackages = { 'impluvium-protocol-xtask': { manifest: 'engine/protocol/xtask/Cargo.toml', kind: 'tool' } };
  assert.equal(workspacePaths(metadata, 'all', root, classified, source).length, 2);
});
test('an engine manifest omitted from Cargo metadata cannot evade profile assignment', t => {
  const root = temporary(t); mkdirSync(path.join(root, 'engine/extra'), { recursive: true });
  writeFileSync(path.join(root, 'engine/extra/Cargo.toml'), '[package]\nname="extra"\nversion="0.1.0"\n');
  assert.throws(() => engineCoverage({ packages: [] }, root, data, attest(root)), /manifest missing from metadata/);
});
for (const directory of ['engine/target/extra', 'engine/dist/extra']) test(`attested standalone workspace under ${directory} needs explicit coverage`, t => {
  const root = temporary(t); mkdirSync(path.join(root, directory), { recursive: true });
  writeFileSync(path.join(root, directory, 'Cargo.toml'), '[workspace]\n[package]\nname = "extra"\nversion = "0.1.0"\n[dependencies]\nring = "0.17"\n');
  assert.throws(() => engineCoverage({ workspace_root: root, packages: [] }, root, data, attest(root)), /unassigned engine manifest missing from metadata/);
});
