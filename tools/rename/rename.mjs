import { realpathSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';
import { files, InputError, json, lineAt, plainObject, problem, read } from '../ci/common.mjs';

// Rewrites the visible product name according to tools/rename/rules.json. It writes files, so unlike tools/ci it is run
// only in a disposable container, from an export of a commit.

const usage = `Usage: node tools/rename/rename.mjs [--root DIR] [--rules FILE] [--identity FILE] [--report | --apply] [--format md|json]
Replace the inherited product name by the value from tools/identity.json, as described by tools/rename/rules.json.
A rule's replacement is an identity key (product.name) or a template with \${key} placeholders (d'\${product.name}).
--report (default) prints every replacement and every match that an exclusion skipped (file, line, before, after, rule)
  to stdout and never writes. --apply writes the files and prints the same report. Applying twice changes nothing.
No file is renamed. Rust destructor forms (impl Drop for, ops::Drop, Drop::drop) are never touched.
Exit: 0 done, 1 an exclusion no longer matches anything, 2 usage or input error.`;

const KINDS = ['text', 'json', 'rust'];
// Rust's destructor trait is not a product name: `Drop for`, `ops::Drop`, `Drop::drop`.
const DESTRUCTOR = /\bDrop(?=\s+for\b)|\bops::Drop\b|\bDrop(?=::drop\b)/g;

// `**/` matches any number of directories (also none), `*` anything but a slash.
const globPattern = glob => new RegExp('^' + glob.split('**/').map(part => part.split('**').map(chunk => chunk.split('*').map(piece => piece.replace(/[.+?^${}()|[\]\\]/g, '\\$&')).join('[^/]*')).join('.*')).join('(?:.*/)?') + '$');
const keyPattern = key => new RegExp('^' + key.split('.').map(part => part === '**' ? '.*' : part === '*' ? '[^.]*' : part.replace(/[.+?^${}()|[\]\\]/g, '\\$&')).join('\\.') + '$');
const lineStart = (source, offset) => source.lastIndexOf('\n', offset - 1) + 1;
const lineText = (source, offset) => source.slice(lineStart(source, offset), (end => end < 0 ? source.length : end)(source.indexOf('\n', offset))).replace(/\r$/, '');

function lookup(identity, name) {
  const value = name.split('.').reduce((node, part) => (plainObject(node) ? node[part] : undefined), identity);
  if (typeof value !== 'string' || !value) throw new InputError(problem('tools/identity.json', 1, `${name}: not a string in the identity table`));
  return value;
}

function loadRules(rulesFile, identity) {
  const data = json(rulesFile);
  if (!plainObject(data) || data.version !== 1 || !Array.isArray(data.rules) || !data.rules.length) {
    throw new InputError(problem(rulesFile, 1, 'expected {"version":1,"rules":[...]}'));
  }
  return data.rules.map((rule, index) => {
    const where = `rule ${index}`;
    const strings = value => Array.isArray(value) && value.length && value.every(item => typeof item === 'string' && item);
    if (!plainObject(rule) || typeof rule.id !== 'string' || !rule.id || !KINDS.includes(rule.kind) || !strings(rule.files) ||
        typeof rule.pattern !== 'string' || typeof rule.replacement !== 'string' ||
        (rule.skip !== undefined && !strings(rule.skip)) || (rule.lines !== undefined && typeof rule.lines !== 'string') ||
        (rule.exclude !== undefined && !Array.isArray(rule.exclude))) {
      throw new InputError(problem(rulesFile, 1, `${where}: expected {id, kind: ${KINDS.join('|')}, files, skip?, pattern, replacement, lines?, exclude?}`));
    }
    const exclude = (rule.exclude ?? []).map((entry, at) => {
      const selectors = ['path', 'glob'].filter(key => typeof entry?.[key] === 'string' && entry[key]);
      const modes = ['lines', 'keys'].filter(key => entry?.[key] !== undefined);
      if (!plainObject(entry) || selectors.length !== 1 || typeof entry.reason !== 'string' || !entry.reason.trim() || modes.length > 1 ||
          modes.some(key => !strings(entry[key])) || Object.keys(entry).some(key => !['path', 'glob', 'lines', 'keys', 'reason'].includes(key)) ||
          (entry.keys && rule.kind !== 'json')) {
        throw new InputError(problem(rulesFile, 1, `${where}, exclude ${at}: expected {"path"|"glob", "lines"|"keys"?, "reason"}`));
      }
      const matcher = selectors[0] === 'path' ? name => name === entry.path : (pattern => name => pattern.test(name))(globPattern(entry.glob));
      return { ...entry, matches: matcher, keyPatterns: (entry.keys ?? []).map(keyPattern), used: new Set() };
    });
    return {
      ...rule,
      regex: new RegExp(rule.pattern, 'gu'),
      lineFilter: rule.lines === undefined ? null : new RegExp(rule.lines, 'u'),
      to: /^[a-z_]+(?:\.[a-z_]+)*$/.test(rule.replacement) ? lookup(identity, rule.replacement) : rule.replacement.replace(/\$\{([^}]+)\}/g, (_, name) => lookup(identity, name)),
      include: rule.files.map(globPattern),
      leave: (rule.skip ?? []).map(globPattern),
      exclude,
    };
  });
}

// Spans of Rust string literals; comments and char literals are skipped, so only text the program can show is a region.
function rustStrings(source, file) {
  const pattern = /\/\/[^\n]*|\/\*|(?:br|cr|r)(#*)"|(?:b|c)?"|'(?:\\(?:u\{[0-9a-fA-F]+\}|x[0-9a-fA-F]{2}|.)|[^'\\\n])'/g;
  const spans = [];
  let match;
  while ((match = pattern.exec(source))) {
    const start = match.index;
    let next = pattern.lastIndex;
    let string = false;
    if (match[0] === '/*') {
      let depth = 1;
      while (next < source.length && depth) {
        if (source.startsWith('/*', next)) { depth++; next += 2; }
        else if (source.startsWith('*/', next)) { depth--; next += 2; }
        else next++;
      }
      if (depth) throw new InputError(problem(file, lineAt(source, start), 'unterminated Rust comment'));
    } else if (match[1] !== undefined) {
      const closing = '"' + match[1];
      const stop = source.indexOf(closing, next);
      if (stop < 0) throw new InputError(problem(file, lineAt(source, start), 'unterminated Rust raw string'));
      next = stop + closing.length;
      string = true;
    } else if (match[0].endsWith('"')) {
      while (next < source.length && source[next] !== '"') {
        if (source[next] === '\\') next++;
        next++;
      }
      if (next >= source.length) throw new InputError(problem(file, lineAt(source, start), 'unterminated Rust string'));
      next++;
      string = true;
    }
    if (string) spans.push([start, next]);
    pattern.lastIndex = next;
  }
  return spans;
}

// String tokens of a JSON document with the dotted key path of every value.
function jsonStrings(source, file) {
  const stack = [];
  const result = [];
  for (let i = 0; i < source.length; i++) {
    const c = source[i];
    const top = stack.at(-1);
    if (c === '{') stack.push({ object: true, key: null, expectKey: true });
    else if (c === '[') stack.push({ object: false, index: 0 });
    else if (c === '}' || c === ']') stack.pop();
    else if (c === ',' && top) { if (top.object) top.expectKey = true; else top.index++; }
    else if (c === ':' && top) top.expectKey = false;
    else if (c === '"') {
      let end = i + 1;
      while (end < source.length && source[end] !== '"') end += source[end] === '\\' ? 2 : 1;
      if (end >= source.length) throw new InputError(problem(file, lineAt(source, i), 'unterminated JSON string'));
      const raw = source.slice(i, end + 1);
      if (top?.object && top.expectKey) top.key = JSON.parse(raw);
      else result.push({ start: i, end: end + 1, raw, key: stack.map(item => item.object ? item.key : String(item.index)).join('.') });
      i = end;
    }
  }
  return result;
}

function process_(rule, file, source, outcome) {
  const exclusions = rule.exclude.filter(entry => entry.matches(file));
  if (exclusions.some(entry => !entry.lines && !entry.keys)) {
    for (const entry of exclusions.filter(item => !item.lines && !item.keys)) outcome.wholeFile.push({ rule: rule.id, file, reason: entry.reason });
    return source;
  }
  const edits = [];
  const note = (offset, before, after, extra) => ({ rule: rule.id, file, line: lineAt(source, offset), before, after, ...extra });
  const skipped = (offset, reason, extra) => outcome.skipped.push({ rule: rule.id, file, line: lineAt(source, offset), text: lineText(source, offset), reason, ...extra });
  const lineExclusion = offset => exclusions.find(entry => entry.lines?.some(line => {
    const hit = line.trim() === lineText(source, offset).trim();
    if (hit) entry.used.add(line);
    return hit;
  }));
  const wanted = offset => !rule.lineFilter || rule.lineFilter.test(lineText(source, offset));

  if (rule.kind === 'json') {
    for (const token of jsonStrings(source, file)) {
      const value = JSON.parse(token.raw);
      const hits = [...value.matchAll(rule.regex)];
      if (!hits.length) continue;
      const keyRule = exclusions.find(entry => entry.keyPatterns.some(pattern => pattern.test(token.key)));
      if (keyRule) { skipped(token.start, keyRule.reason, { key: token.key }); continue; }
      const lineRule = lineExclusion(token.start);
      if (lineRule) { skipped(token.start, lineRule.reason, { key: token.key }); continue; }
      const replaced = value.replace(rule.regex, () => rule.to);
      edits.push({ start: token.start, end: token.end, text: JSON.stringify(replaced), info: note(token.start, value, replaced, { key: token.key }) });
    }
  } else {
    const spans = rule.kind === 'rust' ? rustStrings(source, file) : [[0, source.length]];
    let at = 0;
    for (const match of source.matchAll(rule.regex)) {
      const offset = match.index;
      while (at < spans.length && spans[at][1] <= offset) at++;
      if (at >= spans.length || spans[at][0] > offset || !wanted(offset)) continue;
      if (file.endsWith('.rs') && [...lineText(source, offset).matchAll(DESTRUCTOR)].some(hit => hit.index <= offset - lineStart(source, offset) && offset - lineStart(source, offset) < hit.index + hit[0].length)) continue;
      const lineRule = lineExclusion(offset);
      if (lineRule) { skipped(offset, lineRule.reason); continue; }
      edits.push({ start: offset, end: offset + match[0].length, text: rule.to, info: null });
    }
  }
  for (const entry of exclusions) for (const line of entry.lines ?? []) {
    if (!entry.used.has(line) && !source.split('\n').some(text => text.trim() === line.trim())) outcome.stale.push(problem(file, 1, `exclusion line no longer present: ${line}`));
  }
  let result = source;
  for (const edit of [...edits].reverse()) result = result.slice(0, edit.start) + edit.text + result.slice(edit.end);
  // Line-level view of what changed (replacements never move a line break).
  const before = source.split('\n');
  const after = result.split('\n');
  const keyed = new Map(edits.filter(edit => edit.info).map(edit => [edit.info.line, edit.info.key]));
  before.forEach((text, index) => {
    if (text === after[index]) return;
    outcome.changes.push({ rule: rule.id, file, line: index + 1, ...(keyed.has(index + 1) ? { key: keyed.get(index + 1) } : {}), before: text.replace(/\r$/, ''), after: after[index].replace(/\r$/, ''), count: edits.filter(edit => lineAt(source, edit.start) === index + 1).length });
  });
  return result;
}

export function run(options) {
  const root = path.resolve(options.root ?? '.');
  const identity = json(path.resolve(options.identity ?? path.join(root, 'tools/identity.json')));
  const rules = loadRules(path.resolve(options.rules ?? path.join(root, 'tools/rename/rules.json')), identity);
  const outcome = { changes: [], skipped: [], wholeFile: [], stale: [] };
  const written = [];
  for (const file of files(root)) {
    const absolute = path.join(root, file);
    const original = read(absolute);
    if (original.includes('\0')) continue;
    let source = original;
    for (const rule of rules) {
      if (rule.include.some(pattern => pattern.test(file)) && !rule.leave.some(pattern => pattern.test(file))) source = process_(rule, file, source, outcome);
    }
    if (source !== original) {
      if (options.apply) writeFileSync(absolute, source);
      written.push(file);
    }
  }
  return { ...outcome, written };
}

function group(file) {
  const parts = file.split('/');
  return parts[0] === 'server' && parts[1] === 'i18n' ? 'server/i18n' : parts.slice(0, parts[0] === 'desktop' ? 2 : 1).join('/') + (parts.length > 1 ? '/' : '');
}

function markdown(result, applied) {
  const out = [`# Rename report (${applied ? 'applied' : 'dry run'})`, ''];
  const total = result.changes.reduce((sum, change) => sum + change.count, 0);
  out.push(`${total} replacements on ${result.changes.length} lines in ${result.written.length} files; ${result.skipped.length} matches skipped by exclusions; ${result.wholeFile.length} files excluded as a whole.`, '');
  const bucket = (rows, key) => rows.reduce((map, row) => map.set(key(row), (map.get(key(row)) ?? 0) + (row.count ?? 1)), new Map());
  out.push('## Replacements per rule and file group', '', '| rule | group | replacements |', '|---|---|---|');
  for (const [name, count] of [...bucket(result.changes, change => `${change.rule}\t${group(change.file)}`)].sort()) out.push(`| ${name.split('\t').join(' | ')} | ${count} |`);
  out.push('', '## Replacements per file', '', '| file | replacements |', '|---|---|');
  for (const [name, count] of [...bucket(result.changes, change => change.file)].sort()) out.push(`| ${name} | ${count} |`);
  out.push('', '## Replacements', '');
  const cell = text => JSON.stringify(text.trim());
  for (const change of result.changes) {
    out.push(`- ${change.file}:${change.line} [${change.rule}]${change.key ? ' key ' + change.key : ''}`, `  - before: ${cell(change.before)}`, `  - after: ${cell(change.after)}`);
  }
  out.push('', '## Skipped matches', '');
  for (const skip of result.skipped) out.push(`- ${skip.file}:${skip.line} [${skip.rule}]${skip.key ? ' key ' + skip.key : ''}: ${cell(skip.text)} (${skip.reason})`);
  out.push('', '## Files excluded as a whole', '');
  for (const entry of result.wholeFile) out.push(`- ${entry.file} [${entry.rule}]: ${entry.reason}`);
  return out.join('\n') + '\n';
}

if (process.argv[1] && realpathSync(process.argv[1]) === realpathSync(fileURLToPath(import.meta.url))) {
  try {
    const { values } = parseArgs({ options: { help: { type: 'boolean' }, root: { type: 'string' }, rules: { type: 'string' }, identity: { type: 'string' }, report: { type: 'boolean' }, apply: { type: 'boolean' }, format: { type: 'string' } }, strict: true });
    if (values.help) console.log(usage);
    else {
      if (values.report && values.apply) throw new InputError('<arguments>:1: --report and --apply exclude each other');
      if (values.format !== undefined && !['md', 'json'].includes(values.format)) throw new InputError('<arguments>:1: --format is md or json');
      const result = run(values);
      process.stdout.write(values.format === 'json' ? JSON.stringify(result, null, 2) + '\n' : markdown(result, values.apply));
      for (const issue of result.stale) console.error(issue);
      process.exitCode = result.stale.length ? 1 : 0;
    }
  } catch (error) {
    console.error(error instanceof InputError ? error.message : problem('<arguments>', 1, error.message));
    process.exitCode = 2;
  }
}
