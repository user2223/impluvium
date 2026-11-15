import path from 'node:path';
import { existsSync, lstatSync, mkdirSync, readFileSync, readdirSync, realpathSync, rmSync, writeFileSync } from 'node:fs';
import { cli, json, required, stringOption } from './common.mjs';
import { externalOutput, fail, hash, matrixSelections } from './protocol-contract.mjs';
import { capture } from './capture-protocol-graph.mjs';
import { check } from './check-protocol-graph.mjs';
import { check as tests } from './count-tests.mjs';
import { check as clippy } from './clippy-ratchet.mjs';
import { gitObject, verifyReceipt } from './protocol-source.mjs';

export { matrixSelections } from './protocol-contract.mjs';
export function previousBaselines(options) {
  if (!options['previous-baselines']) {
    if (options['previous-comparison'] || options['previous-source-receipt']) fail('comparison evidence needs a previous-baselines directory');
    return () => ({});
  }
  required(options, 'previous-comparison', 'previous-source-receipt');
  const directory = path.resolve(options['previous-baselines']);
  let entries;
  try {
    if (!lstatSync(directory).isDirectory()) fail('not a directory');
    entries = new Set(readdirSync(directory));
  } catch { fail('previous baseline directory is missing or unreadable'); }
  const comparison = json(options['previous-comparison']), base = comparison.base;
  if (comparison.version !== 1 || !/^[a-f0-9]{40}$/.test(comparison.currentSha ?? '') || comparison.currentSha !== options['source-sha'] ||
      !['pull_request', 'push', 'workflow_dispatch'].includes(comparison.event) || !['available', 'unavailable'].includes(base?.status)) fail('previous comparison does not match the current source');
  let source;
  if (base.status === 'available') {
    source = verifyReceipt(json(options['previous-source-receipt']), base.sha);
    if (source.mutation) fail('previous baselines need an unmodified base receipt');
  } else if (base.sha !== null || typeof base.reason !== 'string' || !base.reason.trim()) fail('missing explicit no-comparison reason');
  const previous = new Map();
  const names = ['clippy-baseline.json', ...matrixSelections.filter(row => row.run === 'test').map(row => `protocol-tests-${row.profile}.json`)];
  for (const name of names) {
    const item = source?.files.find(file => file.path === `tools/ci/${name}`), file = path.join(directory, name);
    if (!item) {
      if (entries.has(name)) fail(`previous baseline is not in the attested base: ${name}`);
      previous.set(name, {}); continue;
    }
    if (!entries.has(name) || !lstatSync(file).isFile()) fail(`previous baseline was not copied: ${name}`);
    const bytes = readFileSync(file);
    if (hash(bytes) !== item.sha256 || gitObject('blob', bytes) !== item.blob) fail(`previous baseline differs from the attested base: ${name}`);
    previous.set(name, { 'previous-baseline': file });
  }
  return name => previous.get(name) ?? fail(`unknown previous baseline: ${name}`);
}
export async function matrix(options, ops = { capture, check, tests, clippy, remove: rmSync }) {
  required(options, 'root', 'contract', 'source-sha', 'source-receipt', 'windows-tools', 'out');
  const root = realpathSync(options.root), out = externalOutput(root, options.out), results = [], issues = [];
  if (existsSync(out)) fail('matrix results directory must be fresh');
  const previous = previousBaselines(options);
  mkdirSync(out, { recursive: true });
  const baseline = name => ({ baseline: path.join(root, 'tools/ci', name), ...previous(name) });
  for (const selection of matrixSelections) {
    const folder = path.join(out, selection.profile, selection.target, selection.run), failures = [];
    try {
      failures.push(...await ops.capture({ ...options, root, ...selection, out: folder }));
      if (!failures.length) failures.push(...ops.check({ root, contract: options.contract, input: folder, 'source-sha': options['source-sha'] }));
      if (!failures.length && selection.run === 'test') failures.push(...ops.tests({ ...baseline(`protocol-tests-${selection.profile}.json`),
        metadata: path.join(folder, 'metadata.json'), input: [path.join(folder, 'tests.txt')] }));
      if (!failures.length && selection.run === 'clippy') failures.push(...ops.clippy({ ...baseline('clippy-baseline.json'),
        metadata: path.join(folder, 'metadata.json'), input: path.join(folder, 'cargo.jsonl'),
        package: ['impluvium-protocol', ...(['client', 'combined'].includes(selection.profile) ? ['impluvium-client'] : [])] }));
    } catch (error) { failures.push(`${folder}:1: ${error.message}`); }
    finally {
      // Keep raw evidence for every outcome; only this selection's build tree
      // is disposable. A cleanup error must not skip subsequent selections.
      try { ops.remove(path.join(folder, 'target'), { recursive: true, force: true }); }
      catch (error) { failures.push(`${folder}:1: ${error.message}`); }
    }
    issues.push(...failures); results.push({ ...selection, issues: failures });
  }
  writeFileSync(path.join(out, 'matrix-results.json'), JSON.stringify(results, null, 2) + '\n', { flag: 'wx' });
  return issues;
}
await cli(import.meta.url, `Usage: node tools/ci/protocol-matrix.mjs --root DIR --contract FILE --source-sha SHA --source-receipt FILE --windows-tools DIR --out DIR [--previous-baselines DIR --previous-comparison FILE --previous-source-receipt FILE]
Capture and check all 16 selections, apply separate test/Clippy ratchets, then remove each target tree.
Comparison baselines must match the actual base commit's attested catalogue; only proven absence permits a first run.
Retain raw evidence and continue after a failed selection; any failure makes the whole matrix fail.
Exit: 0 all passed, 1 selection failures, 2 invalid invocation or output directory.`, {
  root: stringOption, contract: stringOption, 'source-sha': stringOption, 'source-receipt': stringOption,
  'windows-tools': stringOption, out: stringOption, 'previous-baselines': stringOption,
  'previous-comparison': stringOption, 'previous-source-receipt': stringOption,
}, matrix);
