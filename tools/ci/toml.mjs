import { InputError, lineAt, problem, read } from './common.mjs';

// A small data-only TOML reader: strings, scalars, arrays, inline tables and
// dotted keys. It retains source lines and never invokes Cargo to read manifests.
const bareValue = new RegExp('^(?:' + [
  '[+-]?(?:0|[1-9](?:_?\\d)*)(?:[eE][+-]?\\d(?:_?\\d)*)?',
  '0x[0-9A-Fa-f](?:_?[0-9A-Fa-f])*', '0o[0-7](?:_?[0-7])*', '0b[01](?:_?[01])*',
  '[+-]?(?:inf|nan)',
  '\\d{4}-\\d{2}-\\d{2}(?:[Tt]\\d{2}:\\d{2}:\\d{2}(?:[Zz]|[+-]\\d{2}:\\d{2})?)?',
  '\\d{2}:\\d{2}:\\d{2}',
].join('|') + ')$');

export function toml(file) {
  const source = read(file);
  const tokens = [];
  const pattern = /\s+|#[^\n]*|"""[\s\S]*?"""|'''[\s\S]*?'''|"(?:\\.|[^"\\])*"|'[^']*'|[\[\]{},=.]|[^\s#[\]{}=,.'"]+/gy;
  let offset = 0;
  while (offset < source.length) {
    pattern.lastIndex = offset;
    const match = pattern.exec(source);
    if (!match) throw new InputError(problem(file, lineAt(source, offset), 'unsupported or malformed TOML'));
    offset = pattern.lastIndex;
    if (!/^\s|^#/.test(match[0])) tokens.push({ text: match[0], line: lineAt(source, match.index) });
  }
  let index = 0;
  const peek = () => tokens[index]?.text;
  const fail = () => { throw new InputError(problem(file, tokens[index]?.line ?? 1, 'unsupported or malformed TOML')); };
  const take = expected => {
    const token = tokens[index++];
    if (!token || (expected && token.text !== expected)) fail();
    return token;
  };
  const scalar = text => {
    if (text.startsWith('"""') || text.startsWith("'''")) return text.slice(3, -3);
    if (text.startsWith('"')) {
      try { return JSON.parse(text); } catch { fail(); }
    }
    if (text.startsWith("'")) return text.slice(1, -1);
    if (text === 'true' || text === 'false') return text === 'true';
    // Other bare values must be TOML integers, exponent floats, inf/nan or dates and times; they stay
    // text. Anything else, such as an unquoted word, is malformed. Floats with a dot are unsupported.
    if (bareValue.test(text)) return text;
    fail();
  };
  const keyPart = () => {
    const text = take().text;
    if (text.startsWith('"') || text.startsWith("'")) return String(scalar(text));
    if (!/^[A-Za-z0-9_-]+$/.test(text)) fail();
    return text;
  };
  const key = () => {
    const parts = [keyPart()];
    while (peek() === '.') { take('.'); parts.push(keyPart()); }
    return parts;
  };
  const put = (object, parts, value) => {
    for (const part of parts.slice(0, -1)) {
      object = object[part] ??= Object.create(null);
      if (!object || typeof object !== 'object') fail();
    }
    if (Object.hasOwn(object, parts.at(-1))) fail();
    object[parts.at(-1)] = value;
  };
  const value = () => {
    if (peek() === '[') {
      take('[');
      const array = [];
      while (peek() !== ']') {
        array.push(value());
        if (peek() !== ']') take(',');
      }
      take(']');
      return array;
    }
    if (peek() === '{') {
      take('{');
      const object = Object.create(null);
      while (peek() !== '}') {
        const parts = key();
        take('=');
        put(object, parts, value());
        if (peek() !== '}') take(',');
      }
      take('}');
      return object;
    }
    return scalar(take().text);
  };
  const data = Object.create(null);
  const entries = [];
  let section = [];
  let current = data;
  const tables = new Set();
  while (index < tokens.length) {
    if (peek() === '[') {
      take('[');
      const array = peek() === '[';
      if (array) take('[');
      section = key();
      take(']');
      if (array) take(']');
      const identity = JSON.stringify(section);
      if (!array && tables.has(identity)) fail();
      if (!array) tables.add(identity);
      current = data;
      for (const part of section.slice(0, -1)) current = current[part] ??= Object.create(null);
      const last = section.at(-1);
      if (array) {
        const list = current[last] ??= [];
        const item = Object.create(null);
        list.push(item);
        current = item;
      } else current = current[last] ??= Object.create(null);
    } else {
      const line = tokens[index].line;
      const parts = key();
      take('=');
      const item = value();
      put(current, parts, item);
      entries.push({ keys: [...section, ...parts], value: item, line });
    }
  }
  return { data, entries, source, file };
}
