import test from 'node:test';
import assert from 'node:assert/strict';
import { testEvidence } from '../reference-run.mjs';

// Synthetic integration reports, not protocol vectors or checker expectations.
function report() {
  const witness = id => ({ id, executed: true, targetExecuted: true, path: ['provenance.ts/assertCaseConfirmation', 'node:assert/deepStrictEqual'], outcome: 'pass', actual: 1, expected: 1 });
  const baseline = { passed: true, witnesses: Array.from({ length: 12 }, (_, i) => witness('fixture-' + i)) };
  const mutants = [], byKind = {};
  for (const [kind, count] of Object.entries({ pae: 3, purpose: 3, claims: 3, expectation: 4, priority: 2, control: 4 })) {
    const control = kind === 'control';
    byKind[kind] = { total: count, detected: control ? 0 : count, ineffective: control ? count : 0, survived: 0, invalidExecution: 0 };
    for (let i = 0; i < count; i++) {
      const operation = kind + '-' + i, row = witness('fixture-' + mutants.length % 12);
      row.path.unshift('mutation/' + operation);
      const requiredTestFailure = control ? 'none:equivalent-or-normatively-ignored' : kind === 'expectation' ? 'expectation_snapshot_mismatch' : 'ERR_ASSERTION:actual-versus-fixed-expectation';
      if (!control) { row.outcome = kind === 'expectation' ? 'expectation-failure' : 'comparison-failure'; row.failure = requiredTestFailure; row.actual = 2; }
      mutants.push({ id: operation, kind, rule: 'integration-fixture', operation, witnesses: [row.id], violatesRule: !control, verdict: control ? 'ineffective' : 'detected', survived: control, requiredTestFailure, witnessResults: [row] });
    }
  }
  const checks = [{ id: 'provenance.independent-calculations', result: 'pass', evidence: { fields: 1, confirmed: 1, pending: 0, expectationsAccepted: true } }];
  return { format: 'counterexample-contract-v1', status: 'checks-pass', checks, executedMutations: 19, counts: { pass: 21, pending: 0, fail: 0 }, expectationsAccepted: true,
    counterexamples: { passed: true, baseline, mutants, byKind, counts: { mutants: 19, detected: 15, ineffective: 4, survived: 0, invalidExecution: 0 } } };
}
test('counterexample report admission requires executed witnesses and surviving harmless controls', () => {
  assert.equal(testEvidence(report(), 'counterexamples').expectationsAccepted, true);
});
for (const [name, mutate] of [
  ['failed baseline', r => { r.counterexamples.baseline.passed = false; }],
  ['missing baseline witness', r => { r.counterexamples.baseline.witnesses.pop(); }],
  ['unexecuted target', r => { r.counterexamples.mutants[0].witnessResults[0].targetExecuted = false; }],
  ['missing operation trace', r => { r.counterexamples.mutants[0].witnessResults[0].path.shift(); }],
  ['syntax failure counted as detection', r => { r.counterexamples.mutants[0].witnessResults[0].outcome = 'invalid-execution'; }],
  ['unchanged result counted as detection', r => { r.counterexamples.mutants[0].witnessResults[0].actual = 1; }],
  ['wrong error counted as detection', r => { r.counterexamples.mutants[0].witnessResults[0].failure = 'SyntaxError'; }],
  ['surviving harmful mutation', r => { r.counterexamples.mutants[0].verdict = 'survived'; }],
  ['ineffective mutation counted as detection', r => { r.counterexamples.mutants[0].violatesRule = false; }],
  ['harmless control fails', r => { r.counterexamples.mutants.at(-1).witnessResults[0].outcome = 'comparison-failure'; }],
  ['missing mutation', r => { r.counterexamples.mutants.shift(); }],
  ['duplicate mutation', r => { r.counterexamples.mutants[1].id = r.counterexamples.mutants[0].id; }],
  ['wrong category count', r => { r.counterexamples.byKind.pae.total = 2; }],
  ['wrong summary count', r => { r.counts.pass++; }],
  ['pending confirmations', r => { r.checks[0].evidence.pending = 1; }],
]) test(`counterexample report rejects ${name}`, () => {
  const r = report(); mutate(r);
  assert.throws(() => testEvidence(r, 'counterexamples'), /reference (?:counterexample|acceptance)/);
});

test('container report requires confirmed expectations and completed hash evidence', () => {
  const confirmation = report().checks[0], hash = { id: 'hash-fixture', result: 'pass' };
  const value = { format: 'container-gates-v1', results: [confirmation, hash], counts: { pass: 2, pending: 0 }, blake3: [hash], expectationsAccepted: true };
  assert.equal(testEvidence(value, 'container').pass, 2);
  for (const mutate of [
    r => { r.expectationsAccepted = false; },
    r => { r.blake3 = []; },
    r => { r.blake3[0].result = 'pending'; },
    r => { r.blake3 = [{ id: 'not-executed', result: 'pass' }]; },
    r => { r.results.push({ id: 'waiting', result: 'pending' }); r.counts.pending = 1; },
  ]) { const r = structuredClone(value); mutate(r); assert.throws(() => testEvidence(r, 'container'), /reference (?:container|execution)/); }
});
