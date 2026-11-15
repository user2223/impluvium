import test from 'node:test';
import assert from 'node:assert/strict';
import { workspaceRequirements, requirementIssues } from '../protocol-requirements.mjs';
import { workspacePaths } from '../protocol-workspace.mjs';

const root = '/source';
const dependency = (name, features, target = null) => ({ name, rename: null, kind: null, target, features, optional: false, uses_default_features: false });
function fixture(dependencies, features = {}) {
  const name = dependencies[0].name;
  return { workspace_root: root, workspace_members: ['local'], packages: [
    { id: 'local', name: 'torrential', source: null, version: '1.0.0', manifest_path: `${root}/torrential/Cargo.toml`, dependencies, features },
    { id: 'remote', name, source: 'registry', version: '1.0.0', manifest_path: `/registry/${name}-1.0.0/Cargo.toml` },
  ], resolve: { nodes: [
    { id: 'local', features: [], deps: dependencies.map(dep => ({ name: dep.name, pkg: 'remote', dep_kinds: [{ kind: dep.kind, target: dep.target }] })) },
    { id: 'remote', features: name === 'reqwest' ? ['json', 'system-proxy'] : ['aws-lc-rs', 'ring'], deps: [] },
  ] } };
}
for (const name of ['reqwest', 'rustls']) test(`local ${name} activation cannot hide in an unchanged globally unified graph`, () => {
  // The inherited reqwest consumer adds system-proxy; rustls already has ring
  // only under cfg(windows), then also requests it unconditionally.
  const base = fixture(name === 'reqwest' ? [dependency(name, ['json'])] :
    [dependency(name, ['aws-lc-rs']), dependency(name, ['ring'], 'cfg(windows)')]);
  const current = structuredClone(base);
  current.packages[0].dependencies[0].features.push(name === 'reqwest' ? 'system-proxy' : 'ring');
  const data = { profiles: {}, enginePackages: {} }, source = { files: [] };
  assert.deepEqual(workspacePaths(base, 'all', root, data, source), workspacePaths(current, 'all', root, data, source));
  const issues = requirementIssues(workspaceRequirements(base, root), workspaceRequirements(current, root)).join('\n');
  assert.match(issues, /legacy requirement added or expanded/);
  assert.ok(issues.includes(`added features ["${name === 'reqwest' ? 'system-proxy' : 'ring'}"]`));
});
test('requirement diagnostics name changed switches', () => {
  const base = fixture([dependency('rustls', ['std'])]), current = structuredClone(base);
  base.packages[0].dependencies[0].optional = true;
  current.packages[0].dependencies[0].uses_default_features = true;
  const message = requirementIssues(workspaceRequirements(base, root), workspaceRequirements(current, root)).join('\n');
  assert.match(message, /optional: true -> false/);
  assert.match(message, /uses_default_features: false -> true/);
});
for (const request of ['dep:rustls', 'rustls/ring', 'rustls?/ring', 'transport']) test(`new local feature forwarding or indirection ${request} is frozen`, () => {
  const base = fixture([dependency('rustls', ['aws-lc-rs'])], { transport: ['rustls/aws-lc-rs'], default: [] });
  base.packages[0].dependencies[0].optional = true;
  const current = structuredClone(base); current.packages[0].features.default.push(request);
  assert.equal(requirementIssues(workspaceRequirements(base, root), workspaceRequirements(current, root)).length, 1);
});
test('removal and restriction are allowed; expanding defaults, optionality, kinds or target conditions is not', () => {
  const base = fixture([dependency('rustls', ['std', 'aws-lc-rs'])], { transport: ['rustls/ring'] });
  base.packages[0].dependencies[0].uses_default_features = true;
  const narrow = structuredClone(base), dep = narrow.packages[0].dependencies[0];
  dep.optional = true; dep.uses_default_features = false; dep.features = ['std'];
  narrow.packages[0].features.transport = ['rustls?/ring'];
  assert.deepEqual(requirementIssues(workspaceRequirements(base, root), workspaceRequirements(narrow, root)), []);
  assert.deepEqual(requirementIssues(workspaceRequirements(base, root), []), []);
  for (const change of [dep => { dep.uses_default_features = true; }, dep => { dep.optional = false; },
    dep => { dep.target = 'cfg(unix)'; }, dep => { dep.kind = 'build'; }, dep => { dep.rename = 'other'; }]) {
    const changed = structuredClone(narrow); change(changed.packages[0].dependencies[0]);
    assert.ok(requirementIssues(workspaceRequirements(narrow, root), workspaceRequirements(changed, root)).length);
  }
});
test('engine declarations are excluded, and missing or duplicate requirement data fails closed', () => {
  const value = fixture([dependency('reqwest', [])]), rows = workspaceRequirements(value, root);
  const engine = structuredClone(value.packages[0]); engine.id = 'engine'; engine.manifest_path = `${root}/engine/new/Cargo.toml`;
  value.packages.push(engine); value.workspace_members.push(engine.id);
  assert.deepEqual(workspaceRequirements(value, root), rows);
  for (const baseline of [null, [], [...rows, ...rows]]) assert.throws(() => requirementIssues(baseline, rows));
  delete value.packages[0].dependencies[0].uses_default_features;
  assert.throws(() => workspaceRequirements(value, root), /incomplete declared dependency/);
});
