import { existsSync } from 'node:fs';
import path from 'node:path';
import { toml } from './toml.mjs';
import { cli, InputError, json, keyLine, lineAt, plainObject, problem, read, stringOption } from './common.mjs';

const SCHEME_FILE = 'desktop/src-tauri/tauri.conf.json';
const SCHEME_VALUE = 'url_scheme.name';
const SCHEMES_POINTER = '/plugins/deep-link/desktop/schemes';
const SELECTORS = ['json', 'toml', 'regex'];

const relativePath = name => typeof name === 'string' && name && !path.posix.isAbsolute(name) && !name.includes('\\') &&
  path.posix.normalize(name) === name && !name.startsWith('../');

// Value of a dotted path in the identity table, or undefined.
function tableValue(identity, dotted) {
  let value = identity;
  for (const part of dotted.split('.')) {
    if (!plainObject(value) || !Object.hasOwn(value, part)) return undefined;
    value = value[part];
  }
  return value;
}

function pointerValue(document, pointer) {
  if (pointer === '') return document;
  if (!pointer.startsWith('/')) return undefined;
  let value = document;
  for (const raw of pointer.slice(1).split('/')) {
    const part = raw.replaceAll('~1', '/').replaceAll('~0', '~');
    if (Array.isArray(value) && /^(?:0|[1-9]\d*)$/.test(part)) value = value[Number(part)];
    else if (plainObject(value) && Object.hasOwn(value, part)) value = value[part];
    else return undefined;
  }
  return value;
}

function bindingLine(source, binding) {
  const selector = JSON.stringify(binding[SELECTORS.find(key => binding[key] !== undefined)] ?? '');
  const lines = source.split('\n');
  const index = lines.findIndex(line => line.includes(JSON.stringify(binding.file ?? '')) && line.includes(selector));
  return index === -1 ? keyLine(source, 'bindings') : index + 1;
}

function load(root, relative, cache) {
  if (!cache.has(relative)) cache.set(relative, read(path.join(root, relative)));
  return cache.get(relative);
}

// Resolves one binding to { found, line } or { error, line }. Never executes or builds anything.
function resolve(root, binding, cache) {
  const file = binding.file;
  const text = load(root, file, cache);
  if (binding.json !== undefined) {
    let document;
    try {
      document = JSON.parse(text);
    } catch {
      throw new InputError(problem(file, 1, 'invalid JSON'));
    }
    const value = pointerValue(document, binding.json);
    if (value === undefined) return { error: `JSON pointer ${binding.json} does not resolve` };
    if (typeof value !== 'string') return { error: `JSON pointer ${binding.json} does not point to a string` };
    const last = binding.json.split('/').at(-1);
    const at = text.indexOf(/^\d+$/.test(last) ? JSON.stringify(value) : JSON.stringify(last));
    return { found: value, line: at === -1 ? 1 : lineAt(text, at) };
  }
  if (binding.toml !== undefined) {
    const parsed = toml(path.join(root, file));
    const keys = binding.toml.split('.');
    const matches = parsed.entries.filter(entry => entry.keys.length === keys.length && entry.keys.every((key, index) => key === keys[index]));
    if (matches.length !== 1) return { error: `TOML key ${binding.toml} resolves ${matches.length} times, expected exactly once` };
    if (typeof matches[0].value !== 'string') return { error: `TOML key ${binding.toml} is not a string`, line: matches[0].line };
    return { found: matches[0].value, line: matches[0].line };
  }
  let expression;
  try {
    expression = new RegExp(binding.regex, 'gm');
  } catch {
    throw new InputError(problem('tools/identity.json', 1, `${file}: invalid regular expression`));
  }
  if (new RegExp(`${binding.regex}|`).exec('').length !== 2) {
    throw new InputError(problem('tools/identity.json', 1, `${file}: the regular expression must have exactly one capture group`));
  }
  const matches = [...text.matchAll(expression)];
  if (matches.length !== 1) {
    return { error: `regular expression matches ${matches.length} times, expected exactly once`, line: matches[0] ? lineAt(text, matches[0].index) : 1 };
  }
  if (matches[0][1] === undefined) return { error: 'the capture group did not take part in the match', line: lineAt(text, matches[0].index) };
  return { found: matches[0][1], line: lineAt(text, matches[0].index + matches[0][0].indexOf(matches[0][1])) };
}

function validate(binding, index, identitySource) {
  const where = problem('tools/identity.json', keyLine(identitySource, 'bindings'), `binding ${index}`);
  const selectors = SELECTORS.filter(key => binding?.[key] !== undefined);
  if (!plainObject(binding) || typeof binding.value !== 'string' || !binding.value || !relativePath(binding.file) ||
      selectors.length !== 1 || typeof binding[selectors[0]] !== 'string' ||
      Object.keys(binding).some(key => !['value', 'file', ...SELECTORS].includes(key))) {
    throw new InputError(`${where}: expected {"value": "<table path>", "file": "<relative path>", one of "json" | "toml" | "regex"}`);
  }
}

export function check(options) {
  const root = path.resolve(options.root ?? '.');
  const identityFile = path.join(root, 'tools/identity.json');
  const identity = json(identityFile);
  const identitySource = read(identityFile);
  if (!plainObject(identity) || !Array.isArray(identity.bindings)) {
    throw new InputError(problem('tools/identity.json', 1, 'expected an object with a "bindings" array'));
  }
  const registered = tableValue(identity, 'url_scheme.registered_by_current_client');
  if (typeof registered !== 'boolean') {
    throw new InputError(problem('tools/identity.json', keyLine(identitySource, 'url_scheme'), 'url_scheme.registered_by_current_client must be true or false'));
  }
  const issues = [];
  const cache = new Map();
  identity.bindings.forEach((binding, index) => validate(binding, index, identitySource));
  for (const binding of identity.bindings) {
    const line = bindingLine(identitySource, binding);
    const expected = tableValue(identity, binding.value);
    if (typeof expected !== 'string') {
      issues.push(problem('tools/identity.json', line, `${binding.value}: not a string value in the identity table`));
      continue;
    }
    if (!existsSync(path.join(root, binding.file))) {
      issues.push(problem('tools/identity.json', line, `${binding.value}: ${binding.file} does not exist; move the binding with the code`));
      continue;
    }
    const result = resolve(root, binding, cache);
    if (result.error) issues.push(problem(binding.file, result.line ?? 1, `${binding.value}: ${result.error}`));
    else if (result.found !== expected) {
      issues.push(problem(binding.file, result.line, `${binding.value}: found "${result.found}", the identity table says "${expected}"`));
    }
  }
  let document;
  try {
    document = JSON.parse(load(root, SCHEME_FILE, cache));
  } catch (error) {
    if (error instanceof InputError) throw error;
    throw new InputError(problem(SCHEME_FILE, 1, 'invalid JSON'));
  }
  const schemes = pointerValue(document, SCHEMES_POINTER);
  if (registered) {
    const bound = identity.bindings.some(binding => binding.value === SCHEME_VALUE && binding.file === SCHEME_FILE &&
      typeof binding.json === 'string' && new RegExp(`^${SCHEMES_POINTER}/(?:0|[1-9]\\d*)$`).test(binding.json));
    if (!bound) {
      issues.push(problem('tools/identity.json', keyLine(identitySource, 'registered_by_current_client'),
        `registered_by_current_client is true: ${SCHEME_VALUE} must be bound to an entry of ${SCHEMES_POINTER} in ${SCHEME_FILE}`));
    }
    if (!Array.isArray(schemes) || !schemes.includes(tableValue(identity, SCHEME_VALUE))) {
      issues.push(problem(SCHEME_FILE, 1, `registered_by_current_client is true: ${SCHEMES_POINTER} must list the scheme "${tableValue(identity, SCHEME_VALUE)}"`));
    }
  } else if (schemes !== undefined && !(Array.isArray(schemes) && schemes.length === 0)) {
    issues.push(problem(SCHEME_FILE, 1, 'registered_by_current_client is false: the file must name no URL scheme'));
  }
  return issues;
}

await cli(import.meta.url, `Usage: node tools/ci/check-identity.mjs [--root DIR]
Check every binding of tools/identity.json: the file exists, the selector (JSON pointer, TOML key or regular expression with one
capture group) resolves exactly once, and the value equals the entry of the identity table named by "value". With
url_scheme.registered_by_current_client true the URL scheme must be bound to an entry of ${SCHEMES_POINTER} in ${SCHEME_FILE} and listed there; with false that file must name no scheme.
Read-only. Exit: 0 passed, 1 mismatch, 2 usage or input error.`, {
  root: stringOption,
}, check);
