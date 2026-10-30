import { baseline, cli, clean, compareCounts, InputError, json, natural, problem, read,
  repeatedOption, required, selected, stringOption } from './common.mjs';

export function check(options) {
  required(options, 'baseline', 'metadata', 'input');
  const limits = baseline(options.baseline, 'packages', natural);
  const names = selected(limits, options.package);
  const metadata = json(options.metadata);
  if (!metadata || !Array.isArray(metadata.packages) || !metadata.packages.every(item => item && typeof item.id === 'string' && typeof item.name === 'string')) {
    throw new InputError(problem(options.metadata, 1, 'expected cargo metadata packages with id and name'));
  }
  const ids = new Map(metadata.packages.map(item => [item.id, item.name]));
  const counts = new Map();
  const diagnostics = new Map();
  const issues = [];
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
    if (item.reason === 'compiler-artifact') counts.set(name, 0);
    else {
      const message = item.message;
      if (!message || typeof message.level !== 'string') throw new InputError(problem(options.input, index + 1, 'invalid compiler diagnostic'));
      if (message.level === 'error' || message.level === 'failure-note') issues.push(problem(options.input, index + 1, `${name}: compiler error`));
      if (typeof message.code?.code === 'string' && message.code.code.startsWith('clippy::')) {
        diagnostics.set(name, (diagnostics.get(name) ?? 0) + 1);
      }
    }
  }
  if (!finished) throw new InputError(problem(options.input, 1, 'missing build-finished message; output may be truncated'));
  for (const name of counts.keys()) counts.set(name, diagnostics.get(name) ?? 0);
  for (const name of names) if (counts.has(name)) console.log(problem(options.input, 1, `${name}: ${counts.get(name)} Clippy diagnostics`));
  return [...issues, ...compareCounts(options.baseline, limits, counts, names)];
}

await cli(import.meta.url, `Usage: node tools/ci/clippy-ratchet.mjs --baseline FILE --metadata FILE --input FILE [--package NAME ...]
Read cargo clippy --message-format=json stdout and cargo metadata --format-version=1 JSON.
Each selected package needs a compiler-artifact, including when cached, and a successful build-finished.
The baseline is {"version":1,"packages":{"package-name":0}}. All baseline packages are checked by default.
Exit: 0 passed, 1 regression/missing package/build failure, 2 usage or input error.`, {
  baseline: stringOption, metadata: stringOption, input: stringOption, package: repeatedOption,
}, check);
