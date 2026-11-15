import test from 'node:test';
import assert from 'node:assert/strict';
import { negatives, requireNegativeCases, requireNegativeDiagnostic, requireNegativeResults } from '../protocol-negative.mjs';
import { negativeCases } from '../protocol-source.mjs';

for (const mode of ['prepare', 'fetch', 'other']) test(`negative suite rejects the removed or unknown mode ${mode}`, async () => {
  await assert.rejects(negatives({ mode, repository: '.', out: '/unused', commit: 'a'.repeat(40), contract: '/unused' }), /negative mode must be check/);
});
test('a path or human message containing the expected words cannot supply a diagnostic code', () => {
  assert.throws(() => requireNegativeDiagnostic('ring', [{ code: 'policy', file: '/forbidden feature rustls/ring/contract.json',
    message: 'forbidden feature rustls/ring' }]), /intended rejection/);
  assert.throws(() => requireNegativeDiagnostic('ring', []), /intended rejection/);
  assert.throws(() => requireNegativeDiagnostic('ring', [{ code: 'forbidden-feature', package: 'reqwest', feature: 'ring' }]), /intended rejection/);
});
test('all five prescribed cases and completed results are mandatory', () => {
  requireNegativeCases(negativeCases);
  assert.throws(() => requireNegativeCases({}), /all five/);
  const results = [
    ['gzip', 'forbidden-feature', 'connectrpc', 'gzip'], ['zstd', 'forbidden-feature', 'connectrpc', 'zstd'],
    ['system-proxy', 'forbidden-feature', 'reqwest', 'system-proxy'], ['native-tls', 'forbidden-package', 'native-tls', null],
    ['ring', 'forbidden-feature', 'rustls', 'ring'],
  ].map(([name, code, pkg, feature]) => ({ name, result: 'rejected', reason: requireNegativeDiagnostic(name, [{ code, package: pkg, feature }]) }));
  requireNegativeResults(results);
  for (const values of [[], results.slice(1), [...results, results[0]], results.map(row => ({ ...row, result: 'incomplete' }))]) {
    assert.throws(() => requireNegativeResults(values), /all five/);
  }
});
