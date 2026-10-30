import { baseline, cli, clean, compareCounts, InputError, natural, previousDiagnostics, problem, read, required, stringOption } from './common.mjs';

export function check(options) {
  required(options, 'baseline', 'package', 'input', 'status');
  if (!/^\d+$/.test(options.status) || Number(options.status) > 255) throw new InputError(problem('<arguments>', 1, '--status must be an exit status from 0 to 255'));
  const limits = baseline(options.baseline, 'packages', natural);
  const lines = clean(read(options.input)).split('\n');
  let count = 0;
  const summaries = [];
  const issues = previousDiagnostics(options, limits);
  for (const [index, line] of lines.entries()) {
    // tsc --pretty false, vue-tsc and the pretty tsc form all use TS codes.
    const diagnostic = /^\s*(?:.*?\(\d+,\d+\):\s*|.*?:\d+:\d+\s*-\s*|)error TS(\d+):/.exec(line);
    if (diagnostic) {
      count++;
      if (/^[56]\d{3}$|^18003$/.test(diagnostic[1])) {
        issues.push(problem(options.input, index + 1, `configuration/input diagnostic TS${diagnostic[1]} cannot be budgeted`));
      }
    }
    const summary = /^\s*Found (\d+) errors?(?: in .*)?\.?\s*$/.exec(line);
    if (summary) summaries.push({ count: Number(summary[1]), line: index + 1 });
  }
  if (summaries.length > 1 || summaries.some(item => item.count !== count)) {
    throw new InputError(problem(options.input, summaries[0]?.line ?? 1, 'diagnostic count disagrees with tsc summary; use one complete run per input'));
  }
  const status = Number(options.status);
  if (![0, 1, 2].includes(status)) issues.push(problem(options.input, 1, `typecheck exit status ${status} is not a completed tsc/vue-tsc run (expected 0, 1 or 2)`));
  if (status !== 0 && count === 0) issues.push(problem(options.input, 1, `typecheck exited ${status} without TypeScript diagnostics`));
  if (status === 0 && count !== 0) throw new InputError(problem(options.input, 1, 'successful exit status contradicts TypeScript errors'));
  console.log(problem(options.input, 1, `${options.package}: ${count} TypeScript diagnostics`));
  return [...issues, ...compareCounts(options.baseline, limits, new Map([[options.package, count]]), [options.package])];
}

await cli(import.meta.url, `Usage: node tools/ci/tsc-ratchet.mjs --baseline FILE --package NAME --input FILE --status STATUS [--previous-baseline FILE]
Read one captured tsc/vue-tsc/Nuxt typecheck run and its original exit status (before tee).
Prefer --pretty false. ANSI colours are accepted. Empty output plus status 0 explicitly records zero.
Baseline: {"version":1,"packages":{"view":0}}. No files are updated automatically.
Exit: 0 passed, 1 regression/missing package/compiler failure without diagnostics, 2 usage or input error.`, {
  baseline: stringOption, package: stringOption, input: stringOption, status: stringOption, 'previous-baseline': stringOption,
}, check);
