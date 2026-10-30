import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fixture, invoke, temporary } from './helpers.mjs';

const args = file => ['--baseline', fixture('tests.json'), '--metadata', fixture('metadata.json'), '--input', file];
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

test('count-tests accepts custom test paths and multiple merged doctest blocks', t => {
  const text = source().replace('tests/integration.rs', 'custom_tests.rs') +
    '\nrunning 0 tests\ntest result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n';
  invoke('count-tests', args(log(t, text)), 0);
});
test('count-tests fails on missing package or empty log rather than assuming zero', t => {
  invoke('count-tests', ['--baseline', fixture('tests.json'), '--input', fixture('libtest.txt')], 2);
  invoke('count-tests', args(log(t, '')), 1);
  invoke('count-tests', [...args(fixture('libtest.txt')), '--package', 'absent'], 1);
});
test('count-tests sums multiple ordinary suites and permits additional passing tests', t => {
  const extra = '\n     Running tests/more.rs (target/debug/deps/more-0123456789abcdef)\nrunning 1 test\ntest extra ... ok\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n';
  const result = invoke('count-tests', args(log(t, source() + extra)), 0);
  assert.match(result.stdout, /3 successful tests/);
});
test('count-tests demands all baseline packages unless deliberately scoped', t => {
  const baseline = path.join(temporary(t), 'tests.json');
  writeFileSync(baseline, '{"version":1,"packages":{"core":{"tests":{"min":2},"docTests":{"min":1}},"other":{"tests":{"min":0},"docTests":{"min":0}}}}');
  const input = ['--baseline', baseline, ...args(fixture('libtest.txt')).slice(2)];
  assert.match(invoke('count-tests', input, 1).stderr, /other: package missing/);
  invoke('count-tests', [...input, '--package', 'core'], 0);
});

function baselineFile(t, packages) {
  const file = path.join(temporary(t), 'baseline.json');
  writeFileSync(file, JSON.stringify({ version: 1, packages }));
  return file;
}
const suite = (target, count) => `\n     Running unittests src/lib.rs (target/debug/deps/${target}-0123456789abcdef)\nrunning ${count} tests\ntest result: ok. ${count} passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n`;

test('count-tests: ordinary results cannot lend their header to another block', t => {
  const baseline = baselineFile(t, { core: { tests: { exact: 2 }, docTests: { exact: 0 } } });
  const extra = 'running 1 test\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n';
  const input = log(t, suite('core', 1) + extra);
  assert.match(invoke('count-tests', ['--baseline', baseline, '--metadata', fixture('metadata.json'), '--input', input], 2).stderr,
    /capture Cargo Running\/Doc-tests headers/);
});

test('count-tests: a workspace log attributes each suite by metadata target names', t => {
  const baseline = baselineFile(t, {
    core: { tests: { exact: 2 }, docTests: { exact: 1 } },
    bridge: { tests: { exact: 3 }, docTests: { min: 0 } },
  });
  const input = log(t, source() + suite('bridge_target', 3));
  const result = invoke('count-tests', ['--baseline', baseline, '--metadata', fixture('metadata.json'), '--input', input], 0);
  assert.match(result.stdout, /core: 2 successful tests/);
  assert.match(result.stdout, /bridge: 3 successful tests/);
});

test('count-tests: another package cannot cover missing droplet tests', t => {
  const metadata = JSON.parse(readFileSync(fixture('metadata.json'), 'utf8'));
  metadata.packages[1].name = 'droplet';
  metadata.packages[1].targets[0].name = 'droplet_runtime';
  const file = path.join(temporary(t), 'metadata.json');
  writeFileSync(file, JSON.stringify(metadata));
  const baseline = baselineFile(t, {
    core: { tests: { min: 2 }, docTests: { min: 1 } },
    droplet: { tests: { exact: 25 }, docTests: { exact: 0 } },
  });
  const result = invoke('count-tests', ['--baseline', baseline, '--metadata', file, '--input', log(t, source() + suite('droplet_runtime', 0))], 1);
  assert.match(result.stderr, /droplet: tests has 0 successful tests; expected exactly 25/);
  assert.match(invoke('count-tests', ['--baseline', baseline, '--metadata', file, '--input', fixture('libtest.txt')], 1).stderr, /droplet: package missing/);
});

test('count-tests: exact runtime-test baselines reject both zero and extra successes', t => {
  const baseline = baselineFile(t, { core: { tests: { exact: 1 }, docTests: { exact: 0 } } });
  for (const count of [0, 1, 2]) {
    const result = invoke('count-tests', ['--baseline', baseline, '--metadata', fixture('metadata.json'), '--input', log(t, suite('core', count))], count === 1 ? 0 : 1);
    if (count !== 1) assert.match(result.stderr, /expected exactly 1/);
  }
});

test('count-tests: zero exact ordinary counts still need an observed suite', t => {
  const baseline = baselineFile(t, { core: { tests: { exact: 0 }, docTests: { exact: 0 } } });
  const input = ['--baseline', baseline, '--metadata', fixture('metadata.json')];
  invoke('count-tests', [...input, '--input', log(t, suite('core', 0))], 0);
  invoke('count-tests', [...input, '--input', log(t, '')], 1);
});

test('count-tests: unknown, ambiguous and repeated target suites are input errors', t => {
  invoke('count-tests', args(log(t, source().replace('deps/core-', 'deps/unknown-'))), 2);
  invoke('count-tests', [...args(fixture('libtest.txt')), '--input', fixture('libtest.txt')], 2);
  const metadata = JSON.parse(readFileSync(fixture('metadata.json'), 'utf8'));
  metadata.packages[1].targets.push({ name: 'integration', kind: ['test'] });
  const file = path.join(temporary(t), 'metadata.json');
  writeFileSync(file, JSON.stringify(metadata));
  const input = ['--baseline', fixture('tests.json'), '--metadata', file, '--input', fixture('libtest.txt')];
  assert.match(invoke('count-tests', input, 2).stderr, /cannot uniquely attribute tests target integration/);
});

test('count-tests: repeated Doc-tests headers fail but merged doctest blocks pass', t => {
  const extra = '\nrunning 0 tests\ntest result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n';
  invoke('count-tests', args(log(t, source() + extra)), 0);
  assert.match(invoke('count-tests', args(log(t, source() + '\n   Doc-tests core\n' + extra)), 2).stderr, /repeated test suite/);
});

test('count-tests: exact and minimum schemas reject invalid baseline entries', t => {
  for (const tests of [2, { exact: 2, min: 1 }, { min: -1 }, { unknown: 1 }]) {
    const baseline = baselineFile(t, { core: { tests, docTests: { min: 0 } } });
    invoke('count-tests', ['--baseline', baseline, ...args(fixture('libtest.txt')).slice(2)], 2);
  }
});

test('count-tests: lib and bin suites with one target name keep distinct binary hashes', t => {
  const baseline = baselineFile(t, { core: { tests: { exact: 2 }, docTests: { exact: 0 } } });
  const first = suite('core', 1);
  const second = suite('core', 1).replace('0123456789abcdef', 'fedcba9876543210');
  invoke('count-tests', ['--baseline', baseline, '--metadata', fixture('metadata.json'), '--input', log(t, first + second)], 0);
});
