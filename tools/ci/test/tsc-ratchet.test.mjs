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
  const result = invoke('tsc-ratchet', args(log(t, 'error TS2304: Missing name.\n')), 1);
  assert.match(result.stderr, /lower the baseline for core from 2 to 1 in this change/);
  assert.equal(readFileSync(fixture('diagnostics.json'), 'utf8'), before);
});
test('tsc fails when the diagnostic count rises', t => {
  const source = text().replace('Found 2 errors in 2 files.', 'error TS2304: Missing name.\nFound 3 errors in 2 files.');
  assert.match(invoke('tsc-ratchet', args(log(t, source)), 1).stderr, /exceeds baseline 2/);
});
test('tsc accepts ANSI colours and zero established by exit status', t => {
  invoke('tsc-ratchet', args(log(t, '\x1b[91m' + text() + '\x1b[0m')), 0);
  const baseline = path.join(temporary(t), 'zero.json');
  writeFileSync(baseline, '{"version":1,"packages":{"core":0}}');
  invoke('tsc-ratchet', ['--baseline', baseline, ...args(log(t, ''), '0').slice(2)], 0);
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

for (const code of [5058, 5083, 6044, 6045, 6046, 6048, 6050, 6051, 6053, 6054, 6059, 6064, 6231, 6304, 6305, 6306, 6307, 6308, 6309, 6310, 18000, 18001, 18002, 18003]) test(`tsc: TS${code} config/input diagnostics fail even at an exact budget`, t => {
  const baseline = path.join(temporary(t), 'one.json');
  writeFileSync(baseline, '{"version":1,"packages":{"core":1}}');
  const input = log(t, `error TS${code}: Invalid configuration or input.\n`);
  assert.match(invoke('tsc-ratchet', ['--baseline', baseline, '--previous-baseline', baseline, ...args(input, '1').slice(2)], 1).stderr, /configuration\/input diagnostic/);
});

for (const code of [6133, 6138, 6192, 6196, 6198, 6205]) test(`tsc: TS${code} unused source diagnostics remain countable`, t => {
  const baseline = path.join(temporary(t), 'one.json');
  writeFileSync(baseline, '{"version":1,"packages":{"core":1}}');
  const input = log(t, `source.ts(1,1): error TS${code}: Declared but never used.\n`);
  invoke('tsc-ratchet', ['--baseline', baseline, ...args(input).slice(2)], 0);
});

for (const status of ['3', '127', '137', '143']) test(`tsc: incomplete exit status ${status} fails despite matching diagnostics`, () => {
  assert.match(invoke('tsc-ratchet', args(fixture('tsc.txt'), status), 1).stderr, /not a completed tsc\/vue-tsc run/);
});

test('tsc: completed exit statuses 1 and 2 are accepted with matching diagnostics', () => {
  for (const status of ['1', '2']) invoke('tsc-ratchet', args(fixture('tsc.txt'), status), 0);
});
