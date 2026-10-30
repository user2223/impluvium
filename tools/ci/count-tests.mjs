import path from 'node:path';
import { baseline, cli, clean, InputError, keyLine, natural, plainObject, previousLimits, problem, read,
  repeatedOption, required, selected, stringOption } from './common.mjs';
import { cargoMetadata } from './metadata.mjs';

const kinds = ['tests', 'docTests'];
const validLimit = value => plainObject(value) && Object.keys(value).length === 1 &&
  (natural(value.exact) || natural(value.min));
const validEntry = value => plainObject(value) && Object.keys(value).length === 2 && kinds.every(kind => validLimit(value[kind]));
const floor = value => value.exact ?? value.min;
const normalized = name => name.replaceAll('-', '_');

function targetOwners(metadata) {
  const owners = { tests: new Map(), docTests: new Map() };
  for (const pkg of metadata.packages.filter(pkg => metadata.workspace_members.includes(pkg.id))) {
    for (const target of pkg.targets) {
      if (target.kind.includes('custom-build')) continue;
      for (const kind of kinds) {
        if (kind === 'docTests' && !target.kind.some(value => ['lib', 'rlib', 'dylib', 'cdylib', 'staticlib', 'proc-macro'].includes(value))) continue;
        const name = normalized(target.name);
        if (!owners[kind].has(name)) owners[kind].set(name, new Set());
        owners[kind].get(name).add(pkg.name);
      }
    }
  }
  return owners;
}

function suiteHeader(line, file, where, owners) {
  const doc = /^\s*Doc-tests\s+(\S+)\s*$/.exec(line);
  const run = /^\s*Running\s+.+\((.+)\)\s*$/.exec(line);
  if (!doc && !run) return undefined;
  const kind = doc ? 'docTests' : 'tests';
  const binary = run ? path.posix.basename(run[1].replaceAll('\\', '/')) : undefined;
  const target = doc ? doc[1] : binary.replace(/\.exe$/, '').replace(/-[a-f0-9]{8,}$/, '');
  const candidates = owners[kind].get(normalized(target));
  if (!candidates || candidates.size !== 1) {
    throw new InputError(problem(file, where, `cannot uniquely attribute ${kind} target ${target} using metadata; capture unambiguous target names`));
  }
  return { name: [...candidates][0], kind, identity: binary ?? normalized(target) };
}

export function testCounts(source, file, owners, suites = new Set()) {
  const packages = new Map();
  const issues = [];
  let suite, pending;
  let announced = false;
  for (const [index, line] of clean(source).split('\n').entries()) {
    const where = index + 1;
    const header = suiteHeader(line, file, where, owners);
    if (header) {
      if (announced) throw new InputError(problem(file, where, 'test suite ended without a result'));
      const identity = `${header.name}:${header.kind}:${header.identity}`;
      if (suites.has(identity)) throw new InputError(problem(file, where, `repeated test suite ${identity}`));
      suites.add(identity);
      suite = header;
      announced = true;
    }
    const running = /^running (\d+) tests?\s*$/.exec(line);
    if (running) {
      if (!suite || pending !== undefined) throw new InputError(problem(file, where, 'capture Cargo Running/Doc-tests headers and complete libtest suites'));
      pending = Number(running[1]);
      if (!natural(pending)) throw new InputError(problem(file, where, 'invalid running test count'));
      announced = true;
    }
    const result = /^test result: (ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored; (\d+) measured; (\d+) filtered out(?:;.*)?$/.exec(line);
    if (!result) continue;
    if (pending === undefined) throw new InputError(problem(file, where, 'test result without a running count'));
    const [passed, failed, ignored, measured] = result.slice(2, 6).map(Number);
    if (!result.slice(2, 7).map(Number).every(natural)) throw new InputError(problem(file, where, 'invalid libtest result count'));
    if (passed + failed + ignored + measured !== pending) throw new InputError(problem(file, where, 'libtest result does not match the running count'));
    if (result[1] !== 'ok' || failed) issues.push(problem(file, where, `${suite.name}: test suite failed`));
    if (!packages.has(suite.name)) packages.set(suite.name, { counts: { tests: 0, docTests: 0 }, seen: new Set() });
    const entry = packages.get(suite.name);
    entry.counts[suite.kind] += passed;
    entry.seen.add(suite.kind);
    pending = undefined;
    announced = false;
  }
  if (announced) throw new InputError(problem(file, 1, 'truncated libtest output'));
  return { packages, issues };
}

export function check(options) {
  required(options, 'baseline', 'metadata', 'input');
  const limits = baseline(options.baseline, 'packages', validEntry);
  const metadata = cargoMetadata(options.metadata, { targets: true });
  const owners = targetOwners(metadata);
  const names = selected(limits, options.package);
  const issues = previousLimits(options, limits, validEntry, (value, old) => kinds.some(kind =>
    floor(value[kind]) < floor(old[kind]) || (old[kind].exact !== undefined && value[kind].min !== undefined)));
  const packages = new Map();
  const suites = new Set();
  for (const file of options.input) {
    const parsed = testCounts(read(file), file, owners, suites);
    issues.push(...parsed.issues);
    for (const [name, entry] of parsed.packages) {
      if (!Object.hasOwn(limits, name)) issues.push(problem(options.baseline, 1, `${name}: observed package missing from baseline`));
      if (!packages.has(name)) packages.set(name, { counts: { tests: 0, docTests: 0 }, seen: new Set() });
      const total = packages.get(name);
      for (const kind of kinds) {
        total.counts[kind] += entry.counts[kind];
        if (entry.seen.has(kind)) total.seen.add(kind);
      }
    }
  }
  for (const name of names) {
    const where = keyLine(read(options.baseline), name);
    if (!Object.hasOwn(limits, name) || !packages.has(name)) {
      issues.push(problem(options.baseline, where, `${name}: package missing from baseline or inputs`));
      continue;
    }
    const { counts, seen } = packages.get(name);
    for (const kind of kinds) {
      const limit = limits[name][kind];
      const missing = !seen.has(kind) && (kind === 'tests' || floor(limit) > 0);
      const mismatched = limit.exact !== undefined ? counts[kind] !== limit.exact : counts[kind] < limit.min;
      if (missing || mismatched) {
        const expectation = limit.exact !== undefined ? `exactly ${limit.exact}` : `at least ${limit.min}`;
        issues.push(problem(options.baseline, where, `${name}: ${kind} has ${counts[kind]} successful tests; expected ${expectation} (missing suites cannot establish zero)`));
      }
    }
    console.log(problem(options.baseline, where, `${name}: ${counts.tests} successful tests, ${counts.docTests} successful doc-tests`));
  }
  return issues;
}

await cli(import.meta.url, `Usage: node tools/ci/count-tests.mjs --baseline FILE --metadata FILE --input FILE [--input FILE ...] [--package NAME ...] [--previous-baseline FILE]
Capture cargo test --locked stdout AND stderr together, with Cargo headers, without --quiet.
Attribute workspace suites through cargo metadata target names; ambiguous or unknown targets are input errors.
Baseline: {"version":1,"packages":{"sample":{"tests":{"exact":25},"docTests":{"min":0}}}}.
Only successful, non-ignored tests count. Failed/truncated suites fail independently of the baseline.
Exit: 0 passed, 1 missing/mismatched tests or failed suites, 2 usage or malformed/truncated input.`, {
  baseline: stringOption, metadata: stringOption, input: repeatedOption, package: repeatedOption,
  'previous-baseline': stringOption,
}, check);
