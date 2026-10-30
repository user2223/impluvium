import { readFileSync, readdirSync, realpathSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';

export class InputError extends Error {}

export function problem(file, line, message) {
  return `${file}:${line}: ${message}`;
}

export function read(file) {
  try {
    return readFileSync(file, 'utf8');
  } catch (error) {
    throw new InputError(problem(file, 1, error.code ?? error.message));
  }
}

export function json(file) {
  const source = read(file);
  try {
    return JSON.parse(source);
  } catch (error) {
    const offset = Number(/position (\d+)/.exec(error.message)?.[1] ?? 0);
    throw new InputError(problem(file, lineAt(source, offset), 'invalid JSON'));
  }
}

export const lineAt = (source, offset) => source.slice(0, offset).split('\n').length;
export const keyLine = (source, key) => lineAt(source, Math.max(0, source.indexOf(JSON.stringify(key))));
export const plainObject = value => value !== null && typeof value === 'object' && !Array.isArray(value);
export const natural = value => Number.isSafeInteger(value) && value >= 0;
export const clean = source => source.replace(/\x1b\[[0-?]*[ -/]*[@-~]/g, '').replaceAll('\r', '');

// Never descend into generated trees or the checkers' own fixture repositories.
export function files(root, directory = '') {
  const result = [];
  const ignored = new Set(['.git', 'node_modules', 'target', '.nuxt', '.output']);
  const visit = relative => {
    const absolute = path.join(root, relative);
    let entries;
    try {
      entries = readdirSync(absolute, { withFileTypes: true });
    } catch (error) {
      throw new InputError(problem(absolute, 1, error.code ?? error.message));
    }
    for (const entry of entries.sort((a, b) => a.name.localeCompare(b.name))) {
      const child = path.join(relative, entry.name);
      if (ignored.has(entry.name) || child === path.join('tools', 'ci', 'test', 'fixtures')) continue;
      if (entry.isDirectory()) visit(child);
      else if (entry.isFile()) result.push(child.split(path.sep).join('/'));
      else if (entry.isSymbolicLink()) {
        throw new InputError(problem(child, 1, 'symlinks are not supported in checked inputs'));
      }
    }
  };
  visit(directory);
  return result;
}

export function baseline(file, field, validate, allowEmpty = false) {
  const value = json(file);
  if (!plainObject(value) || value.version !== 1 || !plainObject(value[field]) ||
      (!allowEmpty && !Object.keys(value[field]).length) || !Object.entries(value[field]).every(([name, count]) => name && validate(count))) {
    throw new InputError(problem(file, 1, `expected {"version":1,"${field}":{...}} with valid entries`));
  }
  return value[field];
}

export function selected(packages, requested = []) {
  return requested.length ? [...new Set(requested)] : Object.keys(packages);
}

export function compareCounts(file, limits, counts, names) {
  const issues = [];
  const source = read(file);
  for (const name of names) {
    const where = keyLine(source, name);
    if (!Object.hasOwn(limits, name)) issues.push(problem(file, where, `${name}: package missing from baseline`));
    else if (!counts.has(name)) issues.push(problem(file, where, `${name}: count missing from input (zero must be observed)`));
    else if (counts.get(name) > limits[name]) {
      issues.push(problem(file, where, `${name}: ${counts.get(name)} diagnostics exceeds baseline ${limits[name]}`));
    } else if (counts.get(name) < limits[name]) {
      issues.push(problem(file, where, `lower the baseline for ${name} from ${limits[name]} to ${counts.get(name)} in this change`));
    }
  }
  return issues;
}

// Compare every baseline entry, even when this invocation selects one package.
export function previousLimits(options, limits, validate = natural, relaxed = (value, old) => value > old) {
  if (!options['previous-baseline']) return [];
  const previous = baseline(options['previous-baseline'], 'packages', validate);
  const source = read(options.baseline);
  const issues = [];
  for (const [name, value] of Object.entries(limits)) {
    if (!Object.hasOwn(previous, name)) {
      issues.push(problem(options.baseline, keyLine(source, name), `${name}: added package in baseline`));
    } else if (relaxed(value, previous[name])) {
      issues.push(problem(options.baseline, keyLine(source, name), `${name}: baseline was relaxed relative to --previous-baseline`));
    }
  }
  return issues;
}

export const previousDiagnostics = (options, limits) =>
  previousLimits(options, limits, natural, (value, old) => value > old);

export function required(options, ...names) {
  for (const name of names) {
    if (options[name] === undefined) throw new InputError(problem('<arguments>', 1, `--${name} is required`));
  }
}

export async function cli(url, usage, options, main) {
  if (!process.argv[1] || realpathSync(process.argv[1]) !== realpathSync(fileURLToPath(url))) return;
  try {
    const { values } = parseArgs({ options: { help: { type: 'boolean' }, ...options }, strict: true });
    if (values.help) {
      console.log(usage);
      return;
    }
    const issues = await main(values);
    for (const issue of issues) console.error(issue);
    process.exitCode = issues.length ? 1 : 0;
  } catch (error) {
    console.error(error instanceof InputError ? error.message : problem('<arguments>', 1, error.message));
    process.exitCode = 2;
  }
}

export const stringOption = { type: 'string' };
export const repeatedOption = { type: 'string', multiple: true };
