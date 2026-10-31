import test from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { invoke, replace, temporary } from './helpers.mjs';

const CONF = 'desktop/src-tauri/tauri.conf.json';
const BINDINGS = [
  { value: 'desktop.identifier', file: CONF, json: '/identifier' },
  { value: 'url_scheme.name', file: CONF, json: '/plugins/deep-link/desktop/schemes/0' },
  { value: 'desktop.cargo_package', file: 'app/Cargo.toml', toml: 'package.name' },
  { value: 'data_dir.name', file: 'app/db.rs', regex: 'static PREFIX: &str = "([^"]+)";' },
  { value: 'user_agent.tokens.desktop', file: 'web/request.ts', regex: 'startsWith\\("([^"/]+)/"\\)' },
];

function write(root, file, text) {
  mkdirSync(path.dirname(path.join(root, file)), { recursive: true });
  writeFileSync(path.join(root, file), text);
}
function identity(root, change = () => {}) {
  const data = {
    schema: 1,
    desktop: { identifier: 'com.example.app', cargo_package: 'example' },
    url_scheme: { name: 'example', public_contract: 'reserved', registered_by_current_client: true },
    data_dir: { name: 'example' },
    user_agent: { tokens: { desktop: 'example-desktop' } },
    bindings: structuredClone(BINDINGS),
  };
  change(data);
  write(root, 'tools/identity.json', JSON.stringify(data, null, 2) + '\n');
}
function repo(t, change) {
  const root = temporary(t);
  write(root, CONF, '{\n  "identifier": "com.example.app",\n  "plugins": {"deep-link": {"desktop": {"schemes": ["example"]}}}\n}\n');
  write(root, 'app/Cargo.toml', '[package]\nname = "example"\nversion = "1.0.0"\n');
  write(root, 'app/db.rs', 'static PREFIX: &str = "example";\n');
  write(root, 'web/request.ts', 'const ok = agent.startsWith("example-desktop/");\n');
  identity(root, change);
  return root;
}
const args = root => ['--root', root];

test('identity bindings: every kind of selector resolving to the table value passes', t => {
  invoke('check-identity', args(repo(t)), 0);
});

test('identity bindings: a value that differs from the table is named with file and line', t => {
  const root = repo(t);
  replace(root, CONF, '"com.example.app"', '"com.example.other"');
  assert.match(invoke('check-identity', args(root), 1).stderr, /tauri\.conf\.json:2: desktop\.identifier: found "com\.example\.other"/);
  const second = repo(t);
  replace(second, 'app/Cargo.toml', 'name = "example"', 'name = "renamed"');
  assert.match(invoke('check-identity', args(second), 1).stderr, /Cargo\.toml:2: desktop\.cargo_package: found "renamed"/);
  const third = repo(t);
  replace(third, 'app/db.rs', '"example"', '"example-debug"');
  assert.match(invoke('check-identity', args(third), 1).stderr, /db\.rs:1: data_dir\.name: found "example-debug"/);
});

test('identity bindings: a selector that does not resolve is an error', t => {
  const json = repo(t);
  replace(json, CONF, '"identifier"', '"identifiers"');
  assert.match(invoke('check-identity', args(json), 1).stderr, /JSON pointer \/identifier does not resolve/);
  const array = repo(t);
  replace(array, CONF, '["example"]', '[]');
  assert.match(invoke('check-identity', args(array), 1).stderr, /schemes\/0 does not resolve/);
  const key = repo(t);
  replace(key, 'app/Cargo.toml', '[package]\nname = "example"', '[package]\ntitle = "example"');
  assert.match(invoke('check-identity', args(key), 1).stderr, /TOML key package\.name resolves 0 times/);
  const regex = repo(t);
  replace(regex, 'web/request.ts', 'startsWith', 'endsWith');
  assert.match(invoke('check-identity', args(regex), 1).stderr, /matches 0 times, expected exactly once/);
});

test('identity bindings: a selector that matches twice is an error', t => {
  const root = repo(t);
  write(root, 'app/db.rs', 'static PREFIX: &str = "example";\nstatic PREFIX: &str = "example";\n');
  assert.match(invoke('check-identity', args(root), 1).stderr, /db\.rs:1: data_dir\.name: regular expression matches 2 times/);
});

test('identity bindings: a binding whose file is gone fails and names the binding', t => {
  const gone = repo(t, data => { data.bindings[3].file = 'app/missing.rs'; });
  assert.match(invoke('check-identity', args(gone), 1).stderr, /identity\.json:\d+: data_dir\.name: app\/missing\.rs does not exist/);
});

test('identity bindings: a value path that is not a string in the table is an error', t => {
  const root = repo(t, data => { data.bindings[0].value = 'desktop.nothing'; });
  assert.match(invoke('check-identity', args(root), 1).stderr, /desktop\.nothing: not a string value/);
  const object = repo(t, data => { data.bindings[0].value = 'desktop'; });
  assert.match(invoke('check-identity', args(object), 1).stderr, /desktop: not a string value/);
});

test('identity bindings: a regular expression needs exactly one capture group', t => {
  const none = repo(t, data => { data.bindings[3].regex = 'static PREFIX: &str = "[^"]+";'; });
  assert.match(invoke('check-identity', args(none), 2).stderr, /exactly one capture group/);
  const two = repo(t, data => { data.bindings[3].regex = 'static (PREFIX): &str = "([^"]+)";'; });
  assert.match(invoke('check-identity', args(two), 2).stderr, /exactly one capture group/);
  const invalid = repo(t, data => { data.bindings[3].regex = '(['; });
  assert.match(invoke('check-identity', args(invalid), 2).stderr, /invalid regular expression/);
});

test('identity bindings: a malformed binding is an input error', t => {
  for (const change of [
    data => { delete data.bindings[0].json; },
    data => { data.bindings[0].toml = 'package.name'; },
    data => { data.bindings[0].file = '../outside.json'; },
    data => { data.bindings[0].file = '/etc/passwd'; },
    data => { data.bindings[0].extra = 1; },
    data => { data.bindings[0] = 'text'; },
    data => { data.bindings[0].value = ''; },
  ]) invoke('check-identity', args(repo(t, change)), 2);
  invoke('check-identity', args(repo(t, data => { data.bindings = {}; })), 2);
});

test('identity bindings: unreadable or invalid inputs are input errors', t => {
  const root = repo(t);
  write(root, CONF, '{');
  assert.match(invoke('check-identity', args(root), 2).stderr, /tauri\.conf\.json:1: invalid JSON/);
  const table = repo(t);
  write(table, 'tools/identity.json', '[]');
  invoke('check-identity', args(table), 2);
  invoke('check-identity', args(temporary(t)), 2);
  const toml = repo(t);
  write(toml, 'app/Cargo.toml', '[package\nname = ');
  invoke('check-identity', args(toml), 2);
});

test('identity bindings: a registered scheme must be bound in tauri.conf.json', t => {
  const root = repo(t, data => { data.bindings.splice(1, 1); });
  assert.match(invoke('check-identity', args(root), 1).stderr, /registered_by_current_client is true: url_scheme\.name must be bound to an entry of \/plugins\/deep-link\/desktop\/schemes in desktop\/src-tauri\/tauri\.conf\.json/);
  const elsewhere = repo(t, data => { data.bindings[1].file = 'web/request.ts'; data.bindings[1].json = undefined; data.bindings[1].regex = 'startsWith\\("([^"-]+)'; });
  assert.match(invoke('check-identity', args(elsewhere), 1).stderr, /must be bound to an entry of/);
  const flag = repo(t, data => { data.url_scheme.registered_by_current_client = 'yes'; });
  invoke('check-identity', args(flag), 2);
  const missing = repo(t, data => { delete data.url_scheme; });
  invoke('check-identity', args(missing), 2);
});

test('identity bindings: the scheme binding must select the deep-link scheme entry itself', t => {
  const wrong = repo(t, data => {
    data.bindings[1].json = '/identifier';
    data.desktop.identifier = 'example';
  });
  assert.match(invoke('check-identity', args(wrong), 1).stderr, /must be bound to an entry of/);
  const removed = repo(t);
  replace(removed, CONF, '["example"]', '["other"]');
  assert.match(invoke('check-identity', args(removed), 1).stderr, /schemes\/0: found "other"|must list the scheme "example"/);
  const gone = repo(t, data => { data.bindings.splice(1, 1); });
  replace(gone, CONF, '["example"]', '[]');
  assert.match(invoke('check-identity', args(gone), 1).stderr, /must list the scheme "example"/);
});

test('identity bindings: an unregistered scheme must not appear in tauri.conf.json', t => {
  const clean = repo(t, data => {
    data.url_scheme.registered_by_current_client = false;
    data.bindings.splice(1, 1);
  });
  replace(clean, CONF, ',\n  "plugins": {"deep-link": {"desktop": {"schemes": ["example"]}}}', '');
  invoke('check-identity', args(clean), 0);
  const empty = repo(t, data => {
    data.url_scheme.registered_by_current_client = false;
    data.bindings.splice(1, 1);
  });
  replace(empty, CONF, '["example"]', '[]');
  invoke('check-identity', args(empty), 0);
  const still = repo(t, data => {
    data.url_scheme.registered_by_current_client = false;
    data.bindings.splice(1, 1);
  });
  assert.match(invoke('check-identity', args(still), 1).stderr, /registered_by_current_client is false: the file must name no URL scheme/);
});

test('identity bindings: the check never writes', t => {
  const root = repo(t, data => { data.bindings[0].value = 'desktop.nothing'; });
  const before = readFileSync(path.join(root, 'tools/identity.json'), 'utf8');
  invoke('check-identity', args(root), 1);
  assert.equal(readFileSync(path.join(root, 'tools/identity.json'), 'utf8'), before);
});
