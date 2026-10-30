import test from 'node:test';
import assert from 'node:assert/strict';
import { appendFileSync, mkdirSync, rmSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { invoke, replace, repository, temporary } from './helpers.mjs';

function graph(t, transform = () => {}) {
  const root = repository(t);
  const packages = [
    ['core', 'engine/core', 'Apache-2.0'],
    ['shared', 'engine/shared', 'MIT'],
    ['service', 'server/crates/service', 'MPL-2.0'],
    ['registry-helper', 'registry/helper', 'MIT'],
  ].map(([name, directory, license]) => ({ id: name, name, manifest_path: path.join(root, directory, 'Cargo.toml'), license, license_file: null }));
  mkdirSync(path.join(root, 'engine/shared'), { recursive: true });
  writeFileSync(path.join(root, 'engine/shared/Cargo.toml'), '[package]\nname = "shared"\nversion = "1.0.0"\nlicense = "MIT"\n');
  packages[3].manifest_path = path.join(temporary(t), 'registry/helper/Cargo.toml');
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
      const directory = path.dirname(data.packages[3].manifest_path);
      mkdirSync(directory, { recursive: true });
      data.packages[3].license_file = name;
      writeFileSync(path.join(directory, name), content);
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
  assert.match(invoke('check-licence-boundary', args, 1).stderr, /engine\/shared\/Cargo.toml -> server\/crates/);
});

test('licence boundary: renamed workspace dependencies patched to server use resolved IDs', t => {
  const args = graph(t, (data, root) => {
    replace(root, 'Cargo.toml', 'shared = { path = "shared" }', 'shared = { package = "service", version = "1.0.0" }');
    appendFileSync(path.join(root, 'Cargo.toml'), '\n[patch.crates-io]\nservice = { path = "server/crates/service" }\n');
    data.resolve.nodes[0].dependencies = ['service'];
    data.resolve.nodes[0].deps = [{ name: 'shared', pkg: 'service', dep_kinds: [{ kind: null, target: null }] }];
  });
  assert.match(invoke('check-licence-boundary', args, 1).stderr, /crosses outside engine/);
});

test('licence boundary: empty packages or workspace members are input errors', t => {
  for (const emptyPackages of [true, false]) {
    const args = graph(t, data => {
      data.workspace_members = [];
      if (emptyPackages) { data.packages = []; data.resolve.nodes = []; }
    });
    assert.match(invoke('check-licence-boundary', args, 2).stderr, /metadata must contain workspace members/);
  }
});

test('licence boundary: separate engine workspaces still require the repository root', t => {
  const args = graph(t, (data, root) => { data.workspace_root = path.join(root, 'engine'); });
  assert.match(invoke('check-licence-boundary', args.slice(2), 2).stderr, /--root is required/);
  invoke('check-licence-boundary', args, 0);
});

test('licence boundary: foreign mount paths and a wrong root are input errors', t => {
  const args = graph(t, data => { data.packages[0].manifest_path = '/work/elsewhere/engine/core/Cargo.toml'; });
  assert.match(invoke('check-licence-boundary', args, 2).stderr, /workspace member manifest is outside --root/);
  const other = graph(t);
  other[1] = path.join(other[1], 'engine');
  assert.match(invoke('check-licence-boundary', other, 2).stderr, /workspace member manifest is outside --root/);
});

test('licence boundary: every engine manifest on disk must appear in metadata', t => {
  const args = graph(t, (data, root) => {
    mkdirSync(path.join(root, 'engine/missing'), { recursive: true });
    writeFileSync(path.join(root, 'engine/missing/Cargo.toml'), '[package]\nname = "missing"\n');
  });
  assert.match(invoke('check-licence-boundary', args, 2).stderr, /engine\/missing\/Cargo.toml:1: engine manifest missing/);
});

test('licence boundary: metadata without engine packages cannot skip engine manifests', t => {
  const args = graph(t, data => {
    data.workspace_members = ['service'];
    data.packages = data.packages.filter(pkg => pkg.id === 'service');
    data.resolve.nodes = [{ id: 'service', dependencies: [] }];
  });
  assert.match(invoke('check-licence-boundary', args, 2).stderr, /engine manifest missing from metadata/);
});

test('licence boundary: permissive repository dependencies outside engine are forbidden', t => {
  const args = graph(t, (data, root) => {
    rmSync(path.join(root, 'engine/shared'), { recursive: true });
    data.packages[1].manifest_path = path.join(root, 'shared/Cargo.toml');
  });
  assert.match(invoke('check-licence-boundary', args, 1).stderr, /crosses outside engine/);
});

test('licence boundary: inherited copyleft paths outside the server tree are forbidden', t => {
  const args = graph(t, (data, root) => {
    rmSync(path.join(root, 'engine/shared'), { recursive: true });
    data.packages[1].manifest_path = path.join(root, 'libraries/archive/Cargo.toml');
    data.packages[1].license = 'AGPL-3.0-only';
  });
  const result = invoke('check-licence-boundary', args, 1);
  assert.match(result.stderr, /crosses outside engine/);
  assert.match(result.stderr, /AGPL-licensed crate/);
});

for (const license of ['GPL-3.0-only', 'LGPL-2.1-or-later', 'AGPL-3.0-only', 'MIT OR MPL-2.0']) {
  test(`licence boundary: reachable registry expression ${license} is forbidden`, t => {
    const args = graph(t, data => { data.packages[3].license = license; });
    assert.match(invoke('check-licence-boundary', args, 1).stderr, /licensed crate is reachable/);
  });
}

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
