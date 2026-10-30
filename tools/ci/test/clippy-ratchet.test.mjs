import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fixture, invoke, temporary } from './helpers.mjs';

const original = () => readFileSync(fixture('clippy.jsonl'), 'utf8').trim().split('\n').map(JSON.parse);
const args = input => ['--baseline', fixture('diagnostics.json'), '--metadata', fixture('metadata.json'), '--input', input];
function changed(t, transform) {
  const file = path.join(temporary(t), 'cargo.jsonl');
  writeFileSync(file, transform(original()).map(item => JSON.stringify(item)).join('\n') + '\n');
  return file;
}

test('clippy counts only Clippy diagnostics and requires cached zero artifacts', () => {
  const result = invoke('clippy-ratchet', args(fixture('clippy.jsonl')), 0);
  assert.match(result.stdout, /core: 1 Clippy diagnostics/);
  assert.match(result.stdout, /bridge: 0 Clippy diagnostics/);
  assert.match(result.stdout, /lower this baseline from 2 to 1/);
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
  invoke('clippy-ratchet', [...args(fixture('clippy.jsonl')), '--baseline', file], 2);
});
