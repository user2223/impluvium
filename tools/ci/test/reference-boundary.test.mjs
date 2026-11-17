import test from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { cpSync, mkdirSync, readFileSync, rmSync, symlinkSync, writeFileSync } from 'node:fs';
import { temporary } from './helpers.mjs';
import { checkManifest, installedGraphs, referencePath } from '../reference-boundary.mjs';
import { referenceCommand, referenceReceipt, testEvidence, referenceFailure, incompleteCoverageMessage } from '../reference-run.mjs';
import { attest } from './protocol-source-fixture.mjs';
import { check as licenceBoundary } from '../check-licence-boundary.mjs';
const repository = fileURLToPath(new URL('../../../', import.meta.url));
function fixture(t) {
  const root = temporary(t), ref = path.join(root, referencePath);
  cpSync(path.join(repository, referencePath), ref, { recursive: true, filter: file => file !== path.join(repository, referencePath, 'node_modules') });
  writeFileSync(path.join(root, 'pnpm-workspace.yaml'), `packages:\n  - ${referencePath}/\n`);
  return { root, ref };
}
function installed(t) {
  const value = fixture(t), { root, ref } = value;
  const pins = { '@noble/hashes': '1.8.0', '@types/node': '22.20.5', typescript: '5.9.3', 'undici-types': '6.21.0' }, dirs = {};
  const put = (file, text) => { mkdirSync(path.dirname(file), { recursive: true }); writeFileSync(file, text); };
  let lock = `lockfileVersion: '9.0'\nimporters:\n  ${referencePath}:\n    devDependencies:\n`;
  for (const [name, version] of Object.entries(pins).slice(0, 3)) lock += `      '${name}':\n        specifier: ${version}\n        version: ${version}\n`;
  lock += 'packages:\n';
  for (const [name, version] of Object.entries(pins)) lock += `  '${name}@${version}':\n    resolution: {integrity: sha512-${'a'.repeat(86)}==}\n`;
  lock += 'snapshots:\n';
  for (const [name, version] of Object.entries(pins)) {
    lock += name === '@types/node' ? `  '${name}@${version}':\n    dependencies:\n      undici-types: 6.21.0\n` : `  '${name}@${version}': {}\n`;
    dirs[name] = path.join(root, 'node_modules/.pnpm', `${name.replaceAll('/', '+')}@${version}`, 'node_modules', name);
    put(path.join(dirs[name], 'package.json'), JSON.stringify({ name, version, license: name === 'typescript' ? 'Apache-2.0' : 'MIT', ...(name === '@types/node' ? { dependencies: { 'undici-types': '~6.21.0' } } : {}) }));
  }
  put(path.join(dirs['@noble/hashes'], 'LICENSE'), readFileSync(new URL('./fixtures/noble-hashes-LICENSE', import.meta.url)));
  for (const name of Object.keys(pins).slice(0, 3)) {
    const link = path.join(ref, 'node_modules', name);
    mkdirSync(path.dirname(link), { recursive: true }); symlinkSync(dirs[name], link);
  }
  const link = path.join(path.dirname(dirs['@types/node']), '../undici-types');
  symlinkSync(dirs['undici-types'], link);
  writeFileSync(path.join(root, 'pnpm-lock.yaml'), lock);
  return { ...value, dirs, lock };
}
test('reference package is private, pinned, unexported, explicitly installed, and has exactly eight scripts', () => {
  assert.equal(checkManifest(repository).manifest.name, 'impluvium-protocol-reference');
});
for (const change of [
  manifest => { manifest.exports = './src/index.ts'; },
  manifest => { manifest.private = false; },
  manifest => { manifest.scripts.prepare = 'node installer.mjs'; },
  manifest => { manifest.devDependencies['@noble/hashes'] = '^1.8.0'; },
  manifest => { manifest.dependencies = { 'impluvium-protocol': 'workspace:*' }; },
  manifest => { manifest.engines.node = '^22.0.0'; },
]) test(`reference manifest rejects ${change.toString()}`, t => {
  const { root, ref } = fixture(t), manifest = JSON.parse(readFileSync(path.join(ref, 'package.json')));
  change(manifest); writeFileSync(path.join(ref, 'package.json'), JSON.stringify(manifest));
  assert.throws(() => checkManifest(root), /manifest differs/);
});
test('the exact delivered command receives --out and no attestation-only option', t => {
  const { root, ref } = fixture(t);
  for (const name of ['imports', 'corpus', 'coverage', 'primitives', 'vectors', 'limits', 'counterexamples']) {
    assert.deepEqual(referenceCommand(root, name), [path.join(ref, 'tooling/check.mjs'), name, '--out', '/work/results/reference/' + name]);
  }
  assert.deepEqual(referenceCommand(root, 'vectors', ['--stage', '4']), [path.join(ref, 'tooling/check.mjs'), 'vectors', '--out', '/work/results/reference/vectors', '--stage', '4']);
  assert.deepEqual(referenceCommand(root, 'container'), [path.join(ref, 'container/run.mjs'), '--out', '/work/results/reference/container']);
  rmSync(path.join(ref, 'tooling/check.mjs'));
  assert.throws(() => referenceCommand(root, 'corpus'), /reference content is incomplete/);
});
test('one root-lock binds both the runtime and separate transitive type-tool graph', t => {
  const value = installed(t), graph = installedGraphs(value.root);
  assert.equal(graph.runtime.length, 1); assert.equal(graph.typeTools.length, 3);
  assert.equal(graph.edges.length, 4);
});
for (const mutation of [
  value => writeFileSync(path.join(value.root, 'pnpm-lock.yaml'), value.lock.replace('version: 1.8.0', 'version: 1.8.1')),
  value => writeFileSync(path.join(value.root, 'pnpm-lock.yaml'), value.lock.replace('undici-types: 6.21.0', 'undici-types: 6.22.0')),
  value => writeFileSync(path.join(value.root, 'pnpm-lock.yaml'), value.lock.replace('specifier: 1.8.0', 'specifier: npm:other@1.8.0')),
  value => { rmSync(path.join(value.ref, 'node_modules/@noble/hashes')); symlinkSync(value.ref, path.join(value.ref, 'node_modules/@noble/hashes')); },
  value => writeFileSync(path.join(value.dirs['@noble/hashes'], 'package.json'), '{"name":"wrong","version":"1.8.0"}'),
]) test(`installed graph rejects ${mutation.toString()}`, t => {
  const value = installed(t); mutation(value); assert.throws(() => installedGraphs(value.root));
});
test('nested locks and symlinks cannot become reference inputs', t => {
  const { root, ref } = fixture(t);
  writeFileSync(path.join(ref, 'pnpm-lock.yaml'), '');
  assert.throws(() => checkManifest(root), /only the root lock/);
  rmSync(path.join(ref, 'pnpm-lock.yaml'));
  symlinkSync(path.join(repository, 'package.json'), path.join(ref, 'product.json'));
  assert.throws(() => checkManifest(root), /symlink/);
});
test('reference receipts require the independently named commit, even for unchanged bytes', t => {
  const root = temporary(t);
  writeFileSync(path.join(root, 'input.txt'), 'source');
  const receipt = attest(root);
  assert.equal(referenceReceipt(root, receipt, receipt.commit), receipt);
  assert.throws(() => referenceReceipt(root, receipt), /independently supplied/);
  assert.throws(() => referenceReceipt(root, receipt, 'a'.repeat(40)), /source receipt commit/);
});
test('JSON contracts retain pending status and require actual unique executed IDs', () => {
  const report = { format: 'executed-contract-v2', results: [{ id: 'sample', result: 'pass' }, { id: 'waiting', result: 'pending' }], counts: { pass: 1, pending: 1 }, expectationsAccepted: false };
  assert.equal(testEvidence(report, 'vectors').pending, 1);
  for (const change of [
    value => { value.results = []; },
    value => { value.results[1].id = 'sample'; },
    value => { value.counts.pass = 2; },
    value => { value.expectationsAccepted = true; },
    value => { value.results[0].result = 'skipped'; },
  ]) {
    const value = structuredClone(report); change(value);
    assert.throws(() => testEvidence(value, 'vectors'));
  }
  assert.throws(() => testEvidence({ status: 'deferred', reason: 'requirements pending' }, 'counterexamples'), /requirements pending/);
  assert.throws(() => testEvidence({ format: 'counterexamples', executedMutations: 0 }, 'counterexamples'), /counterexamples are empty/);
});

test('confirmed expectations remain accepted while hash execution is pending', () => {
  const confirmation = { id: 'provenance.independent-calculations', result: 'pass', evidence: { fields: 1, confirmed: 1, pending: 0, expectationsAccepted: true } };
  const report = { format: 'executed-contract-v2', results: [confirmation, { id: 'hash-pending', result: 'pending' }], counts: { pass: 1, pending: 1 }, expectationsAccepted: true };
  assert.deepEqual(testEvidence(report, 'vectors').pending, 1);
  for (const mutate of [
    value => { value.results[0].evidence.confirmed = 0; },
    value => { value.results[0].evidence.pending = 1; },
    value => { value.results[0].result = 'pending'; },
    value => { value.results.shift(); },
  ]) { const copy = structuredClone(report); mutate(copy); assert.throws(() => testEvidence(copy, 'vectors'), /acceptance lacks complete confirmation/); }
});

for (const license of ['Apache-2.0', 'GPL-3.0-only', 'MIT OR Apache-2.0', undefined]) test(`the reviewed dependency requires exactly MIT, not ${license}`, t => {
  const value = installed(t), file = path.join(value.dirs['@noble/hashes'], 'package.json');
  const manifest = JSON.parse(readFileSync(file)); manifest.license = license;
  writeFileSync(file, JSON.stringify(manifest));
  assert.throws(() => installedGraphs(value.root), /version\/licence differs/);
});
for (const mutation of ['missing', 'changed', 'symlink']) test(`the reviewed dependency rejects a ${mutation} licence notice`, t => {
  const value = installed(t), file = path.join(value.dirs['@noble/hashes'], 'LICENSE');
  rmSync(file);
  if (mutation === 'changed') writeFileSync(file, 'Different terms');
  if (mutation === 'symlink') symlinkSync(path.join(value.ref, 'package.json'), file);
  assert.throws(() => installedGraphs(value.root));
});

test('the repository licence gate enforces the exact reviewed Noble notice', t => {
  const value = installed(t), { root, ref, dirs } = value;
  cpSync(new URL('../../../deny.toml', import.meta.url), path.join(root, 'deny.toml'));
  const manifestFile = path.join(root, 'engine/core/Cargo.toml');
  mkdirSync(path.dirname(manifestFile), { recursive: true });
  writeFileSync(manifestFile, '[package]\nname = "example"\nversion = "1.0.0"\nlicense = "0BSD"\n');
  const metadata = path.join(root, 'metadata.json'), npm = path.join(root, 'pnpm-graph.json');
  writeFileSync(metadata, JSON.stringify({ version: 1, workspace_root: root, workspace_members: ['core'],
    packages: [{ id: 'core', name: 'example', manifest_path: manifestFile, license: '0BSD' }], resolve: { nodes: [{ id: 'core', dependencies: [] }] } }));
  const entry = name => ({ from: name, version: JSON.parse(readFileSync(path.join(dirs[name], 'package.json'))).version, path: dirs[name] });
  const node = entry('@types/node'); node.dependencies = { 'undici-types': entry('undici-types') };
  writeFileSync(npm, JSON.stringify([{ name: 'impluvium-protocol-reference', version: '0.1.0', path: ref,
    devDependencies: { '@noble/hashes': entry('@noble/hashes'), '@types/node': node, typescript: entry('typescript') } }]));
  const options = { root, metadata, 'pnpm-graph': npm, 'pnpm-lock': path.join(root, 'pnpm-lock.yaml') };
  assert.deepEqual(licenceBoundary(options), []);
  writeFileSync(path.join(dirs['@noble/hashes'], 'LICENSE'), 'Altered notice');
  assert.throws(() => licenceBoundary(options), /licence notice differs/);
});

test('pending coverage and deferred counterexamples retain specific failing diagnostics', () => {
  const report = { format: 'coverage-contract-v2', groups: [{ file: 'cases.json', count: 1 }], cases: 1, complete: false, expectationsAccepted: false };
  assert.equal(referenceFailure('coverage', report), incompleteCoverageMessage);
  assert.throws(() => testEvidence(report, 'coverage'), /reference coverage incomplete/);
  const reason = 'Behavioral counterexamples require confirmed expectations and complete specification conditions.';
  assert.equal(referenceFailure('counterexamples', { status: 'deferred', reason }), reason);
});
