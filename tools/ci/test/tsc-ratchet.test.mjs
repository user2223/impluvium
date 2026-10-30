import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fixture, invoke, temporary } from './helpers.mjs';

const args = (input, status = '2') => ['--baseline', fixture('diagnostics.json'), '--package', 'core', '--input', input, '--status', status];
const text = () => readFileSync(fixture('tsc.txt'), 'utf8');
const log = (t, text) => {
  const file = path.join(temporary(t), 'tsc.txt');
  writeFileSync(file, text);
  return file;
};

test('tsc accepts tsc and vue-tsc diagnostic formats with a matching summary', () => {
  invoke('tsc-ratchet', args(fixture('tsc.txt')), 0);
});
test('tsc reports how to lower a baseline and never rewrites it', t => {
  const before = readFileSync(fixture('diagnostics.json'), 'utf8');
  const result = invoke('tsc-ratchet', args(log(t, 'error TS5058: The specified path does not exist.\n')), 0);
  assert.match(result.stdout, /lower this baseline from 2 to 1/);
  assert.equal(readFileSync(fixture('diagnostics.json'), 'utf8'), before);
});
test('tsc fails when the diagnostic count rises', t => {
  const source = text().replace('Found 2 errors in 2 files.', 'error TS2304: Missing name.\nFound 3 errors in 2 files.');
  assert.match(invoke('tsc-ratchet', args(log(t, source)), 1).stderr, /exceeds baseline 2/);
});
test('tsc accepts ANSI colours and zero established by exit status', t => {
  invoke('tsc-ratchet', args(log(t, '\x1b[91m' + text() + '\x1b[0m')), 0);
  invoke('tsc-ratchet', args(log(t, ''), '0'), 0);
});
test('tsc rejects failures without diagnostics', t => {
  assert.match(invoke('tsc-ratchet', args(log(t, 'Command not found: vue-tsc\n'), '127'), 1).stderr, /without TypeScript diagnostics/);
});
test('tsc rejects contradictory status, mismatched summaries and invalid status', t => {
  invoke('tsc-ratchet', args(fixture('tsc.txt'), '0'), 2);
  invoke('tsc-ratchet', args(log(t, text().replace('Found 2', 'Found 3'))), 2);
  invoke('tsc-ratchet', args(fixture('tsc.txt'), 'missing'), 2);
});
test('tsc requires baseline membership and an explicit input status', () => {
  invoke('tsc-ratchet', [...args(fixture('tsc.txt')), '--package', 'missing'], 1);
  invoke('tsc-ratchet', args(fixture('tsc.txt')).slice(0, -2), 2);
});
