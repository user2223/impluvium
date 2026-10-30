import { assignments, baseline, cli, clean, InputError, keyLine, natural, plainObject, problem, read,
  repeatedOption, required, selected, stringOption } from './common.mjs';

export function testCounts(source, file) {
  const counts = { tests: 0, docTests: 0 };
  const seen = new Set();
  const issues = [];
  let kind, pending;
  let announced = false;
  for (const [index, line] of clean(source).split('\n').entries()) {
    const where = index + 1;
    if (/^\s*(?:Running\s+.+\(.+\)|Doc-tests\s+\S+)/.test(line)) {
      if (announced) throw new InputError(problem(file, where, 'test suite ended without a result'));
      kind = /^\s*Doc-tests\s/.test(line) ? 'docTests' : 'tests';
      announced = true;
    }
    const running = /^running (\d+) tests?\s*$/.exec(line);
    if (running) {
      if (!kind || pending !== undefined) throw new InputError(problem(file, where, 'capture one package with Cargo Running/Doc-tests headers and complete libtest suites'));
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
    if (result[1] !== 'ok' || failed) issues.push(problem(file, where, 'test suite failed'));
    counts[kind] += passed;
    seen.add(kind);
    pending = undefined;
    announced = false;
  }
  if (announced) throw new InputError(problem(file, 1, 'truncated libtest output'));
  return { counts, seen, issues };
}

export function check(options) {
  required(options, 'baseline', 'input');
  const limits = baseline(options.baseline, 'packages', value => plainObject(value) &&
    Object.keys(value).length === 2 && natural(value.tests) && natural(value.docTests));
  const names = selected(limits, options.package);
  const inputs = assignments(options.input);
  const issues = [];
  for (const name of inputs.keys()) if (!names.includes(name)) throw new InputError(problem('<arguments>', 1, `unexpected test input for ${name}`));
  for (const name of names) {
    const where = keyLine(read(options.baseline), name);
    if (!Object.hasOwn(limits, name) || !inputs.has(name)) {
      issues.push(problem(options.baseline, where, `${name}: package missing from baseline or inputs`));
      continue;
    }
    const file = inputs.get(name);
    const { counts, seen, issues: failures } = testCounts(read(file), file);
    issues.push(...failures);
    for (const kind of ['tests', 'docTests']) {
      if ((!seen.has(kind) && (kind === 'tests' || limits[name][kind] > 0)) || counts[kind] < limits[name][kind]) {
        issues.push(problem(file, 1, `${name}: ${kind} has ${counts[kind]} successful tests; expected at least ${limits[name][kind]} (missing suites cannot establish zero)`));
      }
    }
    console.log(problem(file, 1, `${name}: ${counts.tests} successful tests, ${counts.docTests} successful doc-tests`));
  }
  return issues;
}

await cli(import.meta.url, `Usage: node tools/ci/count-tests.mjs --baseline FILE --input PACKAGE=FILE [--input PACKAGE=FILE ...] [--package NAME ...]
Capture cargo test --locked -p PACKAGE stdout AND stderr together, one package per file, without --quiet.
Baseline: {"version":1,"packages":{"sample":{"tests":25,"docTests":0}}}.
Counts are minimum successful, non-ignored tests. Doc-tests are separate and never inflate ordinary counts.
All baseline packages are required unless explicitly scoped with --package. Failed suites always fail.
Exit: 0 passed, 1 missing/insufficient tests or failed suites, 2 usage or malformed/truncated input.`, {
  baseline: stringOption, input: repeatedOption, package: repeatedOption,
}, check);
