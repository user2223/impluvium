import test from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import { copyFileSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { temporary } from './helpers.mjs';
import { matrix, matrixSelections, previousBaselines } from '../protocol-matrix.mjs';
import { attest } from './protocol-source-fixture.mjs';

test('matrix selects exactly four profiles, three Linux runs and one Windows build each', () => {
  assert.equal(matrixSelections.length, 16);
  assert.equal(new Set(matrixSelections.map(row => JSON.stringify(row))).size, 16);
  for (const profile of ['core', 'server', 'client', 'combined']) {
    assert.deepEqual(matrixSelections.filter(row => row.profile === profile).map(row => [row.target, row.run]), [
      ['x86_64-unknown-linux-gnu', 'build'], ['x86_64-unknown-linux-gnu', 'test'],
      ['x86_64-unknown-linux-gnu', 'clippy'], ['x86_64-pc-windows-gnullvm', 'build'],
    ]);
  }
});

function comparisonFixture(t, firstRun = false) {
  const base = temporary(t), evidence = temporary(t), copies = path.join(evidence, 'copies');
  mkdirSync(path.join(base, 'tools/ci'), { recursive: true }); mkdirSync(copies);
  const names = ['clippy-baseline.json', ...['core', 'server', 'client', 'combined'].map(name => `protocol-tests-${name}.json`)];
  for (const name of firstRun ? names.slice(0, 1) : names) {
    const file = path.join(base, 'tools/ci', name);
    writeFileSync(file, '{"version":1,"packages":{"example":0}}\n'); copyFileSync(file, path.join(copies, name));
  }
  const source = attest(base), sourceSha = 'a'.repeat(40);
  const comparison = { version: 1, event: 'pull_request', currentSha: sourceSha, base: { status: 'available', sha: source.commit } };
  const context = path.join(evidence, 'comparison.json'), receipt = path.join(evidence, 'source.json');
  const save = () => { writeFileSync(context, JSON.stringify(comparison)); writeFileSync(receipt, JSON.stringify(source)); };
  save();
  return { copies, source, comparison, save, options: { 'source-sha': sourceSha, 'previous-baselines': copies,
    'previous-comparison': context, 'previous-source-receipt': receipt } };
}
test('comparison copies match the base catalogue and proven first-run absence is explicit', t => {
  const value = comparisonFixture(t), previous = previousBaselines(value.options);
  assert.deepEqual(previous('protocol-tests-core.json'), { 'previous-baseline': path.join(value.copies, 'protocol-tests-core.json') });
  const first = comparisonFixture(t, true), firstPrevious = previousBaselines(first.options);
  assert.deepEqual(firstPrevious('protocol-tests-core.json'), {});
  assert.ok(firstPrevious('clippy-baseline.json')['previous-baseline']);
});
for (const defect of ['missing directory', 'file instead of directory', 'missing copy', 'changed copy', 'omitted catalogue entry', 'different base', 'stale comparison', 'unattested copy']) {
  test(`comparison rejects ${defect}`, t => {
    const value = comparisonFixture(t, defect === 'unattested copy');
    if (defect === 'missing directory') value.options['previous-baselines'] += '/missing';
    if (defect === 'file instead of directory') value.options['previous-baselines'] = value.options['previous-comparison'];
    if (defect === 'missing copy') rmSync(path.join(value.copies, 'protocol-tests-core.json'));
    if (defect === 'changed copy') writeFileSync(path.join(value.copies, 'protocol-tests-core.json'), 'lowered baseline');
    if (defect === 'omitted catalogue entry') value.source.files = value.source.files.filter(file => !file.path.endsWith('protocol-tests-core.json'));
    if (defect === 'different base') value.comparison.base.sha = 'b'.repeat(40);
    if (defect === 'stale comparison') value.comparison.currentSha = 'b'.repeat(40);
    if (defect === 'unattested copy') writeFileSync(path.join(value.copies, 'protocol-tests-core.json'), 'unproven');
    value.save();
    assert.throws(() => previousBaselines(value.options), /previous baseline|source receipt|previous comparison/);
  });
}
test('an explicitly unavailable event base needs a reason and cannot reuse stale copies', t => {
  const value = comparisonFixture(t);
  value.comparison.event = 'workflow_dispatch'; value.comparison.base = { status: 'unavailable', sha: null, reason: 'This event provides no comparison base.' };
  value.save();
  assert.throws(() => previousBaselines(value.options), /not in the attested base/);
  rmSync(value.copies, { recursive: true }); mkdirSync(value.copies);
  assert.deepEqual(previousBaselines(value.options)('clippy-baseline.json'), {});
  value.comparison.base.reason = ''; value.save();
  assert.throws(() => previousBaselines(value.options), /no-comparison reason/);
});
test('missing comparison directory fails before captures, even if current counters would pass', async t => {
  const value = comparisonFixture(t), root = temporary(t);
  let captures = 0;
  await assert.rejects(matrix({ ...value.options, root, out: path.join(temporary(t), 'matrix'), contract: '/unused',
    'source-receipt': '/unused', 'windows-tools': '/unused', 'previous-baselines': value.copies + '/missing' },
  { capture: () => { captures++; return []; } }), /previous baseline directory/);
  assert.equal(captures, 0);
});
for (const failure of [null, 'capture', 'capture-policy', 'check', 'policy', 'tests', 'clippy', 'remove']) test(`matrix completes all selections and propagates ${failure ?? 'no failure'}`, async t => {
  const root = temporary(t), out = path.join(temporary(t), 'matrix'), calls = [];
  let injected = false;
  const record = name => options => {
    calls.push([name, options]);
    if (failure === name && !injected) { injected = true; throw new Error('deliberate failure'); }
    if (failure === 'policy' && name === 'check' && !injected) { injected = true; return ['deliberate policy failure']; }
    if (failure === 'capture-policy' && name === 'capture' && !injected) { injected = true; return ['deliberate capture failure']; }
    return [];
  };
  const options = { root, out, contract: '/contract', 'source-sha': 'a'.repeat(40), 'source-receipt': '/source.json', 'windows-tools': '/llvm-mingw' };
  const issues = await matrix(options, Object.fromEntries(['capture', 'check', 'tests', 'clippy', 'remove'].map(name => [name, record(name)])));
  assert.equal(issues.length > 0, failure !== null);
  assert.equal(calls.filter(([name]) => name === 'capture').length, 16);
  assert.equal(calls.filter(([name]) => name === 'remove').length, 16);
  assert.equal(calls.filter(([name]) => name === 'capture').at(-1)[1].profile, 'combined');
  const results = JSON.parse(readFileSync(path.join(out, 'matrix-results.json')));
  assert.equal(results.length, 16);
  assert.equal(results.some(result => result.issues.length), failure !== null);
  if (!failure) {
    assert.equal(calls.filter(([name]) => name === 'tests').length, 4);
    assert.equal(calls.filter(([name]) => name === 'clippy').length, 4);
    for (let index = 0; index < calls.length; index++) if (calls[index][0] === 'remove') {
      assert.ok(['check', 'tests', 'clippy'].includes(calls[index - 1][0]));
      assert.equal(path.basename(calls[index][1]), 'target');
    }
  }
  for (const [name, args] of calls) if (name === 'check') assert.equal(args['source-sha'], options['source-sha']);
});
