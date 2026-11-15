import path from 'node:path';
import { mkdirSync, writeFileSync, rmSync } from 'node:fs';
import { cli, required, stringOption } from './common.mjs';
import { externalOutput, fail, linux } from './protocol-contract.mjs';
import { exportSource } from './export-protocol-source.mjs';
import { capture } from './capture-protocol-graph.mjs';
import { checkDiagnostics } from './check-protocol-graph.mjs';
import { negativeCases } from './protocol-source.mjs';

const expected = {
  gzip: ['forbidden-feature', 'connectrpc', 'gzip'], zstd: ['forbidden-feature', 'connectrpc', 'zstd'],
  'system-proxy': ['forbidden-feature', 'reqwest', 'system-proxy'],
  'native-tls': ['forbidden-package', 'native-tls', null], ring: ['forbidden-feature', 'rustls', 'ring'],
};
export function requireNegativeCases(cases) {
  if (JSON.stringify(Object.keys(cases).sort()) !== JSON.stringify(Object.keys(expected).sort())) fail('all five prescribed negative cases are required');
}
export function requireNegativeDiagnostic(name, diagnostics) {
  const wanted = expected[name];
  if (!wanted || !diagnostics.some(issue => issue.code === wanted[0] && issue.package === wanted[1] && (issue.feature ?? null) === wanted[2])) fail(`negative case did not reach its intended rejection: ${name}`);
  return { code: wanted[0], package: wanted[1], feature: wanted[2] };
}
export function requireNegativeResults(results) {
  requireNegativeCases(Object.fromEntries(results.map(result => [result.name, true])));
  if (results.length !== 5 || results.some(result => result.result !== 'rejected')) fail('all five negative cases must complete and be rejected');
  for (const result of results) requireNegativeDiagnostic(result.name, [result.reason]);
}

export async function negatives(options) {
  required(options, 'mode', 'repository', 'out', 'commit', 'contract');
  if (options.mode !== 'check') fail('negative mode must be check');
  requireNegativeCases(negativeCases);
  const out = externalOutput(options.repository, options.out); mkdirSync(out);
  const results = [];
  for (const name of Object.keys(expected)) {
    const folder = path.join(out, name), root = path.join(folder, 'source'); mkdirSync(folder);
    const receipt = path.join(folder, 'source.json'), evidence = path.join(folder, 'evidence');
    exportSource({ repository: options.repository, commit: options.commit, out: root, receipt, 'negative-case': name, 'commit-object': options['commit-object'] });
    const profile = ['gzip', 'zstd'].includes(name) ? 'server' : 'client';
    await capture({ root, contract: options.contract, profile, target: linux, run: 'build',
      'source-sha': options.commit, 'source-receipt': receipt, 'negative-case': name, out: evidence });
    const issues = checkDiagnostics({ root, contract: options.contract, input: evidence, 'negative-case': name, 'source-sha': options.commit });
    writeFileSync(path.join(folder, 'rejection.json'), JSON.stringify(issues, null, 2) + '\n');
    results.push({ name, result: 'rejected', reason: requireNegativeDiagnostic(name, issues) });
    rmSync(path.join(evidence, 'target'), { recursive: true });
  }
  requireNegativeResults(results);
  writeFileSync(path.join(out, 'results.json'), JSON.stringify(results, null, 2) + '\n');
  return [];
}
await cli(import.meta.url, 'Usage: node tools/ci/protocol-negative.mjs --mode check --repository DIR --commit SHA --contract FILE --out DIR [--commit-object FILE]\nDerive package-preserving locks offline and run all five complete resolution/build/check counterexamples.', {
  mode: stringOption, repository: stringOption, out: stringOption, commit: stringOption, contract: stringOption, 'commit-object': stringOption,
}, negatives);
