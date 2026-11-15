import test from 'node:test';
import assert from 'node:assert/strict';
import { appendFileSync, cpSync, mkdirSync, rmSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { invoke, replace, repository, temporary } from './helpers.mjs';

function graph(t, transform = () => {}) {
  const root = repository(t);
  cpSync(new URL('../../../deny.toml', import.meta.url), path.join(root, 'deny.toml'));
  mkdirSync(path.join(root, 'engine/npm'), { recursive: true });
  writeFileSync(path.join(root, 'engine/npm/package.json'), JSON.stringify({ name: 'types', version: '1.0.0', license: '0BSD' }));
  writeFileSync(path.join(root, 'pnpm-workspace.yaml'), 'packages:\n  - engine/npm\n');
  writeFileSync(path.join(root, 'pnpm-lock.yaml'), "lockfileVersion: '9.0'\nimporters:\n  engine/npm: {}\npackages: {}\nsnapshots: {}\n");
  writeFileSync(path.join(root, 'pnpm-graph.json'), JSON.stringify([{ name: 'types', version: '1.0.0', path: path.join(root, 'engine/npm') }]));
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
  return ['--root', root, '--metadata', metadata, '--pnpm-graph', path.join(root, 'pnpm-graph.json'), '--pnpm-lock', path.join(root, 'pnpm-lock.yaml')];
}

test('licence boundary: unreachable MPL packages are allowed in resolved metadata', t => {
  invoke('check-licence-boundary', graph(t), 0);
});

for (const license of ['MIT/Apache-2.0', 'MIT OR Apache-2.0 OR LGPL-2.1-or-later', 'MIT OR MPL-2.0']) {
  test(`licence boundary: usable permissive alternative in ${license}`, t => {
    invoke('check-licence-boundary', graph(t, data => { data.packages[3].license = license; }), 0);
  });
}

for (const license of [null, 'MIT OR Unknown-License', 'MIT AND', '(MIT']) {
  test(`licence boundary: missing or invalid expression ${license} fails closed`, t => {
    invoke('check-licence-boundary', graph(t, data => { data.packages[3].license = license; }), 2);
  });
}

for (const index of [0, 1, 3]) test(`licence boundary: reachable MPL expressions fail at graph package ${index}`, t => {
  for (const license of ['MPL-2.0', '(MPL-2.0 AND Apache-2.0)']) {
    const args = graph(t, data => { data.packages[index].license = license; });
    assert.match(invoke('check-licence-boundary', args, 1).stderr, /not permitted/);
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
for (const name of ['target', 'dist', 'node_modules']) test(`licence boundary: ${name} cannot hide an independent engine workspace`, t => {
  const args = graph(t, (data, root) => {
    const directory = path.join(root, 'engine', name, 'extra'); mkdirSync(directory, { recursive: true });
    writeFileSync(path.join(directory, 'Cargo.toml'), '[workspace]\n[package]\nname = "extra"\nversion = "0.1.0"\n[dependencies]\nring = "0.17"\n');
  });
  assert.match(invoke('check-licence-boundary', args, 2).stderr, /engine manifest missing from metadata/);
});
test('licence boundary: a Node manifest under target cannot escape importer coverage', t => {
  const args = graph(t, (data, root) => {
    const directory = path.join(root, 'engine/target/extra'); mkdirSync(directory, { recursive: true });
    writeFileSync(path.join(directory, 'package.json'), '{"name":"extra","version":"1.0.0","license":"AGPL-3.0-only"}');
  });
  assert.match(invoke('check-licence-boundary', args, 2).stderr, /engine package is not a workspace member/);
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
  assert.match(result.stderr, /not permitted/);
});

for (const license of ['GPL-3.0-only', 'LGPL-2.1-or-later', 'AGPL-3.0-only', 'MIT AND MPL-2.0']) {
  test(`licence boundary: reachable registry expression ${license} is forbidden`, t => {
    const args = graph(t, data => { data.packages[3].license = license; });
    assert.match(invoke('check-licence-boundary', args, 1).stderr, /not permitted/);
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

test('licence boundary: a standalone engine root cannot select an empty traversal', t => {
  const args = graph(t, (data, root) => {
    data.packages = data.packages.filter(pkg => pkg.id === 'core');
    data.packages[0].license = 'MPL-2.0';
    data.workspace_members = ['core'];
    data.workspace_root = path.join(root, 'engine');
    data.resolve.nodes = [{ id: 'core', dependencies: [] }];
    rmSync(path.join(root, 'engine/shared'), { recursive: true });
  });
  args[1] = path.join(args[1], 'engine');
  assert.match(invoke('check-licence-boundary', args, 2).stderr, /no engine\/ packages selected/);
});

test('licence boundary: no engine packages always fails', t => {
  const args = graph(t, (data, root) => {
    rmSync(path.join(root, 'engine'), { recursive: true });
    data.packages = data.packages.filter(pkg => pkg.id === 'service');
    data.workspace_members = ['service'];
    data.resolve.nodes = [{ id: 'service', dependencies: [] }];
  });
  assert.match(invoke('check-licence-boundary', args, 2).stderr, /no engine\/ packages selected/);
  invoke('check-licence-boundary', [...args, '--allow-no-engine'], 2);
});

test('licence boundary: removed empty-engine option is rejected', t => {
  const incomplete = graph(t, data => {
    data.packages = data.packages.filter(pkg => pkg.id === 'service');
    data.workspace_members = ['service'];
    data.resolve.nodes = [{ id: 'service', dependencies: [] }];
  });
  assert.match(invoke('check-licence-boundary', [...incomplete, '--allow-no-engine'], 2).stderr, /Unknown option/);
  const forbidden = graph(t, data => { data.packages[0].license = 'MPL-2.0'; });
  assert.match(invoke('check-licence-boundary', [...forbidden, '--allow-no-engine'], 2).stderr, /Unknown option/);
});

test('licence boundary: virtual engine workspace manifests need no metadata package', t => {
  const args = graph(t, (data, root) => {
    writeFileSync(path.join(root, 'engine/Cargo.toml'), '[workspace]\nmembers = ["core", "shared"]\n');
    data.workspace_root = path.join(root, 'engine');
  });
  invoke('check-licence-boundary', args, 0);
});

test('licence boundary: a workspace with a package still requires metadata coverage', t => {
  const args = graph(t, (data, root) => {
    writeFileSync(path.join(root, 'engine/Cargo.toml'), '[workspace]\nmembers = ["core"]\n[package]\nname = "workspace-package"\nversion = "1.0.0"\n');
  });
  assert.match(invoke('check-licence-boundary', args, 2).stderr, /engine\/Cargo.toml:1: engine manifest missing/);
});
