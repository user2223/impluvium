import test from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import { readFileSync, writeFileSync, mkdirSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { copyOf, invoke, replace } from './helpers.mjs';
import { loadAndRender, render } from '../identity-doc.mjs';

const repo = t => copyOf(t, 'identity-repository');
const args = root => ['--root', root, '--check'];
const page = root => path.join(root, 'docs/dev/identity.md');
const writer = fileURLToPath(new URL('../../identity/write-doc.mjs', import.meta.url));
function generate(root) {
  mkdirSync(path.dirname(page(root)), { recursive: true });
  writeFileSync(page(root), loadAndRender(root));
}

test('identity doc: the page rendered from the tables passes the check', t => {
  const root = repo(t);
  generate(root);
  invoke('identity-doc', args(root), 0);
});

test('identity doc: the render function is pure and carries the change rule, the tables and every register name', t => {
  const root = repo(t);
  const identity = JSON.parse(readFileSync(path.join(root, 'tools/identity.json'), 'utf8'));
  const entries = JSON.parse(readFileSync(path.join(root, 'tools/env-register.json'), 'utf8')).entries;
  const text = render(identity, entries);
  assert.equal(render(identity, entries), text);
  assert.match(text, /only with the approval of the maintainer/);
  assert.match(text, /every change of a value is a breaking change/);
  assert.match(text, /\| `domain` \| `example\.com` \|/);
  assert.match(text, /\| `os` \| `linux, windows` \|/);
  assert.doesNotMatch(text, /bindings|schema/);
  for (const entry of entries) assert.ok(text.includes('`' + entry.name + '`'), entry.name);
  assert.ok(text.endsWith('\n') && !text.endsWith('\n\n'));
});

test('identity doc: drift in the table, in the register or in the page is an error', t => {
  const root = repo(t);
  generate(root);
  replace(root, 'tools/identity.json', '"example.com"', '"example.org"');
  assert.match(invoke('identity-doc', args(root), 1).stderr, /identity\.md:\d+: out of date/);
  const second = repo(t);
  generate(second);
  replace(second, 'tools/env-register.json', 'Synthetic example variable.', 'Changed text.');
  invoke('identity-doc', args(second), 1);
  const third = repo(t);
  generate(third);
  writeFileSync(page(third), readFileSync(page(third), 'utf8') + 'hand edit\n');
  invoke('identity-doc', args(third), 1);
});

test('identity doc: a missing page is an error and nothing is written', t => {
  const root = repo(t);
  assert.match(invoke('identity-doc', args(root), 1).stderr, /identity\.md:1: missing/);
  assert.throws(() => readFileSync(page(root)));
});

test('identity doc: --check is required, a broken register is an input error', t => {
  const root = repo(t);
  invoke('identity-doc', ['--root', root], 2);
  replace(root, 'tools/env-register.json', '"version":1', '"version":3');
  invoke('identity-doc', args(root), 2);
});

test('identity doc: the writer produces exactly the page that the check expects', t => {
  const root = repo(t);
  const written = spawnSync(process.execPath, [writer, '--root', root], { encoding: 'utf8' });
  assert.equal(written.status, 0, written.stderr);
  invoke('identity-doc', args(root), 0);
  const printed = spawnSync(process.execPath, [writer, '--root', root, '--stdout'], { encoding: 'utf8' });
  assert.equal(printed.status, 0, printed.stderr);
  assert.equal(printed.stdout, readFileSync(page(root), 'utf8'));
});
