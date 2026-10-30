import { InputError, problem, read } from './common.mjs';

// Workflow YAML deliberately uses a small, explicit subset. Reject aliases,
// tags and merge keys rather than checking a different document from the runner.
export function yaml(file) {
  const lines = read(file).replaceAll('\r', '').split('\n');
  let index = 0;
  const fail = (line, message = 'unsupported or malformed workflow YAML') => {
    throw new InputError(problem(file, line, message));
  };
  const strip = text => {
    let quote = '';
    for (let i = 0; i < text.length; i++) {
      if (quote === '"' && text[i] === '\\') { i++; continue; }
      if (text[i] === quote) {
        if (quote === "'" && text[i + 1] === "'") { i++; continue; }
        quote = '';
      } else if (!quote && /['"]/.test(text[i])) quote = text[i];
      else if (!quote && text[i] === '#' && (i === 0 || /\s/.test(text[i - 1]))) return text.slice(0, i).trimEnd();
    }
    return text.trimEnd();
  };
  const split = (text, separator) => {
    const parts = [];
    let quote = '', depth = 0, start = 0;
    for (let i = 0; i < text.length; i++) {
      const char = text[i];
      if (quote) {
        if (quote === '"' && char === '\\') i++;
        else if (char === quote) {
          if (quote === "'" && text[i + 1] === "'") i++;
          else quote = '';
        }
      } else if (/['"]/.test(char)) quote = char;
      else if (char === '[' || char === '{') depth++;
      else if (char === ']' || char === '}') depth--;
      else if (depth === 0 && char === separator &&
          (separator !== ':' || i === text.length - 1 || /\s/.test(text[i + 1]))) {
        parts.push(text.slice(start, i)); start = i + 1;
        if (separator === ':') break;
      }
    }
    parts.push(text.slice(start));
    return parts;
  };
  const scalar = (text, line) => {
    text = text.trim();
    const node = { line, value: text };
    if (/^[&*!]|^<<\s*:/.test(text)) fail(line, 'YAML aliases, tags and merge keys are not supported');
    if (text.startsWith('[') || text.startsWith('{')) {
      const map = text[0] === '{';
      if (!text.endsWith(map ? '}' : ']')) fail(line);
      const items = split(text.slice(1, -1), ',').filter(item => item.trim());
      node.value = map ? Object.create(null) : items.map(item => scalar(item, line));
      if (map) for (const item of items) add(node.value, item, line);
    } else if (text.startsWith("'")) {
      if (!/^'(?:[^']|'')*'$/.test(text)) fail(line);
      node.value = text.slice(1, -1).replaceAll("''", "'");
    } else if (text.startsWith('"')) {
      try { node.value = JSON.parse(text); } catch { fail(line); }
    }
    return node;
  };
  const add = (map, text, line, nested) => {
    const parts = split(text, ':');
    if (parts.length !== 2) fail(line);
    const key = scalar(parts[0], line).value;
    if (typeof key !== 'string' || key === '<<' || Object.hasOwn(map, key)) fail(line, `invalid or duplicate YAML key ${key}`);
    map[key] = nested ? nested(parts[1].trim(), line) : scalar(parts[1], line);
  };
  const significant = () => {
    while (index < lines.length && !strip(lines[index]).trim()) index++;
    if (index < lines.length && /^\s*\t/.test(lines[index])) fail(index + 1, 'tabs in YAML indentation');
  };
  const indent = text => text.length - text.trimStart().length;
  const block = level => {
    significant();
    const sequence = /^-($|\s)/.test(lines[index]?.trimStart() ?? '');
    const node = { line: index + 1, value: sequence ? [] : Object.create(null) };
    const nested = (text, line) => {
      if (/^[|>][+-]?$/.test(text)) {
        const start = index;
        while (index < lines.length && (!lines[index].trim() || indent(lines[index]) > level)) index++;
        const content = lines.slice(start, index);
        const widths = content.filter(item => item.trim()).map(indent);
        const width = widths.length ? Math.min(...widths) : level + 2;
        const chunks = content.map(item => item.slice(width));
        if (text[0] === '>') {
          while (chunks.length && !chunks.at(-1).trim()) chunks.pop();
          if (chunks.some(item => !item.trim() || /^\s/.test(item))) {
            fail(start + 1, 'use literal | blocks for blank or more-indented shell lines');
          }
        }
        return { line: start + 1, value: chunks.join(text[0] === '>' ? ' ' : '\n'), block: true };
      }
      if (/^[|>]/.test(text)) fail(line, 'unsupported YAML block scalar indicator');
      if (!text) {
        significant();
        if (index < lines.length && indent(lines[index]) > level) return block(indent(lines[index]));
        return { line, value: Object.create(null) };
      }
      // Quoted and plain continuation lines are folded by YAML before execution.
      while (index < lines.length && lines[index].trim() && indent(lines[index]) > level &&
          !/^[\[{]/.test(text)) {
        text += ' ' + strip(lines[index++]).trim();
      }
      return scalar(strip(text), line);
    };
    while (index < lines.length) {
      significant();
      if (index >= lines.length || indent(lines[index]) < level) break;
      if (indent(lines[index]) !== level) fail(index + 1);
      let text = strip(lines[index]).trim();
      const line = ++index;
      if (sequence) {
        if (!/^-($|\s)/.test(text)) fail(line);
        text = text.slice(1).trim();
        if (!text) node.value.push(nested('', line));
        else if (split(text, ':').length === 2) {
          const item = { line, value: Object.create(null) };
          // The first key in a sequence mapping shares its indentation with
          // subsequent keys, not with the dash.
          const tail = blockMappingEntry(text, line, level + 2);
          Object.assign(item.value, tail);
          significant();
          if (index < lines.length && indent(lines[index]) > level) {
            const rest = block(level + 2);
            if (Array.isArray(rest.value)) fail(rest.line);
            for (const [key, value] of Object.entries(rest.value)) {
              if (Object.hasOwn(item.value, key)) fail(value.line, `duplicate YAML key ${key}`);
              item.value[key] = value;
            }
          }
          node.value.push(item);
        } else node.value.push(scalar(text, line));
      } else add(node.value, text, line, nested);
    }
    return node;
  };
  const blockMappingEntry = (text, line, level) => {
    // Parse a sequence's first mapping key using the ordinary block parser.
    const original = lines[line - 1];
    lines[line - 1] = ' '.repeat(level) + text;
    index = line - 1;
    const result = block(level).value;
    lines[line - 1] = original;
    return result;
  };
  significant();
  const result = block(index < lines.length ? indent(lines[index]) : 0);
  significant();
  if (index < lines.length) fail(index + 1);
  return result;
}

export function walkYaml(node, visit, keys = []) {
  visit(node, keys);
  if (node.value && typeof node.value === 'object') {
    for (const [key, child] of Object.entries(node.value)) walkYaml(child, visit, [...keys, key]);
  }
}
