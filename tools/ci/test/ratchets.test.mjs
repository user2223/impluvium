import test from 'node:test';
import assert from 'node:assert/strict';
import { writeFileSync } from 'node:fs';
import path from 'node:path';
import { fixture, invoke, temporary } from './helpers.mjs';

const configurations = [
  ['clippy-ratchet', { core: 1, bridge: 0 }, ['--metadata', fixture('metadata.json'), '--input', fixture('clippy.jsonl')]],
  ['tsc-ratchet', { core: 2, bridge: 0 }, ['--package', 'core', '--input', fixture('tsc.txt'), '--status', '2']],
  ['count-tests', { core: { tests: { min: 2 }, docTests: { min: 1 } } }, ['--metadata', fixture('metadata.json'), '--input', fixture('libtest.txt')]],
];
function baselines(t, current, previous) {
  const root = temporary(t);
  const args = [];
  for (const [option, packages] of [['baseline', current], ['previous-baseline', previous]]) {
    const file = path.join(root, option + '.json');
    writeFileSync(file, JSON.stringify({ version: 1, packages }));
    args.push('--' + option, file);
  }
  return args;
}

for (const [tool, packages, input] of configurations) {
  test(`${tool}: unchanged base-revision baseline passes`, t => {
    invoke(tool, [...baselines(t, packages, packages), ...input], 0);
  });
  test(`${tool}: previous-baseline permits new test entries or rejects nonzero diagnostic budgets`, t => {
    const previous = { ...packages };
    delete previous.core;
    if (!Object.keys(previous).length) previous.existing = packages.core;
    const result = invoke(tool, [...baselines(t, packages, previous), ...input], tool === 'count-tests' ? 0 : 1);
    if (tool !== 'count-tests') assert.match(result.stderr, /core: added package in baseline/);
  });
  test(`${tool}: previous-baseline rejects relaxed limits`, t => {
    const previous = structuredClone(packages);
    if (tool === 'count-tests') previous.core.tests.min = 3;
    else previous.core--;
    assert.match(invoke(tool, [...baselines(t, packages, previous), ...input], 1).stderr, /core: baseline was relaxed/);
  });
  test(`${tool}: previous-baseline permits tighter limits and package removal`, t => {
    const previous = structuredClone(packages);
    previous.removed = packages.core;
    if (tool === 'count-tests') previous.core.tests.min = 1;
    else previous.core++;
    invoke(tool, [...baselines(t, packages, previous), ...input], 0);
  });
}

test('count-tests: previous-baseline prevents weakening exact counts to minimums', t => {
  const current = { core: { tests: { min: 2 }, docTests: { min: 1 } } };
  const previous = { core: { tests: { exact: 2 }, docTests: { min: 1 } } };
  assert.match(invoke('count-tests', [...baselines(t, current, previous), '--metadata', fixture('metadata.json'), '--input', fixture('libtest.txt')], 1).stderr,
    /baseline was relaxed/);
});

test('count-tests: previous-baseline rejects a lower exact test count', t => {
  const current = { core: { tests: { exact: 2 }, docTests: { exact: 1 } } };
  const previous = { core: { tests: { exact: 25 }, docTests: { exact: 1 } } };
  assert.match(invoke('count-tests', [...baselines(t, current, previous), '--metadata', fixture('metadata.json'), '--input', fixture('libtest.txt')], 1).stderr,
    /baseline was relaxed/);
});

for (const tool of ['clippy-ratchet', 'tsc-ratchet', 'count-tests']) {
  test(`${tool}: previous-baseline checks unselected packages for additions and relaxation`, t => {
    const testLimit = { tests: { min: 0 }, docTests: { min: 0 } };
    const current = tool === 'count-tests'
      ? { core: { tests: { min: 2 }, docTests: { min: 1 } }, other: testLimit }
      : { core: tool === 'clippy-ratchet' ? 1 : 2, other: 1 };
    const input = tool === 'clippy-ratchet'
      ? ['--metadata', fixture('metadata.json'), '--input', fixture('clippy.jsonl'), '--package', 'core', '--exclude', 'bridge']
      : tool === 'tsc-ratchet'
        ? ['--package', 'core', '--input', fixture('tsc.txt'), '--status', '2']
        : ['--metadata', fixture('metadata.json'), '--input', fixture('libtest.txt'), '--package', 'core'];
    for (const added of [true, false]) {
      const previous = structuredClone(current);
      if (added) delete previous.other;
      else if (tool === 'count-tests') previous.other.tests.min = 1;
      else previous.other = 0;
      const allowed = added && tool === 'count-tests';
      const result = invoke(tool, [...baselines(t, current, previous), ...input], allowed ? 0 : 1);
      if (!allowed) assert.match(result.stderr, added ? /other: added package/ : /other: baseline was relaxed/);
    }
  });
}

for (const [tool, packages, input] of configurations.filter(([tool]) => tool !== 'count-tests')) {
  test(`${tool}: previous-baseline allows added zero diagnostic budgets`, t => {
    const previous = { ...packages };
    delete previous.bridge;
    invoke(tool, [...baselines(t, packages, previous), ...input], 0);
  });
  test(`${tool}: an empty previous baseline permits only zero diagnostic additions`, t => {
    const zeroInput = tool === 'clippy-ratchet'
      ? ['--metadata', fixture('metadata.json'), '--input', fixture('clippy.jsonl'), '--package', 'bridge', '--exclude', 'core']
      : ['--package', 'bridge', '--input', path.join(temporary(t), 'empty.txt'), '--status', '0'];
    if (tool === 'tsc-ratchet') writeFileSync(zeroInput[3], '');
    invoke(tool, [...baselines(t, { bridge: 0 }, {}), ...zeroInput], 0);
  });
}

test('count-tests: previous-baseline allows added exact and minimum test entries', t => {
  for (const constraint of ['exact', 'min']) {
    const current = { core: { tests: { [constraint]: 2 }, docTests: { [constraint]: 1 } } };
    const previous = { existing: { tests: { min: 0 }, docTests: { min: 0 } } };
    invoke('count-tests', [...baselines(t, current, previous), '--metadata', fixture('metadata.json'), '--input', fixture('libtest.txt')], 0);
  }
});
