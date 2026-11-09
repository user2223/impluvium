import { baseline, cli, clean, compareCounts, InputError, natural, previousDiagnostics, problem, read, required, stringOption } from './common.mjs';

// Compiler-option arguments, missing/unsupported/out-of-root inputs, project
// reference/output settings, and malformed or empty project configurations.
// Keep source diagnostics (including the TS6133 unused family) budgetable.
const configurationCodes = new Set([6044, 6045, 6046, 6048, 6050, 6051, 6053, 6054, 6059, 6064, 6231,
  6304, 6305, 6306, 6307, 6308, 6309, 6310, 18000, 18001, 18002, 18003]);

// This is a single package rename, not a configurable source of new budgets.
const predecessor = 'view';
const successor = 'impluvium-desktop-ui';

function previousTypeScriptDiagnostics(options, limits) {
  const issues = [];
  const hasBoth = packages => Object.hasOwn(packages, predecessor) && Object.hasOwn(packages, successor);
  if (hasBoth(limits)) {
    issues.push(problem(options.baseline, 1, `${predecessor} and ${successor}: both names cannot claim a budget`));
  }
  if (!options['previous-baseline']) return issues;
  const previous = baseline(options['previous-baseline'], 'packages', natural, true);
  if (hasBoth(previous)) {
    issues.push(problem(options['previous-baseline'], 1, `${predecessor} and ${successor}: duplicate predecessor budgets`));
  }
  if (Object.hasOwn(limits, successor) && !Object.hasOwn(previous, successor)) {
    if (!Object.hasOwn(previous, predecessor)) {
      issues.push(problem(options.baseline, 1, `${successor}: missing predecessor ${predecessor} in --previous-baseline`));
    } else if (!hasBoth(limits) && !hasBoth(previous)) {
      // Compare the successor's budget against the predecessor only during the
      // transition. Subsequent revisions compare the successor normally.
      const migrated = { ...limits, [predecessor]: limits[successor] };
      delete migrated[successor];
      return [...issues, ...previousDiagnostics(options, migrated)];
    }
  }
  return [...issues, ...previousDiagnostics(options, limits)];
}

export function check(options) {
  required(options, 'baseline', 'package', 'input', 'status');
  if (!/^\d+$/.test(options.status) || Number(options.status) > 255) throw new InputError(problem('<arguments>', 1, '--status must be an exit status from 0 to 255'));
  const limits = baseline(options.baseline, 'packages', natural);
  const lines = clean(read(options.input)).split('\n');
  let count = 0;
  const summaries = [];
  const issues = previousTypeScriptDiagnostics(options, limits);
  for (const [index, line] of lines.entries()) {
    // tsc --pretty false, vue-tsc and the pretty tsc form all use TS codes.
    const diagnostic = /^\s*(?:.*?\(\d+,\d+\):\s*|.*?:\d+:\d+\s*-\s*|)error TS(\d+):/.exec(line);
    if (diagnostic) {
      count++;
      if (/^5\d{3}$/.test(diagnostic[1]) || configurationCodes.has(Number(diagnostic[1]))) {
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
Baseline: {"version":1,"packages":{"impluvium-desktop-ui":0}}. No files are updated automatically.
Only view -> impluvium-desktop-ui may inherit a predecessor budget, without an increase or duplicate names.
Exit: 0 passed, 1 regression/missing package/compiler failure without diagnostics, 2 usage or input error.`, {
  baseline: stringOption, package: stringOption, input: stringOption, status: stringOption, 'previous-baseline': stringOption,
}, check);
