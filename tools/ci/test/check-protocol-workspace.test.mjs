import test from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { evidence } from './protocol-fixture.mjs';
import { attest } from './protocol-source-fixture.mjs';
import { gitObject } from '../protocol-source.mjs';
import { hash } from '../protocol-contract.mjs';
import { workspacePaths } from '../protocol-workspace.mjs';
import { workspaceRequirements } from '../protocol-requirements.mjs';
import { check } from '../check-protocol-graph.mjs';
import { writeContract } from '../write-protocol-contract.mjs';

function fixture(t) {
  const value = evidence(t), manifest = path.join(value.root, 'legacy/Cargo.toml');
  mkdirSync(path.dirname(manifest)); writeFileSync(manifest, '[package]\nname = "legacy"\nversion = "1.0.0"\n');
  value.metadata.packages.push({ id: 'legacy', name: 'legacy', version: '1.0.0', source: null, manifest_path: manifest,
    features: { transport: ['blake3/std'] }, dependencies: [
      { name: 'blake3', rename: null, kind: null, target: null, optional: false, uses_default_features: false, features: ['std'] },
    ] });
  value.metadata.workspace_members.push('legacy');
  value.metadata.resolve.nodes.push({ id: 'legacy', features: [], dependencies: ['blake'], deps: [
    { name: 'blake3', pkg: 'blake', dep_kinds: [{ kind: null, target: null }] },
  ] });
  const source = attest(value.root), raw = path.join(value.input, 'all.json'), workspace = path.join(value.input, 'workspace.json');
  const requirements = workspaceRequirements(value.metadata, value.root);
  const report = { version: 3, scope: 'all-targets-conservative', root: value.root, source, sourceSha: source.commit, baseSha: source.commit,
    lockSha256: hash(readFileSync(path.join(value.root, 'Cargo.lock'))), requirements,
    paths: workspacePaths(value.metadata, 'all', value.root, value.data, source), metadata: {} };
  value.data.legacy = { baseSha: source.commit, exceptions: report.paths, requirements };
  writeFileSync(value.contractFile, JSON.stringify(value.data));
  const save = () => {
    const bytes = JSON.stringify(value.metadata); writeFileSync(raw, bytes);
    report.metadata.all = { file: raw, sha256: hash(Buffer.from(bytes)), arguments: ['metadata', '--locked', '--offline', '--all-features', '--format-version=1'] };
    writeFileSync(workspace, JSON.stringify(report));
  };
  save();
  return { ...value, report, workspace, save, options: { root: value.root, contract: value.contractFile, workspace, 'source-sha': source.commit } };
}
test('complete workspace proof replays paths and declared requirements', t => assert.deepEqual(check(fixture(t).options), []));
test('same tree with another expected commit rejects workspace evidence and contract writing', t => {
  const value = fixture(t), raw = Buffer.from(value.report.source.commitObject, 'base64');
  const other = gitObject('commit', Buffer.concat([raw, Buffer.from('Different commit message\n')]));
  assert.throws(() => check({ ...value.options, 'source-sha': other }), /source commit differs from expected/);
  for (const expected of [{ 'source-sha': other, 'base-sha': value.report.baseSha }, { 'source-sha': value.report.sourceSha, 'base-sha': other }]) {
    assert.throws(() => writeContract({ root: value.root, contract: value.contractFile, captures: '/unused', base: value.workspace,
      current: value.workspace, out: path.join(value.input, 'review.json'), ...expected }), /review a real base/);
  }
});
test('workspace requirements are replayed from raw metadata before checking the frozen requests', t => {
  const value = fixture(t);
  value.metadata.packages.at(-1).features.transport.push('blake3/new'); value.save();
  assert.throws(() => check(value.options), /requirements differ from raw declarations/);
  value.report.requirements = workspaceRequirements(value.metadata, value.root); value.save();
  assert.match(check(value.options).join('\n'), /legacy requirement added or expanded/);
});
test('a missing requirements baseline explicitly requires recapture', t => {
  const value = fixture(t); value.data.legacy.requirements = null;
  writeFileSync(value.contractFile, JSON.stringify(value.data));
  assert.throws(() => check(value.options), /requirements need recapture/);
});
