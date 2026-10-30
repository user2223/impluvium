import path from 'node:path';
import { baseline, cli, files, InputError, keyLine, lineAt, problem, read, stringOption } from './common.mjs';
import { manifests } from './cargo.mjs';

// Mask comments and string/character literals, preserving offsets and newlines.
// Rust raw strings and nested block comments must not create phantom attributes.
export function rustCode(source, file) {
  const mask = text => text.replace(/[^\n]/g, ' ');
  const pattern = /\/\/[^\n]*|\/\*|(?:br|cr|r)(#*)"|(?:b|c)?"|'(?:\\(?:u\{[0-9a-fA-F]+\}|x[0-9a-fA-F]{2}|.)|[^'\\\n])'/g;
  let output = '', end = 0, match;
  while ((match = pattern.exec(source))) {
    const start = match.index;
    let next = pattern.lastIndex;
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
    } else if (match[0].endsWith('"')) {
      while (next < source.length && source[next] !== '"') {
        if (source[next] === '\\') next++;
        next++;
      }
      if (next >= source.length) throw new InputError(problem(file, lineAt(source, start), 'unterminated Rust string'));
      next++;
    }
    output += source.slice(end, start) + mask(source.slice(start, next));
    end = next; pattern.lastIndex = next;
  }
  return output + source.slice(end);
}

export function gates(source, file) {
  const code = rustCode(source, file);
  const result = [];
  const parseMeta = (text, offset) => {
    const match = /^\s*(feature|cfg_attr)\s*\(([\s\S]*)\)\s*$/.exec(text);
    if (!match) return;
    const body = match[2];
    const bodyStart = offset + text.indexOf('(') + 1;
    const parts = [];
    let depth = 0, start = 0;
    for (let i = 0; i <= body.length; i++) {
      if ('([{'.includes(body[i] ?? '\0')) depth++;
      else if (')]}'.includes(body[i] ?? '\0')) depth--;
      if (i === body.length || (body[i] === ',' && depth === 0)) {
        parts.push({ text: body.slice(start, i), offset: bodyStart + start }); start = i + 1;
      }
    }
    if (match[1] === 'cfg_attr') {
      for (const part of parts.slice(1)) parseMeta(part.text, part.offset);
    } else for (const part of parts) {
      const name = part.text.trim();
      if (!name) continue;
      if (!/^[A-Za-z_][A-Za-z_0-9]*$/.test(name)) throw new InputError(problem(file, lineAt(code, part.offset), 'malformed feature gate'));
      result.push({ name, line: lineAt(code, part.offset + part.text.search(/\S/)) });
    }
  };
  const attribute = /#\s*!?\s*\[/g;
  let match;
  while ((match = attribute.exec(code))) {
    const start = attribute.lastIndex;
    let end = start, depth = 1;
    for (; end < code.length && depth; end++) {
      if (code[end] === '[') depth++;
      if (code[end] === ']') depth--;
    }
    if (depth) throw new InputError(problem(file, lineAt(code, start), 'unterminated Rust attribute'));
    parseMeta(code.slice(start, end - 1), start);
    attribute.lastIndex = end;
  }
  return result;
}

function allowlist(file) {
  const entries = baseline(file, 'crates', value => Array.isArray(value) && value.every(gate => typeof gate === 'string' && /^[A-Za-z_]\w*$/.test(gate)) && new Set(value).size === value.length, true);
  if (!Object.keys(entries).every(key => !path.posix.isAbsolute(key) && !key.includes('\\') &&
      path.posix.normalize(key) === key && !key.startsWith('../') &&
      (key === 'Cargo.toml' || key.endsWith('/Cargo.toml')))) {
    throw new InputError(problem(file, 1, 'crate keys must be repository-relative Cargo.toml paths'));
  }
  return entries;
}

export function check(options) {
  const root = path.resolve(options.root ?? '.');
  const file = options.baseline ?? path.join(root, 'tools/ci/feature-gates.json');
  const allowed = allowlist(file);
  const issues = [];
  if (options['previous-baseline']) {
    const previous = allowlist(options['previous-baseline']);
    for (const [crate, names] of Object.entries(allowed)) for (const name of names) {
      if (!previous[crate]?.includes(name)) issues.push(problem(file, keyLine(read(file), crate), `${crate}: allow-list grew with gate ${name}`));
    }
  }
  const crates = manifests(root).filter(item => item.data.package).sort((a, b) => b.directory.length - a.directory.length);
  const seen = new Map();
  for (const relative of files(root).filter(item => item.endsWith('.rs'))) {
    const owner = crates.find(item => item.directory === '.' || relative.startsWith(item.directory + '/'));
    const absolute = path.join(root, relative);
    for (const gate of gates(read(absolute), relative)) {
      const crate = owner?.relative;
      if (!crate || !allowed[crate]?.includes(gate.name)) issues.push(problem(relative, gate.line, `${crate ?? 'unowned Rust source'}: feature gate ${gate.name} is not allowed`));
      if (crate) {
        if (!seen.has(crate)) seen.set(crate, new Set());
        seen.get(crate).add(gate.name);
      }
    }
  }
  for (const [crate, names] of Object.entries(allowed)) {
    const unused = names.filter(name => !seen.get(crate)?.has(name));
    if (unused.length) console.log(problem(file, keyLine(read(file), crate), `${crate}: remove unused gates from the allow-list: ${unused.join(', ')}`));
  }
  return issues;
}

await cli(import.meta.url, `Usage: node tools/ci/check-features.mjs [--root DIR] [--baseline FILE] [--previous-baseline FILE]
Check feature gate names per owning Cargo package, including nested cfg_attr attributes.
Default baseline: tools/ci/feature-gates.json; format {"version":1,"crates":{"path/Cargo.toml":["gate_name"]}}.
Unlisted crates allow no gates. Pass the base revision's allow-list as --previous-baseline to reject baseline growth.
Exit: 0 passed, 1 unapproved gate/baseline growth, 2 usage or input error.`, {
  root: stringOption, baseline: stringOption, 'previous-baseline': stringOption,
}, check);
