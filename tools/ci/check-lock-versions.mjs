import { baseline, cli, keyLine, plainObject, problem, read, required, stringOption, InputError } from './common.mjs';
import { toml } from './toml.mjs';

const version = value => typeof value === 'string' && /^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$/.test(value);

// Every package named in the expectations must be locked, and locked only at the expected version.
export function check(options) {
  required(options, 'lock', 'expect');
  const expected = baseline(options.expect, 'packages', version);
  const { data, entries } = toml(options.lock);
  const packages = data.package;
  if (!Array.isArray(packages) || !packages.length || !packages.every(item => plainObject(item) &&
      typeof item.name === 'string' && typeof item.version === 'string')) {
    throw new InputError(problem(options.lock, 1, 'expected [[package]] tables with name and version'));
  }
  const lines = entries.filter(entry => entry.keys.length === 2 && entry.keys[0] === 'package' && entry.keys[1] === 'name')
    .map(entry => entry.line);
  if (lines.length !== packages.length) throw new InputError(problem(options.lock, 1, 'unsupported [[package]] layout'));
  const source = read(options.expect);
  const issues = [];
  for (const [name, want] of Object.entries(expected)) {
    const locked = packages.map((item, index) => ({ ...item, line: lines[index] })).filter(item => item.name === name);
    if (!locked.length) {
      issues.push(problem(options.expect, keyLine(source, name), `${name}: not in ${options.lock}; expected ${want}`));
      continue;
    }
    for (const item of locked) {
      if (item.version !== want) issues.push(problem(options.lock, item.line, `${name}: locked at ${item.version}, expected ${want}`));
    }
    if (locked.every(item => item.version === want)) console.log(problem(options.lock, locked[0].line, `${name}: ${want}`));
  }
  return issues;
}

await cli(import.meta.url, `Usage: node tools/ci/check-lock-versions.mjs --lock Cargo.lock --expect FILE
Check that each named package is in the Cargo lock file, at exactly the expected version and at no other.
Expectations: {"version":1,"packages":{"package-name":"1.2.3"}}.
Exit: 0 passed, 1 missing or different version, 2 usage or malformed input.`, {
  lock: stringOption, expect: stringOption,
}, check);
