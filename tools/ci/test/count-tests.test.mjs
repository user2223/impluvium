import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fixture, invoke, temporary } from './helpers.mjs';

const args = file => ['--baseline', fixture('tests.json'), '--input', `core=${file}`];
const source = () => readFileSync(fixture('libtest.txt'), 'utf8');
function log(t, text) {
  const file = path.join(temporary(t), 'libtest.txt');
  writeFileSync(file, text);
  return file;
}
test('count-tests excludes ignored/filtered tests and separates doc-tests', () => {
  const result = invoke('count-tests', args(fixture('libtest.txt')), 0);
  assert.match(result.stdout, /2 successful tests, 1 successful doc-tests/);
});
test('count-tests fails if ordinary successes fall, even when doc-tests grow', t => {
  const text = source().replace('test tests::second ... ok', 'test tests::second ... ignored')
    .replace('running 1 test\n', 'running 2 tests\ntest engine/core/src/lib.rs - extra_example (line 2) ... ok\n')
    .replace('ok. 1 passed;', 'ok. 2 passed;')
    .replace('2 passed; 0 failed; 1 ignored', '1 passed; 0 failed; 2 ignored');
  assert.match(invoke('count-tests', args(log(t, text)), 1).stderr, /expected at least 2/);
});
test('count-tests fails if doc-tests vanish or a suite fails', t => {
  invoke('count-tests', args(log(t, source().split('   Doc-tests')[0])), 1);
  const text = source().replace('ok. 2 passed; 0 failed', 'FAILED. 1 passed; 1 failed');
  assert.match(invoke('count-tests', args(log(t, text)), 1).stderr, /test suite failed/);
});
test('count-tests rejects truncated output and inconsistent libtest totals', t => {
  invoke('count-tests', args(log(t, source().replace(/test result: ok\. 1 passed;[^\n]+/, ''))), 2);
  invoke('count-tests', args(log(t, source().replace('running 3 tests', 'running 4 tests'))), 2);
  invoke('count-tests', args(log(t, source() + '\n     Running custom.rs (target/debug/deps/custom-0123456789abcdef)\n')), 2);
});

test('count-tests accepts custom test paths and multiple doctest suites', t => {
  const text = source().replace('tests/integration.rs', 'custom_tests.rs') +
    '\nrunning 0 tests\ntest result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n';
  invoke('count-tests', args(log(t, text)), 0);
});
test('count-tests fails on missing package or empty log rather than assuming zero', t => {
  invoke('count-tests', ['--baseline', fixture('tests.json'), '--input', 'unrelated=' + fixture('libtest.txt')], 2);
  invoke('count-tests', args(log(t, '')), 1);
  invoke('count-tests', [...args(fixture('libtest.txt')), '--package', 'absent'], 2);
});
test('count-tests sums multiple ordinary suites and permits additional passing tests', t => {
  const extra = '\n     Running tests/more.rs (target/debug/deps/more-0123456789abcdef)\nrunning 1 test\ntest extra ... ok\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n';
  const result = invoke('count-tests', args(log(t, source() + extra)), 0);
  assert.match(result.stdout, /3 successful tests/);
});
test('count-tests demands all baseline packages unless deliberately scoped', t => {
  const baseline = path.join(temporary(t), 'tests.json');
  writeFileSync(baseline, '{"version":1,"packages":{"core":{"tests":2,"docTests":1},"other":{"tests":0,"docTests":0}}}');
  const input = [...args(fixture('libtest.txt')), '--baseline', baseline];
  assert.match(invoke('count-tests', input, 1).stderr, /other: package missing/);
  invoke('count-tests', [...input, '--package', 'core'], 0);
});
