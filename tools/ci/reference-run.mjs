import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';
import { parseArgs } from 'node:util';
import { isDeepStrictEqual } from 'node:util';
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { json } from './common.mjs';
import { verifySource } from './protocol-source.mjs';
import { checkManifest, digest, installedGraphs, referencePath, scriptNames } from './reference-boundary.mjs';
import { inspectImports } from './check-reference-imports.mjs';

export const runNames = [...scriptNames, 'container'];

export function referenceCommand(root, name, extra = [], out = `/work/results/reference/${name}`) {
  const { ref } = checkManifest(root);
  if (!runNames.includes(name)) throw new Error('unknown reference check');
  if (extra.length && !(name === 'vectors' && JSON.stringify(extra) === '["--stage","4"]')) throw new Error('unsupported reference arguments');
  if (name === 'typecheck') {
    const graph = installedGraphs(root);
    const compiler = graph.typeTools.find(file => graph.packages.get(file).name === 'typescript');
    return [path.join(compiler, 'lib/tsc.js'), '-p', path.join(ref, 'tsconfig.json')];
  }
  const module = ['container', 'coverage'].includes(name) ? 'container/run.mjs' : 'tooling/check.mjs';
  const file = path.join(ref, module);
  if (!existsSync(file)) throw new Error(`reference content is incomplete: missing ${module}; no conformance result`);
  return [file, ...(['container', 'coverage'].includes(name) ? [] : [name]), '--out', out, ...extra];
}

export const incompleteCoverageMessage = 'reference coverage incomplete: independent calculations pending or specification groups blocked';
export function referenceFailure(name, report) {
  if (report?.error) return report.error;
  if (name === 'coverage' && report?.complete === false) return incompleteCoverageMessage;
  if (name === 'counterexamples' && report?.status === 'deferred') return report.reason;
  return null;
}

export function testEvidence(report, name) {
  if (!report || typeof report !== 'object' || report.error || report.status === 'failed') throw new Error('reference contract report is missing or failed');
  if (name === 'coverage' && report.format === 'container-gates-v1') return testEvidence(report, 'container');
  const confirmation = (report.checks ?? report.results ?? []).filter(item => item.id === 'provenance.independent-calculations');
  const confirmed = confirmation.length === 1 && confirmation[0].result === 'pass' &&
    Number.isSafeInteger(confirmation[0].evidence?.fields) && confirmation[0].evidence.fields > 0 &&
    confirmation[0].evidence.confirmed === confirmation[0].evidence.fields && confirmation[0].evidence.pending === 0 &&
    confirmation[0].evidence.expectationsAccepted === true;
  if (report.expectationsAccepted === true && !confirmed) throw new Error('reference acceptance lacks complete confirmation evidence');
  if (['primitives', 'vectors', 'limits', 'container'].includes(name)) {
    if (!['executed-contract-v2', 'container-gates-v1'].includes(report.format) || !Array.isArray(report.results) ||
        !report.results.length || report.results.some(item => !item.id || !['pass', 'pending'].includes(item.result))) throw new Error('reference execution is empty or incomplete');
    const ids = report.results.map(item => item.id);
    if (new Set(ids).size !== ids.length) throw new Error('duplicate executed reference ID');
    const pass = report.results.filter(item => item.result === 'pass').length, pending = report.results.length - pass;
    if (pass < 1 || report.counts?.pass !== pass || report.counts?.pending !== pending || typeof report.expectationsAccepted !== 'boolean') throw new Error('reference execution counts/acceptance differ');
    if (name === 'container' && (!confirmed || report.expectationsAccepted !== true || pending || !Array.isArray(report.blake3) || report.blake3.length !== 46 ||
        report.blake3.some(row => row.result !== 'pass' || !ids.includes(row.id)) ||
        new Set(report.blake3.map(row => row.id)).size !== report.blake3.length)) throw new Error('reference container execution is incomplete');
    if (name === 'container') coverageEvidence(report.coverage);
    return { pass, pending, ids, expectationsAccepted: report.expectationsAccepted };
  }
  if (name === 'corpus' || name === 'coverage') {
    if (!['corpus-contract-v2', 'coverage-contract-v2', 'coverage-contract-v3'].includes(report.format) || !(report.cases > 0) || !Array.isArray(report.groups) || !report.groups.length) throw new Error('reference corpus report is empty');
    if (name === 'coverage' && (report.complete !== true || !confirmed || report.expectationsAccepted !== true ||
        report.specificationConditions?.length !== 0)) throw new Error(incompleteCoverageMessage);
    if (name === 'coverage') coverageEvidence(report);
  } else if (name === 'imports' && report.format !== 'module-import-inventory-v1') throw new Error('reference import inventory is missing');
  else if (name === 'counterexamples') {
    if (report.status === 'deferred') throw new Error(report.reason);
    if (!(report.executedMutations > 0)) throw new Error('reference counterexamples are empty');
    const suite = report.counterexamples, baseline = suite?.baseline, mutants = suite?.mutants;
    const bad = () => { throw new Error('reference counterexample execution evidence is incomplete or inconsistent'); };
    if (report.format !== 'counterexample-contract-v1' || report.status !== 'checks-pass' || !confirmed ||
        report.expectationsAccepted !== true || suite?.passed !== true || baseline?.passed !== true ||
        !Array.isArray(report.checks) || report.checks.some(row => row.result !== 'pass') ||
        !Array.isArray(baseline.witnesses) || baseline.witnesses.length !== 15 || !Array.isArray(mutants) || !mutants.length) bad();
    const validWitness = row => row.executed === true && row.targetExecuted === true && typeof row.id === 'string' &&
      Array.isArray(row.path) && row.path.includes('provenance.ts/assertCaseConfirmation') && Object.hasOwn(row, 'expected');
    const witnesses = new Set(baseline.witnesses.map(row => row.id));
    if (witnesses.size !== baseline.witnesses.length || baseline.witnesses.some(row => !validWitness(row) || row.outcome !== 'pass' ||
        !row.path.includes('node:assert/deepStrictEqual') || !Object.hasOwn(row, 'actual') || !isDeepStrictEqual(row.actual, row.expected))) bad();
    const kinds = {}, ids = new Set(); let detected = 0, ineffective = 0;
    for (const mutant of mutants) {
      if (!mutant.id || ids.has(mutant.id) || !mutant.rule || !mutant.operation || typeof mutant.violatesRule !== 'boolean' ||
          !['pae', 'purpose', 'claims', 'expectation', 'priority', 'control'].includes(mutant.kind) ||
          !Array.isArray(mutant.witnesses) || !mutant.witnesses.length || !Array.isArray(mutant.witnessResults) ||
          new Set(mutant.witnesses).size !== mutant.witnesses.length ||
          JSON.stringify(mutant.witnesses) !== JSON.stringify(mutant.witnessResults.map(row => row.id))) bad();
      ids.add(mutant.id);
      const expectedOutcome = mutant.kind === 'expectation' ?
        (mutant.requiredTestFailure === 'confirmation_value_mismatch' ? 'confirmation-failure' :
          mutant.requiredTestFailure === 'expectation_snapshot_mismatch' ? 'expectation-failure' : null) : 'comparison-failure';
      for (const row of mutant.witnessResults) {
        if (!validWitness(row) || !witnesses.has(row.id) || !row.path.includes('mutation/' + mutant.operation) ||
            !['pass', expectedOutcome].includes(row.outcome)) bad();
        if (row.outcome === 'pass' || row.outcome === 'comparison-failure') {
          if (!row.path.includes('node:assert/deepStrictEqual') || !Object.hasOwn(row, 'actual') ||
              (isDeepStrictEqual(row.actual, row.expected) !== (row.outcome === 'pass'))) bad();
        }
        if (row.outcome !== 'pass' && (typeof row.failure !== 'string' || !row.failure.startsWith(mutant.requiredTestFailure))) bad();
      }
      const group = kinds[mutant.kind] ??= { total: 0, detected: 0, ineffective: 0, survived: 0, invalidExecution: 0 }; group.total++;
      if (mutant.violatesRule) {
        if (mutant.kind === 'control' || mutant.verdict !== 'detected' || mutant.survived !== false || !mutant.witnessResults.some(row => row.outcome === expectedOutcome)) bad();
        detected++; group.detected++;
      } else {
        if (mutant.kind !== 'control' || mutant.verdict !== 'ineffective' || mutant.survived !== true || mutant.witnessResults.some(row => row.outcome !== 'pass')) bad();
        ineffective++; group.ineffective++;
      }
    }
    if (detected !== 17 || ineffective !== 4 || report.executedMutations !== mutants.length ||
        new Set(mutants.flatMap(row => row.witnesses)).size !== witnesses.size ||
        !isDeepStrictEqual(Object.fromEntries(Object.entries(kinds).map(([kind, value]) => [kind, value.total])),
          { pae: 3, purpose: 5, claims: 3, expectation: 4, priority: 2, control: 4 }) ||
        !isDeepStrictEqual(suite.counts, { mutants: mutants.length, detected, ineffective, survived: 0, invalidExecution: 0 }) ||
        !isDeepStrictEqual(suite.byKind, kinds) ||
        report.counts?.pass !== report.checks.length + 1 + mutants.length || report.counts.pending !== 0 || report.counts.fail !== 0) bad();
  }
  return { expectationsAccepted: report.expectationsAccepted === true };
}

function coverageEvidence(report) {
  if (report?.format !== 'coverage-contract-v3' || report.complete !== true || report.passed !== report.cases ||
      !['rows', 'cases', 'schemaFields', 'reasons', 'boundaryPairs', 'aggregatePairs'].every(key => Number.isSafeInteger(report[key]) && report[key] > 0) ||
      !Array.isArray(report.outstanding) || report.outstanding.length || report.confirmationsPending !== 0 ||
      !Array.isArray(report.specificationConditions) || report.specificationConditions.length) throw new Error(incompleteCoverageMessage);
}

export function referenceReceipt(root, receipt, expectedSha) {
  if (!/^[a-f0-9]{40}$/.test(expectedSha ?? '')) throw new Error('an independently supplied --source-sha is required');
  return verifySource(root, receipt, expectedSha);
}

export async function runReference(root, name, extra = [], expectedSha, outputDirectory) {
  const out = '/work/results/reference';
  const contractOut = path.resolve(outputDirectory ?? path.join(out, name));
  if (contractOut !== path.join(out, name)) throw new Error('reference --out must name its exact external check directory');
  mkdirSync(out, { recursive: true });
  const report = { check: name, runtime: { node: process.versions.node, openssl: process.versions.openssl }, expected_source_sha: expectedSha ?? null, source_sha: null, ok: false, exit: null, command: null, diagnostic: null };
  let status = 1, sourceReceipt = null, capturedLogs = false;
  try {
    if (process.versions.node.split('.')[0] !== '24') throw new Error('full reference checks require Node 24');
    const receipt = json(path.join(out, 'source.json'));
    referenceReceipt(root, receipt, expectedSha);
    sourceReceipt = receipt;
    report.source_sha = receipt.commit;
    if (existsSync(contractOut)) throw new Error('reference output directory must be fresh');
    report.inputs = receipt.files.filter(item => item.path.startsWith(referencePath + '/') || item.path.startsWith('engine/protocol/testdata/') || item.path.startsWith('tools/ci/reference-') || item.path === 'tools/ci/check-reference-imports.mjs' || item.path === 'pnpm-lock.yaml')
      .map(item => ({ path: item.path, sha256: item.sha256 }));
    const command = referenceCommand(root, name, extra, contractOut);
    report.command = command === null ? 'inspectImports (in process)' : [process.execPath, ...command];
    let importIssues = [];
    if (name === 'imports' || name === 'counterexamples') {
      const graph = await inspectImports(root);
      importIssues = graph.issues;
      writeFileSync(path.join(out, `${name}-graph.json`), JSON.stringify({ source_sha: receipt.commit, ...graph }, null, 2) + '\n');
    }
    if (command) {
      const child = spawnSync(process.execPath, command, { cwd: path.join(root, referencePath), encoding: 'utf8', maxBuffer: 64 * 1024 * 1024 });
      capturedLogs = true;
      writeFileSync(path.join(out, `${name}.stdout`), child.stdout ?? '');
      writeFileSync(path.join(out, `${name}.stderr`), child.stderr ?? '');
      if (child.error || child.signal || child.status === null) throw new Error(`reference infrastructure failure: ${child.error?.message ?? child.signal}`);
      process.stdout.write(child.stdout); process.stderr.write(child.stderr);
      status = child.status;
      const reportName = ['container', 'coverage'].includes(name) ? 'container' : name + (extra.length ? '-stage-' + extra[1] : '');
      const reportFile = path.join(contractOut, reportName + '.json');
      if (name !== 'typecheck' && existsSync(reportFile)) {
        const result = json(reportFile);
        report.contract_report_sha256 = digest(readFileSync(reportFile));
        report.contract = result;
        report.expectationsAccepted = result.expectationsAccepted === true;
        if (status !== 0) report.diagnostic = referenceFailure(name, result);
        if (status === 0) report.execution = testEvidence(result, name);
      } else if (name !== 'typecheck' && status === 0) throw new Error('reference contract report is missing');
    } else status = 0;
    if (importIssues.length) throw new Error(importIssues.join('\n'));
    report.ok = status === 0;
    report.exit = status;
  } catch (error) {
    report.diagnostic = error.message;
    report.exit = status = 1;
    console.error(error.message);
  } finally {
    if (sourceReceipt) {
      try { referenceReceipt(root, sourceReceipt, expectedSha); }
      catch (error) { report.ok = false; report.exit = status = 1; report.diagnostic = error.message; console.error(error.message); }
    }
    for (const extension of ['stdout', 'stderr']) {
      const file = path.join(out, `${name}.${extension}`);
      if (capturedLogs && existsSync(file)) (report.logs ??= {})[extension] = digest(readFileSync(file));
    }
    writeFileSync(path.join(out, `${name}-status.json`), JSON.stringify(report, null, 2) + '\n');
  }
  return status;
}
if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const [name, ...rest] = process.argv.slice(2);
  const extra = rest[0] === '--' ? rest.slice(1) : rest;
  const root = fileURLToPath(new URL('../../', import.meta.url));
  if (!runNames.includes(name)) throw new Error('unknown reference check');
  const { values } = parseArgs({ args: extra, options: { 'source-sha': { type: 'string' }, stage: { type: 'string' }, out: { type: 'string' } }, strict: true });
  process.exitCode = await runReference(path.resolve(root), name, values.stage === undefined ? [] : ['--stage', values.stage], values['source-sha'], values.out);
}
