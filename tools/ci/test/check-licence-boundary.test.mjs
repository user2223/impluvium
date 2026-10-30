import test from 'node:test';
import assert from 'node:assert/strict';
import { appendFileSync, mkdirSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { invoke, replace, repository } from './helpers.mjs';

function graph(t, transform = () => {}) {
  const root = repository(t);
  const packages = [
    ['core', 'engine/core', 'Apache-2.0'],
    ['shared', 'shared', 'MIT'],
    ['service', 'server/crates/service', 'MPL-2.0'],
    ['registry-helper', 'registry/helper', 'MIT'],
  ].map(([name, directory, license]) => ({ id: name, name, manifest_path: path.join(root, directory, 'Cargo.toml'), license, license_file: null }));
  const data = {
    version: 1, workspace_root: root, workspace_members: ['core', 'shared', 'service'], packages,
    resolve: { nodes: [
      { id: 'core', dependencies: ['shared'] },
      { id: 'shared', dependencies: ['registry-helper'] },
      { id: 'service', dependencies: [] },
      { id: 'registry-helper', dependencies: [] },
    ] },
  };
  transform(data, root);
  const metadata = path.join(root, 'metadata.json');
  writeFileSync(metadata, JSON.stringify(data));
  return ['--root', root, '--metadata', metadata];
}

test('licence boundary: unreachable MPL packages are allowed in resolved metadata', t => {
  invoke('check-licence-boundary', graph(t), 0);
});

for (const index of [0, 1, 3]) test(`licence boundary: reachable MPL expressions fail at graph package ${index}`, t => {
  for (const license of ['MPL-2.0', 'MIT OR MPL-2.0', '(MPL-2.0 AND Apache-2.0)']) {
    const args = graph(t, data => { data.packages[index].license = license; });
    assert.match(invoke('check-licence-boundary', args, 1).stderr, /MPL-licensed crate/);
  }
});

test('licence boundary: reachable registry license_file content and MPL filenames fail', t => {
  for (const [name, content] of [['COPYING.txt', 'Mozilla Public License Version 2.0\n'], ['MPL-2.0.txt', 'license notice\n']]) {
    const args = graph(t, (data, root) => {
      mkdirSync(path.join(root, 'registry/helper'), { recursive: true });
      data.packages[3].license_file = name;
      writeFileSync(path.join(root, 'registry/helper', name), content);
    });
    assert.match(invoke('check-licence-boundary', args, 1).stderr, /MPL/);
  }
});

test('licence boundary: conventional engine ancestor licence notices remain checked', t => {
  const args = graph(t, (data, root) => writeFileSync(path.join(root, 'engine/LICENSE'), 'Mozilla Public License\n'));
  assert.match(invoke('check-licence-boundary', args, 1).stderr, /engine\/LICENSE:1:/);
});

test('licence boundary: transitive server edges are checked and cycles terminate', t => {
  const args = graph(t, data => { data.resolve.nodes[1].dependencies.push('core', 'service'); });
  assert.match(invoke('check-licence-boundary', args, 1).stderr, /shared\/Cargo.toml -> server\/crates/);
});

test('licence boundary: renamed workspace dependencies patched to server use resolved IDs', t => {
  const args = graph(t, (data, root) => {
    replace(root, 'Cargo.toml', 'shared = { path = "shared" }', 'shared = { package = "service", version = "1.0.0" }');
    appendFileSync(path.join(root, 'Cargo.toml'), '\n[patch.crates-io]\nservice = { path = "server/crates/service" }\n');
    data.resolve.nodes[0].dependencies = ['service'];
    data.resolve.nodes[0].deps = [{ name: 'shared', pkg: 'service', dep_kinds: [{ kind: null, target: null }] }];
  });
  assert.match(invoke('check-licence-boundary', args, 1).stderr, /crosses into server\/crates/);
});

for (const kind of [null, 'dev', 'build']) test(`licence boundary: resolved ${kind ?? 'normal'} and target-specific dependency edges fail`, t => {
  const args = graph(t, data => {
    data.resolve.nodes[0].dependencies = ['service'];
    data.resolve.nodes[0].deps = [{ name: 'renamed', pkg: 'service', dep_kinds: [{ kind, target: 'cfg(target_os = "linux")' }] }];
  });
  invoke('check-licence-boundary', args, 1);
});

test('licence boundary: metadata is required and unresolved graphs are input errors', t => {
  invoke('check-licence-boundary', ['--root', repository(t)], 2);
  for (const transform of [data => { data.resolve = null; }, data => { data.resolve.nodes[0].dependencies.push('missing'); },
    data => { data.resolve.nodes.splice(1, 1); }, data => { data.packages[0].license_file = 'missing'; }]) {
    invoke('check-licence-boundary', graph(t, transform), 2);
  }
});
