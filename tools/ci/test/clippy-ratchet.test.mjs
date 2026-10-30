import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fixture, invoke, temporary } from './helpers.mjs';

const original = () => readFileSync(fixture('clippy.jsonl'), 'utf8').trim().split('\n').map(JSON.parse);
const args = input => ['--baseline', fixture('clippy-baseline.json'), '--metadata', fixture('metadata.json'), '--input', input];
function changed(t, transform) {
  const file = path.join(temporary(t), 'cargo.jsonl');
  writeFileSync(file, transform(original()).map(item => JSON.stringify(item)).join('\n') + '\n');
  return file;
}

test('clippy counts only Clippy diagnostics and requires cached zero artifacts', () => {
  const result = invoke('clippy-ratchet', args(fixture('clippy.jsonl')), 0);
  assert.match(result.stdout, /core: 1 Clippy diagnostics/);
  assert.match(result.stdout, /bridge: 0 Clippy diagnostics/);
});

test('clippy rejects an increase per package rather than a combined total', t => {
  const file = changed(t, items => {
    items.splice(4, 0, { ...items[3], package_id: items[5].package_id });
    return items;
  });
  assert.match(invoke('clippy-ratchet', args(file), 1).stderr, /bridge: 1 diagnostics exceeds baseline 0/);
});

test('clippy rejects a missing package even when it had zero diagnostics', t => {
  const file = changed(t, items => items.filter(item => !item.package_id?.startsWith('bridge ')));
  assert.match(invoke('clippy-ratchet', args(file), 1).stderr, /bridge: count missing/);
  invoke('clippy-ratchet', [...args(file), '--package', 'core'], 0);
});

test('clippy cannot establish zero from diagnostics without an artifact', t => {
  const file = changed(t, items => items.filter(item => item.reason !== 'compiler-artifact'));
  invoke('clippy-ratchet', args(file), 1);
});

test('clippy preserves compiler/build failures independently of counts', t => {
  const file = changed(t, items => {
    items[0].message.level = 'error';
    items.at(-1).success = false;
    return items;
  });
  const result = invoke('clippy-ratchet', args(file), 1);
  assert.match(result.stderr, /compiler error/);
  assert.match(result.stderr, /did not finish successfully/);
});

test('clippy rejects truncated, non-JSON and unknown package inputs', t => {
  const truncated = changed(t, items => items.slice(0, -1));
  invoke('clippy-ratchet', args(truncated), 2);
  const unknown = changed(t, items => { items[0].package_id = 'unknown'; return items; });
  invoke('clippy-ratchet', args(unknown), 2);
  writeFileSync(truncated, 'Compiling core v0.1.0\n');
  invoke('clippy-ratchet', args(truncated), 2);
});

test('clippy diagnoses missing baseline packages and invalid baselines', t => {
  invoke('clippy-ratchet', [...args(fixture('clippy.jsonl')), '--package', 'absent'], 1);
  const file = path.join(temporary(t), 'baseline.json');
  writeFileSync(file, '{"version":1,"packages":{"core":null}}');
  invoke('clippy-ratchet', ['--baseline', file, ...args(fixture('clippy.jsonl')).slice(2)], 2);
});

test('clippy: improvements require lowering the baseline in this change', t => {
  const file = changed(t, items => items.filter(item => item.message?.code?.code !== 'clippy::needless_return'));
  assert.match(invoke('clippy-ratchet', args(file), 1).stderr, /lower the baseline for core from 1 to 0 in this change/);
});

test('clippy: every observed workspace member needs a baseline or explicit exclusion', t => {
  const baseline = path.join(temporary(t), 'baseline.json');
  writeFileSync(baseline, '{"version":1,"packages":{"bridge":0}}');
  const input = ['--baseline', baseline, ...args(fixture('clippy.jsonl')).slice(2)];
  assert.match(invoke('clippy-ratchet', input, 1).stderr, /core: observed workspace package missing from baseline/);
  invoke('clippy-ratchet', [...input, '--exclude', 'core'], 0);
  const diagnosticsOnly = changed(t, items => items.filter(item => !(item.reason === 'compiler-artifact' && item.package_id.startsWith('path+'))));
  assert.match(invoke('clippy-ratchet', ['--baseline', baseline, '--metadata', fixture('metadata.json'), '--input', diagnosticsOnly], 1).stderr,
    /core: observed workspace package missing from baseline/);
});

test('clippy: package scoping reports observed members outside the selection', () => {
  const input = [...args(fixture('clippy.jsonl')), '--package', 'core'];
  assert.match(invoke('clippy-ratchet', input, 1).stderr, /bridge: observed workspace package outside --package selection/);
  invoke('clippy-ratchet', [...input, '--exclude', 'bridge'], 0);
});

test('clippy: registry artifacts do not require workspace baseline entries', t => {
  const metadata = JSON.parse(readFileSync(fixture('metadata.json'), 'utf8'));
  metadata.packages.push({ id: 'registry-helper', name: 'helper' });
  const file = path.join(temporary(t), 'metadata.json');
  writeFileSync(file, JSON.stringify(metadata));
  const input = changed(t, items => {
    items.splice(-1, 0, { reason: 'compiler-artifact', package_id: 'registry-helper' });
    return items;
  });
  invoke('clippy-ratchet', ['--baseline', fixture('clippy-baseline.json'), '--metadata', file, '--input', input], 0);
});

test('clippy: a registry namesake cannot supply a missing workspace artifact', t => {
  const root = temporary(t);
  const metadata = JSON.parse(readFileSync(fixture('metadata.json'), 'utf8'));
  metadata.packages.push({ id: 'registry-core', name: 'core' });
  const file = path.join(root, 'metadata.json');
  const baseline = path.join(root, 'baseline.json');
  writeFileSync(file, JSON.stringify(metadata));
  writeFileSync(baseline, '{"version":1,"packages":{"core":0}}');
  const input = changed(t, () => [
    { reason: 'compiler-artifact', package_id: 'registry-core' },
    { reason: 'build-finished', success: true },
  ]);
  assert.match(invoke('clippy-ratchet', ['--baseline', baseline, '--metadata', file, '--input', input], 1).stderr,
    /core: count missing from input/);
});

test('clippy: registry namesake diagnostics cannot change a workspace count', t => {
  const metadata = JSON.parse(readFileSync(fixture('metadata.json'), 'utf8'));
  metadata.packages.push({ id: 'registry-core', name: 'core' });
  const file = path.join(temporary(t), 'metadata.json');
  writeFileSync(file, JSON.stringify(metadata));
  const input = changed(t, items => {
    items.splice(-1, 0, { reason: 'compiler-artifact', package_id: 'registry-core' },
      { reason: 'compiler-message', package_id: 'registry-core', message: { level: 'warning', code: { code: 'clippy::other' } } });
    return items;
  });
  invoke('clippy-ratchet', ['--baseline', fixture('clippy-baseline.json'), '--metadata', file, '--input', input], 0);
});

for (const record of [
  { reason: 'compiler-artifact', package_id: 'core' },
  { reason: 'compiler-message', package_id: 'core', message: { level: 'warning', code: { code: 'clippy::example' } } },
  { reason: 'build-script-executed', package_id: 'core' },
  { reason: 'build-finished', success: true },
  { reason: 'future-record' },
]) test(`clippy: every record after build-finished is an input error: ${record.reason}`, t => {
  const file = changed(t, items => [...items, record]);
  assert.match(invoke('clippy-ratchet', args(file), 2).stderr, /record after terminal build-finished/);
});

test('clippy: an earlier completion cannot validate a truncated appended build', t => {
  const root = temporary(t);
  const metadata = path.join(root, 'metadata.json');
  const baseline = path.join(root, 'baseline.json');
  writeFileSync(metadata, JSON.stringify({ packages: [{ id: 'core', name: 'core' }], workspace_members: ['core'] }));
  writeFileSync(baseline, '{"version":1,"packages":{"core":0}}');
  for (const prefix of [[], [{ reason: 'compiler-artifact', package_id: 'core' }]]) {
    const file = changed(t, () => [...prefix, { reason: 'build-finished', success: true }, { reason: 'compiler-artifact', package_id: 'core' }]);
    assert.match(invoke('clippy-ratchet', ['--baseline', baseline, '--metadata', metadata, '--input', file], 2).stderr, /record after terminal build-finished/);
  }
});

test('clippy: exactly one completion is required and trailing blank lines are allowed', t => {
  const missing = changed(t, items => items.filter(item => item.reason !== 'build-finished'));
  invoke('clippy-ratchet', args(missing), 2);
  const complete = changed(t, items => items);
  writeFileSync(complete, readFileSync(complete, 'utf8') + '\n\n');
  invoke('clippy-ratchet', args(complete), 0);
});
