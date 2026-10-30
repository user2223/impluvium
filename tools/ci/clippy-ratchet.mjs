import { baseline, cli, clean, compareCounts, InputError, natural, previousDiagnostics, problem, read,
  repeatedOption, required, selected, stringOption } from './common.mjs';
import { cargoMetadata } from './metadata.mjs';

export function check(options) {
  required(options, 'baseline', 'metadata', 'input');
  const limits = baseline(options.baseline, 'packages', natural);
  const names = selected(limits, options.package).filter(name => !(options.exclude ?? []).includes(name));
  const metadata = cargoMetadata(options.metadata);
  const ids = new Map(metadata.packages.map(item => [item.id, item.name]));
  const members = new Set(metadata.workspace_members);
  const excluded = new Set(options.exclude ?? []);
  const counts = new Map();
  const diagnostics = new Map();
  const issues = previousDiagnostics(options, limits);
  const observed = new Set();
  let finished = false;
  for (const [index, raw] of clean(read(options.input)).split('\n').entries()) {
    if (!raw.trim()) continue;
    let item;
    try { item = JSON.parse(raw); } catch {
      throw new InputError(problem(options.input, index + 1, 'expected Cargo JSON lines; capture stdout separately from progress output'));
    }
    if (!item || typeof item.reason !== 'string') throw new InputError(problem(options.input, index + 1, 'missing Cargo message reason'));
    if (item.reason === 'build-finished') {
      if (typeof item.success !== 'boolean' || finished) throw new InputError(problem(options.input, index + 1, 'invalid or repeated build-finished message'));
      finished = true;
      if (!item.success) issues.push(problem(options.input, index + 1, 'Cargo did not finish successfully'));
    }
    if (!['compiler-artifact', 'compiler-message'].includes(item.reason)) continue;
    const name = ids.get(item.package_id);
    if (!name) throw new InputError(problem(options.input, index + 1, `package id not found in metadata: ${item.package_id}`));
    if (members.has(item.package_id)) observed.add(name);
    if (item.reason === 'compiler-artifact') {
      if (members.has(item.package_id)) counts.set(item.package_id, 0);
    } else {
      const message = item.message;
      if (!message || typeof message.level !== 'string') throw new InputError(problem(options.input, index + 1, 'invalid compiler diagnostic'));
      if (message.level === 'error' || message.level === 'failure-note') issues.push(problem(options.input, index + 1, `${name}: compiler error`));
      if (members.has(item.package_id) && typeof message.code?.code === 'string' && message.code.code.startsWith('clippy::')) {
        diagnostics.set(item.package_id, (diagnostics.get(item.package_id) ?? 0) + 1);
      }
    }
  }
  if (!finished) throw new InputError(problem(options.input, 1, 'missing build-finished message; output may be truncated'));
  for (const name of observed) {
    if (excluded.has(name)) continue;
    if (!Object.hasOwn(limits, name)) issues.push(problem(options.baseline, 1, `${name}: observed workspace package missing from baseline`));
    if (!names.includes(name)) issues.push(problem(options.input, 1, `${name}: observed workspace package outside --package selection; include it or use --exclude explicitly`));
  }
  const namedCounts = new Map([...counts.keys()].map(id => [ids.get(id), diagnostics.get(id) ?? 0]));
  for (const name of names) if (namedCounts.has(name)) console.log(problem(options.input, 1, `${name}: ${namedCounts.get(name)} Clippy diagnostics`));
  return [...issues, ...compareCounts(options.baseline, limits, namedCounts, names)];
}

await cli(import.meta.url, `Usage: node tools/ci/clippy-ratchet.mjs --baseline FILE --metadata FILE --input FILE [--package NAME ...] [--exclude NAME ...] [--previous-baseline FILE]
Read cargo clippy --message-format=json stdout and cargo metadata --format-version=1 JSON.
Each selected package needs a compiler-artifact, including when cached, and a successful build-finished.
The baseline is {"version":1,"packages":{"package-name":0}}. All baseline packages are checked by default.
Exit: 0 passed, 1 regression/missing package/build failure, 2 usage or input error.`, {
  baseline: stringOption, metadata: stringOption, input: stringOption, package: repeatedOption, exclude: repeatedOption, 'previous-baseline': stringOption,
}, check);
