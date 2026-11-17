import test from 'node:test';
import assert from 'node:assert/strict';
import { fileURLToPath } from 'node:url';
import { yamlData } from '../pnpm-licences.mjs';
import { protocolSteps, protocolWorkflowIssues } from '../protocol-workflow.mjs';

const source = () => yamlData(fileURLToPath(new URL('../../../.gitea/workflows/ci.yml', import.meta.url)));
test('the real workflow satisfies all exact required execution blocks', () => {
  assert.equal(protocolSteps.length, 27);
  assert.deepEqual(protocolWorkflowIssues(source()), []);
});
const mutations = {
  removed: (steps, index) => steps.splice(index, 1),
  skipped: (steps, index) => { steps[index].if = false; },
  nonblocking: (steps, index) => { steps[index]['continue-on-error'] = true; },
  'suppressed failure': (steps, index) => { steps[index].run += ' || echo failed'; },
  'conditional wrapper': (steps, index) => { steps[index].run = 'if [ 0 -eq 1 ]; then\n' + steps[index].run + '\nfi'; },
  'colon instead of invocation': (steps, index) => { steps[index].run = ': ' + JSON.stringify(steps[index].run); },
  'different shell': (steps, index) => { steps[index].shell = 'bash --noprofile --norc -uo pipefail {0}'; },
  'different directory': (steps, index) => { steps[index]['working-directory'] = '/tmp'; },
  'different job': (steps, index, workflow, job) => { workflow.jobs[job === 'rust' ? 'checks' : 'rust'].steps.push(...steps.splice(index, 1)); },
  reordered: steps => steps.reverse(),
  'job shell override': (steps, index, workflow, job) => { workflow.jobs[job].defaults = { run: { shell: 'sh {0}' } }; },
};
for (const [job, name] of protocolSteps) for (const [label, mutate] of Object.entries(mutations)) {
  test(`required ${job}/${name} rejects ${label}`, () => {
    const workflow = source(), steps = workflow.jobs[job].steps, index = steps.findIndex(step => step.name === name);
    mutate(steps, index, workflow, job);
    const issues = protocolWorkflowIssues(workflow);
    const expected = ['removed', 'different job'].includes(label) ? `${job} needs exactly one required step: ${name}` :
      label === 'reordered' ? `${job} required step out of order:` : `${job} required execution block differs: ${name}`;
    assert.ok(issues.some(issue => issue.startsWith(expected)), expected);
  });
}
for (const job of ['checks', 'rust']) for (const field of ['if', 'continue-on-error', 'needs']) test(`${job} may not declare ${field}`, () => {
  const workflow = source(); workflow.jobs[job][field] = false;
  assert.ok(protocolWorkflowIssues(workflow).length);
});
for (const variable of ['NODE_OPTIONS', 'BASH_ENV', 'ENV', 'LD_PRELOAD', 'PATH']) {
  for (const scope of ['workflow', 'job', 'step']) test(`${scope} cannot inject ${variable}`, () => {
    const workflow = source();
    const owner = scope === 'workflow' ? workflow : scope === 'job' ? workflow.jobs.rust :
      workflow.jobs.checks.steps.find(step => step.name === 'Protocol TypeScript package (strict types, build and exports)');
    owner.env = { ...owner.env, [variable]: '--import=data:text/javascript,process.exit(0)' };
    assert.ok(protocolWorkflowIssues(workflow).length);
  });
}
for (const job of ['checks', 'rust']) for (const field of ['env', 'options', 'volumes']) test(`${job} container rejects ${field}`, () => {
  const workflow = source();
  workflow.jobs[job].container[field] = field === 'env' ? { NODE_OPTIONS: '--import=data:text/javascript,process.exit(0)' } : '--env NODE_OPTIONS=--import=data:text/javascript,process.exit(0)';
  assert.ok(protocolWorkflowIssues(workflow).length);
});
test('event variables must retain their exact bindings', () => {
  const workflow = source(), step = workflow.jobs.checks.steps.find(step => step.name === 'Protocol comparison base (actual event commit)');
  step.env.PR_BASE_SHA = '${{ github.sha }}';
  assert.ok(protocolWorkflowIssues(workflow).length);
});
for (const mutate of [
  workflow => { workflow.jobs.gate = { if: false, steps: [{ run: ':' }] }; workflow.jobs.checks.needs = 'gate'; },
  workflow => { workflow.jobs.gate = { if: false, steps: [{ run: ':' }] }; workflow.jobs.rust.needs = ['gate']; },
  workflow => { workflow.on.pull_request['paths-ignore'] = ['engine/**']; },
  workflow => { workflow.on.push.paths = ['docs/**']; },
  workflow => { workflow.on.push.branches = ['other']; },
  workflow => { workflow.on.push.tags = ['other*']; },
  workflow => { delete workflow.on.pull_request; },
]) test(`workflow execution boundary rejects ${mutate.toString()}`, () => {
  const workflow = source(); mutate(workflow);
  assert.ok(protocolWorkflowIssues(workflow).length);
});
test('an additional generation step cannot remove the mandatory source proof before drift', () => {
  const workflow = source(), steps = workflow.jobs.checks.steps;
  let index = steps.findIndex(step => step.name === 'Protocol drift (compare unchanged checked-in outputs)');
  steps.splice(index, 0, { name: 'Additional generation', run: 'cargo run --locked --offline -p impluvium-protocol-xtask -- generate' });
  assert.deepEqual(protocolWorkflowIssues(workflow), []);
  index = steps.findIndex(step => step.name === 'Protocol drift (compare unchanged checked-in outputs)');
  steps[index].run = 'cargo run --locked --offline -p impluvium-protocol-xtask -- check-drift';
  assert.ok(protocolWorkflowIssues(workflow).length);
});
test('line endings and surrounding whitespace are the only text normalizations', () => {
  const workflow = source();
  for (const [job, name] of protocolSteps) {
    const step = workflow.jobs[job].steps.find(step => step.name === name);
    step.run = '\n  ' + step.run.trim().replaceAll('\n', '\r\n') + '  \n';
  }
  assert.deepEqual(protocolWorkflowIssues(workflow), []);
});

for (const [label, before, after] of [
  ['dynamic profile check applied to tool baselines',
    '[[ "$name" == protocol-tests-core || "$name" == protocol-tests-server || "$name" == protocol-tests-client || "$name" == protocol-tests-combined ]]',
    '[[ "$name" == protocol-tests-* ]]'],
  ['hashchunk baseline omitted from base retrieval', 'protocol-tests-xtask-unit protocol-tests-hashchunk; do', 'protocol-tests-xtask-unit; do'],
]) test(`base retrieval rejects ${label}`, () => {
  const workflow = source(), step = workflow.jobs.rust.steps.find(step => step.name === 'Baselines of the base revision');
  assert.ok(step.run.includes(before));
  step.run = step.run.replace(before, after);
  assert.ok(protocolWorkflowIssues(workflow).length);
});
test('hashchunk Clippy cannot omit the protocol dependency from its ratchet selection', () => {
  const workflow = source(), step = workflow.jobs.rust.steps.find(step => step.name === 'Hash and chunk benchmark tool (tests and Clippy)');
  assert.ok(step.run.includes('--package impluvium-hashchunk-bench --package impluvium-protocol'));
  step.run = step.run.replace(' --package impluvium-protocol', '');
  assert.ok(protocolWorkflowIssues(workflow).length);
});

for (const mutation of [
  step => { delete step.if; },
  step => { step.if = 'success()'; },
  step => { step.with.path = step.with.path.replace('/work/results/reference/**', ''); },
  step => { step.with.path += '\n!/work/results/reference/**'; },
  step => { step['continue-on-error'] = true; },
]) test(`reference evidence rejects ${mutation.toString()}`, () => {
  const workflow = source();
  mutation(workflow.jobs.checks.steps.find(step => step.name === 'Protocol schema and codegen evidence'));
  assert.ok(protocolWorkflowIssues(workflow).length);
});
for (const name of ['Protocol TypeScript dependencies (root lock and directory filter)', 'Licence boundary (Rust and installed TypeScript graph)']) {
  for (const value of ['test -f engine/protocol/ts/package.json', 'test -f engine/protocol/conformance/reference/package.json',
    '--filter "{./engine/protocol/ts}..."', '--filter "{./engine/protocol/conformance/reference}..."', '--fail-if-no-match']) {
    test(`${name} requires ${value}`, () => {
      const workflow = source(), step = workflow.jobs.checks.steps.find(step => step.name === name);
      assert.ok(step.run.includes(value));
      step.run = step.run.replace(value, '');
      assert.ok(protocolWorkflowIssues(workflow).length);
    });
  }
}

for (const [stepName, text] of [
  ['Pins (images, actions, tools, locked Cargo commands, frozen installs, Node major, required jobs)', 'node tools/ci/reference-products.mjs --root .'],
  ['Reference types and imports', '--out /work/results/reference/imports'],
  ['Reference conformance', 'node tools/ci/reference-run.mjs container'],
  ['Reference conformance', '--out /work/results/reference/vectors'],
  ['Reference counterexamples', '--out /work/results/reference/counterexamples'],
]) test(`reference invocation retains ${text}`, () => {
  const workflow = source(), step = workflow.jobs.checks.steps.find(step => step.name === stepName);
  assert.ok(step.run.includes(text));
  step.run = step.run.replace(text, '');
  assert.ok(protocolWorkflowIssues(workflow).length);
});
