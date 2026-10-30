import test from 'node:test';
import assert from 'node:assert/strict';
import { writeFileSync, appendFileSync } from 'node:fs';
import path from 'node:path';
import { invoke, replace, repository } from './helpers.mjs';

test('licence boundary: workspace inheritance and MPL outside engine are allowed', t => {
  invoke('check-licence-boundary', ['--root', repository(t)], 0);
});

test('licence boundary: MPL expressions and inherited MPL licences fail', t => {
  for (const licence of ['MPL-2.0', 'MIT OR MPL-2.0', '(MPL-2.0 AND Apache-2.0)']) {
    const root = repository(t);
    replace(root, 'Cargo.toml', 'license = "Apache-2.0"', `license = '${licence}'`);
    assert.match(invoke('check-licence-boundary', ['--root', root], 1).stderr, /MPL-licensed crate/);
  }
});

test('licence boundary: licence-file text cannot hide MPL', t => {
  const root = repository(t);
  replace(root, 'engine/core/Cargo.toml', 'license.workspace = true', 'license-file = "COPYING.txt"');
  writeFileSync(path.join(root, 'engine/core/COPYING.txt'), 'Mozilla Public License Version 2.0\n');
  assert.match(invoke('check-licence-boundary', ['--root', root], 1).stderr, /COPYING.txt:1:/);
});

const forms = [
  '[dependencies]\nrenamed = { package = "service", path = "../../server/crates/service", optional = true }',
  '[build-dependencies.renamed]\npackage = "service"\npath = "../../server/crates/service"',
  '[target.\'cfg(target_os = "linux")\'.dev-dependencies]\nrenamed = {\n package = "service",\n path = "../../server/crates/service"\n}',
];
for (const form of forms) test(`licence boundary rejects ${form.split('\n')[0]}`, t => {
  const root = repository(t);
  appendFileSync(path.join(root, 'engine/core/Cargo.toml'), `\n${form.replace('[dependencies]\n', '')}\n`);
  // The fixture's last section is already [dependencies]. Other table forms
  // switch sections explicitly, and the direct form just adds an entry.
  assert.match(invoke('check-licence-boundary', ['--root', root], 1).stderr, /crosses into server\/crates/);
});

test('licence boundary: a dependency inherited from the workspace cannot cross', t => {
  const root = repository(t);
  replace(root, 'Cargo.toml', 'path = "shared"', 'path = "server/crates/service"');
  assert.match(invoke('check-licence-boundary', ['--root', root], 1).stderr, /crosses into server\/crates/);
});

test('licence boundary follows transitive dependencies and terminates cycles', t => {
  const root = repository(t);
  appendFileSync(path.join(root, 'shared/Cargo.toml'), '\n[dependencies]\ncycle = { path = "../engine/core" }\nservice = { path = "../server/crates/service" }\n');
  assert.match(invoke('check-licence-boundary', ['--root', root], 1).stderr, /shared\/Cargo.toml -> server\/crates/);
});

test('licence boundary follows registry patches', t => {
  const root = repository(t);
  appendFileSync(path.join(root, 'Cargo.toml'), '\n[patch.crates-io]\nservice = { path = "server/crates/service" }\n');
  appendFileSync(path.join(root, 'engine/core/Cargo.toml'), '\nservice = "1.0.0"\n');
  invoke('check-licence-boundary', ['--root', root], 1);
});

test('licence boundary: unresolved paths and workspace dependencies are input errors', t => {
  const root = repository(t);
  replace(root, 'Cargo.toml', 'path = "shared"', 'path = "missing"');
  invoke('check-licence-boundary', ['--root', root], 2);
  replace(root, 'Cargo.toml', 'path = "missing"', 'path = "../outside"');
  invoke('check-licence-boundary', ['--root', root], 2);
});
