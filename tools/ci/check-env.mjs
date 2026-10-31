import path from 'node:path';
import { cli, files, InputError, json, keyLine, lineAt, plainObject, problem, read, stringOption } from './common.mjs';
import { rustCode } from './check-features.mjs';

const KINDS = ['runtime', 'build', 'test'];
const STATUSES = ['active', 'planned', 'reserved', 'retired'];
const ACTIONS = ['keep', 'rename', 'remove'];
const NAME = /^[A-Z][A-Z0-9_]*\*?$/;
const IDENT = '[A-Za-z_][A-Za-z0-9_]*';
// Planning identifiers must not leak into the public register (phase numbers, tracker keys, finding keys).
const INTERNAL = /\b(?:P[B]?\d+[a-z]?(?:-\d+)?|W\d+[a-z]?|[A-Z]{2,5}-\d{2,}|E-\d{4})\b/;
const SCRIPT_ROOTS = ['server/', 'desktop/main/', 'libraries/base/'];
const SCRIPT_EXTENSIONS = ['.ts', '.js', '.mjs', '.vue'];
const SKIPPED = ['tools/', '.gitea/'];

const string = value => typeof value === 'string' && value.trim() !== '';

// Validate the register and return its entries. Structure problems are input errors, never silent passes.
export function loadRegister(file) {
  const data = json(file);
  const fail = (index, message) => new InputError(problem(file, 1, `entry ${index}: ${message}`));
  if (!plainObject(data) || data.version !== 1 || !Array.isArray(data.entries)) {
    throw new InputError(problem(file, 1, 'expected {"version":1,"entries":[...]}'));
  }
  const seen = new Set();
  data.entries.forEach((entry, index) => {
    if (!plainObject(entry)) throw fail(index, 'not an object');
    const known = ['name', 'kind', 'component', 'status', 'evidence', 'standard', 'secret', 'fate', 'description', 'pattern'];
    for (const key of Object.keys(entry)) if (!known.includes(key)) throw fail(index, `unknown field ${key}`);
    if (!string(entry.name) || !NAME.test(entry.name)) throw fail(index, 'invalid name');
    if (seen.has(entry.name)) throw fail(index, `duplicate name ${entry.name}`);
    seen.add(entry.name);
    if (!KINDS.includes(entry.kind)) throw fail(index, `${entry.name}: kind must be one of ${KINDS.join(', ')}`);
    if (!string(entry.component)) throw fail(index, `${entry.name}: missing component`);
    if (!STATUSES.includes(entry.status)) throw fail(index, `${entry.name}: status must be one of ${STATUSES.join(', ')}`);
    if (entry.evidence !== 'read' && !(string(entry.evidence) && /^consumer: \S/.test(entry.evidence))) {
      throw fail(index, `${entry.name}: evidence must be "read" or "consumer: <who reads it>"`);
    }
    if (typeof entry.standard !== 'boolean' || typeof entry.secret !== 'boolean') throw fail(index, `${entry.name}: standard and secret must be booleans`);
    const fate = entry.fate;
    if (!plainObject(fate) || !ACTIONS.includes(fate.action) || !string(fate.when) ||
        Object.keys(fate).some(key => !['action', 'to', 'when'].includes(key)) ||
        (fate.action === 'rename') !== string(fate.to) || (fate.to !== undefined && !string(fate.to))) {
      throw fail(index, `${entry.name}: fate must be {action: keep|remove|rename, to (rename only), when}`);
    }
    if (!string(entry.description)) throw fail(index, `${entry.name}: missing description`);
    if (entry.pattern !== undefined) {
      const pattern = entry.pattern;
      if (!plainObject(pattern) || !string(pattern.prefix) || !string(pattern.file) || !Array.isArray(pattern.snippet) ||
          pattern.snippet.length < 2 || !pattern.snippet.every(string) || entry.name !== pattern.prefix + '*' ||
          !pattern.snippet[0].includes(pattern.prefix)) {
        throw fail(index, `${entry.name}: pattern needs prefix (the name without its trailing *), file and a snippet of at least two lines whose first line contains the prefix`);
      }
    }
  });
  return data.entries;
}

// Expand the tree after `use` into {path, alias, glob} items.
function useItems(text, file, line) {
  const list = text.match(/::|[{},*]|[A-Za-z_][A-Za-z0-9_]*|\S/g) ?? [];
  let position = 0;
  const fail = () => new InputError(problem(file, line, 'malformed use declaration'));
  const parse = prefix => {
    const items = [];
    const parts = [...prefix];
    if (list[position] === '::' && !parts.length) position++;
    for (;;) {
      const token = list[position++];
      if (token === '{') {
        for (;;) {
          items.push(...parse(parts));
          if (list[position] === ',') {
            position++;
            if (list[position] === '}') break;
          } else break;
        }
        if (list[position++] !== '}') throw fail();
        return items;
      }
      if (token === '*') {
        items.push({ path: parts, glob: true });
        return items;
      }
      if (!token || !new RegExp(`^${IDENT}$`).test(token)) throw fail();
      parts.push(token);
      if (list[position] === '::') {
        position++;
        continue;
      }
      let alias = null;
      if (list[position] === 'as') {
        position++;
        alias = list[position++];
        if (!alias) throw fail();
      }
      items.push({ path: parts, alias });
      return items;
    }
  };
  const items = parse([]);
  if (position !== list.length) throw fail();
  return items;
}

const READERS = ['var', 'var_os', 'vars', 'vars_os'];

export function rustSites(source, file) {
  const code = rustCode(source, file);
  const sites = [];
  const reject = (offset, message) => sites.push({ rejected: message, line: lineAt(source, offset) });
  let envImported = false;
  const use = /(?<![A-Za-z0-9_])use\s/g;
  let match;
  while ((match = use.exec(code))) {
    const start = use.lastIndex;
    let end = start, depth = 0;
    for (; end < code.length && (code[end] !== ';' || depth); end++) {
      if (code[end] === '{') depth++;
      if (code[end] === '}') depth--;
    }
    if (end >= code.length) throw new InputError(problem(file, lineAt(source, start), 'unterminated use declaration'));
    for (const item of useItems(code.slice(start, end), file, lineAt(source, match.index))) {
      // `{self}` in a group names the parent: use std::{self as s} aliases std, use std::env::{self} imports env.
      if (item.path.at(-1) === 'self' && item.path.length > 1) item.path = item.path.slice(0, -1);
      const [root, module, member] = item.path;
      if (root !== 'std') continue;
      if (item.path.length === 1 && (item.glob || item.alias)) reject(match.index, 'glob import or alias of std hides environment access');
      else if (module === 'env' && item.path.length === 2 && !item.glob) {
        if (item.alias) reject(match.index, 'aliased import of std::env; import the module as env');
        else envImported = true;
      } else if (module === 'env' && (item.glob || READERS.includes(member))) {
        reject(match.index, 'direct import of an environment reader; call env::var("NAME") through the env module');
      }
    }
    use.lastIndex = end;
  }
  const reader = /(?<![A-Za-z0-9_])(?:::\s*)?std\s*::\s*env\s*::\s*(var_os|var|vars_os|vars)(?![A-Za-z0-9_])|(?<![A-Za-z0-9_])(?<!::\s*)env\s*::\s*(var_os|var|vars_os|vars)(?![A-Za-z0-9_])/g;
  while ((match = reader.exec(code))) {
    const qualified = match[1] !== undefined;
    const name = match[1] ?? match[2];
    if (!qualified && !envImported) reject(match.index, 'env:: used without `use std::env;`');
    else if (name.startsWith('vars')) reject(match.index, 'enumerating all environment variables is not supported');
    else {
      const found = /^\s*\(\s*"([A-Za-z_][A-Za-z0-9_]*)"\s*\)/.exec(source.slice(reader.lastIndex));
      if (found) sites.push({ name: found[1], line: lineAt(source, match.index), read: true });
      else reject(match.index, 'environment name is not a string literal');
    }
  }
  const macro = /(?<![A-Za-z0-9_])(option_env|env)\s*!\s*[({[]/g;
  while ((match = macro.exec(code))) {
    const found = /^\s*"([A-Za-z_][A-Za-z0-9_]*)"\s*[,)\]}]/.exec(source.slice(macro.lastIndex));
    if (found) sites.push({ name: found[1], line: lineAt(source, match.index), read: true });
    else reject(match.index, `${match[1]}! needs a string literal name`);
  }
  return sites;
}

// Blank out comments (and, with strings set, string/template/regex literal text), keeping offsets and newlines.
// Template expressions `${...}` stay code. Unterminated block comments are input errors.
export function scriptMask(source, strings, file = '<source>') {
  const out = source.split('');
  const n = source.length;
  const blank = (from, to) => { for (let k = from; k < to; k++) if (source[k] !== '\n') out[k] = ' '; };
  const code = (start, inExpression) => {
    let i = start, depth = 0;
    while (i < n) {
      const c = source[i];
      if (inExpression) {
        if (c === '{') depth++;
        else if (c === '}') {
          if (!depth) return i;
          depth--;
        }
      }
      if (c === '/' && source[i + 1] === '/') {
        let end = source.indexOf('\n', i);
        if (end < 0) end = n;
        blank(i, end);
        i = end;
      } else if (c === '/' && source[i + 1] === '*') {
        const end = source.indexOf('*/', i + 2);
        if (end < 0) throw new InputError(problem(file, lineAt(source, i), 'unterminated comment'));
        blank(i, end + 2);
        i = end + 2;
      } else if (c === '"' || c === "'") {
        let j = i + 1;
        while (j < n && source[j] !== c && source[j] !== '\n') j += source[j] === '\\' ? 2 : 1;
        if (source[j] === c) {
          if (strings) blank(i + 1, j);
          i = j + 1;
        } else i = j;
      } else if (c === '`') {
        let j = i + 1, text = j, closed = false;
        const pieces = [];
        while (j < n) {
          if (source[j] === '\\') j += 2;
          else if (source[j] === '`') { closed = true; break; }
          else if (source[j] === '$' && source[j + 1] === '{') {
            pieces.push([text, j]);
            j = code(j + 2, true) + 1;
            text = j;
          } else j++;
        }
        if (!closed) throw new InputError(problem(file, lineAt(source, i), 'unterminated template literal'));
        pieces.push([text, j]);
        if (strings) for (const [from, to] of pieces) blank(from, to);
        i = j + 1;
      } else if (c === '/') {
        const before = source.slice(0, i).trimEnd();
        const previous = before.at(-1);
        // After a value (identifier, number, closing bracket, string, postfix ++ or --) a slash divides. Where the
        // context is unclear, division is assumed: that never hides code from the scanner.
        const postfix = /(?:\+\+|--)$/.test(before);
        if (!postfix && (!previous || '(,=:[!&|?{};+-*%<>~^'.includes(previous) || /\b(?:return|typeof)$/.test(before))) {
          let j = i + 1, inClass = false;
          while (j < n && source[j] !== '\n') {
            if (source[j] === '\\') j++;
            else if (source[j] === '[') inClass = true;
            else if (source[j] === ']') inClass = false;
            else if (source[j] === '/' && !inClass) break;
            j++;
          }
          if (source[j] === '/') {
            if (strings) blank(i + 1, j);
            i = j + 1;
            continue;
          }
        }
        i++;
      } else i++;
    }
    return n;
  };
  code(0, false);
  return out.join('');
}

// A Vue file has code only in its script blocks; the template cannot reach process, and its quoted attributes
// may span lines, which the script masker does not model.
export function scriptBlocks(source, file = '<source>') {
  const out = source.split('').map(char => (char === '\n' ? char : ' '));
  const opening = /<script(?=[\s>/])(?:[^>"']|"[^"]*"|'[^']*')*>/iy;
  const closing = /<\/script\s*>/ig;
  let position = 0;
  for (;;) {
    const start = source.slice(position).search(/<script(?=[\s>/])/i);
    if (start < 0) break;
    const at = position + start;
    opening.lastIndex = at;
    const tag = opening.exec(source);
    closing.lastIndex = at;
    const end = tag ? closing.exec(source) : null;
    if (!tag || !end) throw new InputError(problem(file, lineAt(source, at), 'cannot extract the script block of this Vue file'));
    const from = at + tag[0].length;
    for (let k = from; k < end.index; k++) out[k] = source[k];
    position = end.index + end[0].length;
  }
  return out.join('');
}

export function scriptSites(source, file = '<source>') {
  const sites = [];
  const reject = (offset, message) => sites.push({ rejected: message, line: lineAt(source, offset) });
  // Comments are blanked for every match; strings only where an identifier cannot be part of one.
  const text = scriptMask(source, false, file);
  const code = scriptMask(source, true, file);
  let match;
  const globals = /(?<![\w$.])(?:globalThis|global|window|self)\s*\.\s*process\b/g;
  while ((match = globals.exec(code))) reject(match.index, 'process reached through a global object');
  const imports = /['"](?:node:)?process['"]/g;
  while ((match = imports.exec(text))) reject(match.index, 'process imported as a module; use process.env.NAME');
  // `process` is only supported as process.<member> (platform, arch, cwd(), exit(), env ...).
  const identifier = /(?<![\w$])(?<![^.]\.)process(?![\w$])/g;
  while ((match = identifier.exec(code))) {
    const rest = code.slice(identifier.lastIndex);
    const member = new RegExp(`^\\s*(?:\\?\\.|\\.(?!\\.))\\s*(${IDENT})`).exec(rest);
    if (!member) reject(match.index, 'process used other than as process.<member> (alias, cast, spread, destructuring, computed access)');
    else if (member[1] === 'env') {
      const after = text.slice(identifier.lastIndex + member[0].length);
      const property = new RegExp(`^\\s*(?:\\?\\.|\\.)\\s*(${IDENT})`).exec(after);
      const indexed = new RegExp(`^\\s*(?:\\?\\.)?\\s*\\[\\s*(["'])(${IDENT})\\1\\s*\\]`).exec(after);
      const line = lineAt(source, match.index);
      if (property) sites.push({ name: property[1], line, read: true });
      else if (indexed) sites.push({ name: indexed[2], line, read: true });
      else reject(match.index, /^\s*(?:\?\.)?\s*\[/.test(after) ? 'process.env[...] needs a string literal name' : 'process.env used as a whole (alias, spread, destructuring or enumeration)');
    }
  }
  return sites;
}

// Split on whitespace outside quotes.
const words = text => text.match(/(?:[^\s"']|"(?:[^"\\]|\\.)*"|'[^']*')+/g) ?? [];

export function dockerSites(source, file) {
  const sites = [];
  const lines = source.split('\n');
  for (let index = 0; index < lines.length; index++) {
    const start = index + 1;
    let text = lines[index];
    while (/\\\s*$/.test(text) && index + 1 < lines.length) text = text.replace(/\\\s*$/, ' ') + lines[++index];
    const match = /^\s*(ARG|ENV)\s+(.*)$/i.exec(text);
    if (!match) continue;
    const kind = match[1].toUpperCase();
    const parts = words(match[2]);
    const names = kind === 'ENV' && !parts[0]?.includes('=') ? [parts[0]] : parts.map(part => part.split('=')[0]);
    if (kind === 'ENV' && parts[0]?.includes('=') && !parts.every(part => part.includes('='))) {
      throw new InputError(problem(file, start, 'unsupported ENV instruction; use NAME=value pairs'));
    }
    for (const name of names) {
      if (!name || !new RegExp(`^${IDENT}$`).test(name)) throw new InputError(problem(file, start, `unsupported ${kind} name ${name ?? ''}`));
      sites.push({ name, line: start, declared: true });
    }
  }
  return sites;
}

export function exampleSites(source, file) {
  const sites = [];
  source.split('\n').forEach((raw, index) => {
    const line = raw.replace(/\r$/, '').trim();
    if (!line) return;
    // Commented declarations (`# NAME=value`) count as declarations; other comments are free text.
    const comment = line.startsWith('#');
    const match = new RegExp(`^(?:${comment ? '#\\s*' : ''})(?:export\\s+)?(${IDENT})\\s*=`).exec(line);
    if (match) sites.push({ name: match[1], line: index + 1, declared: true });
    else if (!comment) throw new InputError(problem(file, index + 1, 'unsupported line in an environment example file; use NAME=value'));
  });
  return sites;
}

const isDockerfile = base => base === 'Dockerfile' || base.startsWith('Dockerfile.') || base.endsWith('.Dockerfile');
const skipped = relative => SKIPPED.some(prefix => relative.startsWith(prefix));

function boundBy(entries, relative, lines, line) {
  return entries.find(entry => {
    const pattern = entry.pattern;
    if (!pattern || pattern.file !== relative) return false;
    const wanted = pattern.snippet.map(item => item.trim());
    const first = line - wanted.length;
    if (first < 0) return false;
    return wanted.every((item, offset) => (lines[first + offset] ?? '').trim() === item);
  });
}

export function check(options) {
  const root = path.resolve(options.root ?? '.');
  const registerFile = options.register ?? path.join(root, 'tools/env-register.json');
  const entries = loadRegister(registerFile);
  const registerSource = read(registerFile);
  const where = name => keyLine(registerSource, name);
  const issues = [];
  const registered = new Set(entries.map(entry => entry.name));
  const reads = new Set();
  const found = [];
  const used = new Set();
  const everything = files(root);
  const text = new Map();
  const content = relative => {
    if (!text.has(relative)) text.set(relative, read(path.join(root, relative)));
    return text.get(relative);
  };
  for (const relative of everything) {
    if (skipped(relative)) continue;
    const base = path.posix.basename(relative);
    const extension = path.posix.extname(relative);
    let sites = [];
    if (extension === '.rs') sites = rustSites(content(relative), relative);
    else if (SCRIPT_EXTENSIONS.includes(extension) && SCRIPT_ROOTS.some(prefix => relative.startsWith(prefix))) sites = scriptSites(extension === '.vue' ? scriptBlocks(content(relative), relative) : content(relative), relative);
    else if (isDockerfile(base)) sites = dockerSites(content(relative), relative);
    else if (base === '.env.example' || base.endsWith('.env.example')) sites = exampleSites(content(relative), relative);
    if (!sites.length) continue;
    const lines = content(relative).split('\n');
    for (const site of sites) {
      if (site.rejected) {
        const entry = boundBy(entries, relative, lines, site.line);
        if (entry) {
          used.add(entry.name);
          reads.add(entry.name);
        } else issues.push(problem(relative, site.line, `${site.rejected}; register a pattern with this exact snippet or rewrite the access`));
      } else {
        found.push({ ...site, file: relative });
        if (site.read) reads.add(site.name);
      }
    }
  }
  for (const site of found) {
    if (!registered.has(site.name)) issues.push(problem(site.file, site.line, `${site.name}: environment name is not in the register`));
  }
  for (const entry of entries) {
    if (INTERNAL.test(JSON.stringify(entry))) issues.push(problem(registerFile, where(entry.name), `${entry.name}: the register is public; remove planning identifiers`));
    if (entry.pattern) {
      const target = entry.pattern.file;
      const exists = everything.includes(target);
      if (!exists || !used.has(entry.name)) {
        issues.push(problem(registerFile, where(entry.name), `${entry.name}: pattern exception matches no dynamic access in ${target}; the file or its snippet changed`));
      }
    }
    if (entry.status === 'active' && entry.evidence === 'read' && !reads.has(entry.name)) {
      issues.push(problem(registerFile, where(entry.name), `${entry.name}: active entry with evidence "read" has no read site; update or retire the entry`));
    }
  }
  const exact = name => registered.has(name) || [...registered].some(item => item.endsWith('*') && !name.endsWith('*') && name.startsWith(item.slice(0, -1)));
  for (const relative of everything) {
    if (relative.startsWith('tools/ci/test/')) continue;
    const source = content(relative);
    if (source.includes('\0')) continue;
    const pattern = /(?<![A-Za-z0-9_])IMPLUVIUM_[A-Z0-9_]+\*?/g;
    let match;
    while ((match = pattern.exec(source))) {
      if (!exact(match[0])) issues.push(problem(relative, lineAt(source, match.index), `${match[0]}: IMPLUVIUM_ name is not in the register`));
    }
  }
  if (options['previous-register']) {
    const previous = new Map(loadRegister(options['previous-register']).map(entry => [entry.name, entry]));
    for (const entry of entries) {
      const live = item => item && ['active', 'planned'].includes(item.status);
      if (['active', 'planned'].includes(entry.status) && !live(previous.get(entry.name)) && !entry.standard && !entry.name.startsWith('IMPLUVIUM_')) {
        issues.push(problem(registerFile, where(entry.name), `${entry.name}: new environment name without the IMPLUVIUM_ prefix and not a standard name`));
      }
    }
  }
  return issues;
}

await cli(import.meta.url, `Usage: node tools/ci/check-env.mjs [--root DIR] [--register FILE] [--previous-register FILE]
Check the register of environment names (default tools/env-register.json) against the code.
Scans process.env in server/, desktop/main/ and libraries/base/ (ts, js, mjs, vue), Rust env readers, ARG/ENV in Dockerfiles and *.env.example files.
Fails on unregistered names, unsupported access forms, active "read" entries without a read site, unregistered IMPLUVIUM_ names
and, with --previous-register (the base revision's file), new non-standard names without the IMPLUVIUM_ prefix.
Exit: 0 passed, 1 failed check, 2 usage or input error.`, {
  root: stringOption, register: stringOption, 'previous-register': stringOption,
}, check);
