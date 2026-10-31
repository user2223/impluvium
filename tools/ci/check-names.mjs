import path from 'node:path';
import { rustCode } from './check-features.mjs';
import { cli, files, InputError, json, keyLine, natural, plainObject, problem, read, stringOption } from './common.mjs';

// Names inherited from the upstream project. Every pattern counts a line at most once per file.
export const PATTERNS = {
  word: /\bDrop\b/,
  oss: /drop-?oss/i,
  scheme: /drop:\/\//,
  'desktop-title': /Drop Desktop (?:Client|App)/,
  'app-package': /\bdrop-app\b/,
  'app-lib': /\bdrop_app_lib\b/,
  'rpm-package': /\bdrop-desktop-client\b/,
  'build-arg': /\bBUILD_DROP_VERSION\b/,
  'literal-name': /["']drop["']/,
  'literal-debug-name': /["']drop-debug["']/,
};

// Rust's destructor trait is not a product name. Only the `Drop` token of `Drop for`, `ops::Drop` and `Drop::drop` is
// blanked, and only in code: comments and string literals are masked first, so text between or around the tokens still
// counts.
const DESTRUCTOR = /\b(Drop)(?=\s+for\b)|\bops::(Drop)\b|\b(Drop)(?=::drop\b)/dg;

function withoutDestructors(line, code) {
  let result = line;
  for (const match of code.matchAll(DESTRUCTOR)) {
    const [from, to] = match.indices.slice(1).find(Boolean);
    result = result.slice(0, from) + ' '.repeat(to - from) + result.slice(to);
  }
  return result;
}

function counts(file, validate) {
  const data = json(file);
  if (!plainObject(data) || data.version !== 1 || !plainObject(data.files)) {
    throw new InputError(problem(file, 1, 'expected {"version":1,"files":{"path":{"pattern":count}}}'));
  }
  for (const [name, entry] of Object.entries(data.files)) {
    if (!validate(name) || !plainObject(entry) || !Object.entries(entry).every(([id, count]) => Object.hasOwn(PATTERNS, id) && natural(count) && count > 0)) {
      throw new InputError(problem(file, 1, `${name}: expected a repository-relative path with positive counts for known patterns`));
    }
  }
  return data.files;
}

const relativePath = name => name && !path.posix.isAbsolute(name) && !name.includes('\\') && path.posix.normalize(name) === name && !name.startsWith('../');

const globPattern = glob => new RegExp('^' + glob.split('**').map(part => part.split('*').map(piece => piece.replace(/[.+?^${}()|[\]\\]/g, '\\$&')).join('[^/]*')).join('.*') + '$');

function allowList(file) {
  const data = json(file);
  if (!plainObject(data) || data.version !== 1 || !Array.isArray(data.entries)) {
    throw new InputError(problem(file, 1, 'expected {"version":1,"entries":[...]}'));
  }
  return data.entries.map((entry, index) => {
    const where = `entry ${index}`;
    const selectors = ['path', 'glob'].filter(key => entry?.[key] !== undefined);
    if (!plainObject(entry) || selectors.length !== 1 || typeof entry[selectors[0]] !== 'string' || !entry[selectors[0]] ||
        typeof entry.reason !== 'string' || !entry.reason.trim() ||
        (entry.lines !== undefined && (!Array.isArray(entry.lines) || !entry.lines.length || !entry.lines.every(line => typeof line === 'string' && line.trim()))) ||
        Object.keys(entry).some(key => !['path', 'glob', 'lines', 'reason'].includes(key))) {
      throw new InputError(problem(file, 1, `${where}: expected {"path"|"glob", "lines"?, "reason"}`));
    }
    if (selectors[0] === 'path' && !relativePath(entry.path)) throw new InputError(problem(file, 1, `${where}: invalid path`));
    return { ...entry, matches: selectors[0] === 'path' ? name => name === entry.path : (matcher => name => matcher.test(name))(globPattern(entry.glob)) };
  });
}

export function count(root, allowed) {
  const result = {};
  const issues = [];
  for (const relative of files(root)) {
    const source = read(path.join(root, relative));
    if (source.includes('\0')) continue;
    const rules = allowed.filter(entry => entry.matches(relative));
    if (rules.some(entry => !entry.lines)) continue;
    const lines = source.split('\n').map(line => line.replace(/\r$/, ''));
    const present = new Set(lines.map(line => line.trim()));
    const exempt = new Set(rules.flatMap(entry => entry.lines.map(line => line.trim())));
    for (const line of exempt) if (!present.has(line)) issues.push(problem(relative, 1, `allow-list line no longer present: ${line}`));
    const masked = relative.endsWith('.rs') ? rustCode(source, relative).split('\n') : null;
    lines.forEach((line, index) => {
      if (exempt.has(line.trim())) return;
      // Blank the destructor spans found in the code, keep everything else (including strings) for counting.
      const counted = masked ? withoutDestructors(line, masked[index]) : line;
      for (const [id, pattern] of Object.entries(PATTERNS)) {
        if (!pattern.test(counted)) continue;
        result[relative] ??= {};
        result[relative][id] = (result[relative][id] ?? 0) + 1;
      }
    });
  }
  return { result, issues };
}

function format(data) {
  const keys = Object.keys(data).sort();
  const body = keys.map(name => `  ${JSON.stringify(name)}: ${JSON.stringify(Object.fromEntries(Object.entries(data[name]).sort(([a], [b]) => a.localeCompare(b))))}`);
  return `{"version":1,"files":{${body.length ? '\n' + body.join(',\n') + '\n' : ''}}}\n`;
}

export function check(options) {
  const root = path.resolve(options.root ?? '.');
  const baselineFile = options.baseline ?? path.join(root, 'tools/ci/names-baseline.json');
  const allowFile = options.allow ?? path.join(root, 'tools/ci/names-allow.json');
  const { result, issues } = count(root, allowList(allowFile));
  if (options['print-baseline']) {
    process.stdout.write(format(result));
    return [];
  }
  const limits = counts(baselineFile, relativePath);
  const source = read(baselineFile);
  const names = [...new Set([...Object.keys(limits), ...Object.keys(result)])].sort();
  for (const name of names) {
    for (const id of Object.keys(PATTERNS)) {
      const observed = result[name]?.[id] ?? 0;
      const limit = limits[name]?.[id] ?? 0;
      const where = keyLine(source, name);
      if (observed > limit) issues.push(problem(Object.hasOwn(limits, name) ? baselineFile : name, Object.hasOwn(limits, name) ? where : 1, `${name}: ${observed} lines for ${id} exceed baseline ${limit}; use the product name or the identity table`));
      else if (observed < limit) issues.push(problem(baselineFile, where, `lower the baseline for ${name} ${id} from ${limit} to ${observed} in this change`));
    }
  }
  if (options['previous-baseline']) {
    const previous = counts(options['previous-baseline'], relativePath);
    for (const [name, entry] of Object.entries(limits)) for (const [id, value] of Object.entries(entry)) {
      if (value > (previous[name]?.[id] ?? 0)) issues.push(problem(baselineFile, keyLine(source, name), `${name}: ${id} baseline grew relative to --previous-baseline`));
    }
  }
  return issues;
}

await cli(import.meta.url, `Usage: node tools/ci/check-names.mjs [--root DIR] [--baseline FILE] [--allow FILE] [--previous-baseline FILE] [--print-baseline]
Count lines that carry names inherited from the upstream project, per file and pattern (a line counts once per pattern).
Rust destructor forms (impl Drop for, ops::Drop, Drop::drop) are not names. tools/ci/names-allow.json lists permanent exceptions.
Default baseline: tools/ci/names-baseline.json, {"version":1,"files":{"path":{"pattern":count}}}. The count must equal the baseline.
--previous-baseline (the base revision's file) allows the counts to shrink only. --print-baseline prints the current counts as a
new baseline to stdout and never writes a file.
Exit: 0 passed, 1 count differs/baseline grew, 2 usage or input error.`, {
  root: stringOption, baseline: stringOption, allow: stringOption, 'previous-baseline': stringOption, 'print-baseline': { type: 'boolean' },
}, check);
