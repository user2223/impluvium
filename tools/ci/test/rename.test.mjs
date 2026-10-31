import test from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';
import { temporary } from './helpers.mjs';

const script = fileURLToPath(new URL('../../rename/rename.mjs', import.meta.url));
const pattern = '(?:(?<![A-Za-z0-9_])|(?<=\\\\[nrt]))Drop(?![A-Za-z0-9_])(?!-OSS)';

const LOCALE = `{
  "title": "Drop",
  "greeting": "Welcome to Drop,\\nDrop is here",
  "helpUsTranslate": "Help us translate Drop {arrow}",
  "nested": {"fsTitle": "Drop-style", "list": ["Drop", "Drop"], "other": "About Drop"},
  "drop": "x"
}
`;
const FRENCH = `{
  "a": "Le serveur interne de Drop.",
  "b": "Drop a échoué",
  "aboutDrop": "À propos de Drop"
}
`;
const RUST = `use std::ops::Drop;
struct Guard;
impl Drop for Guard {
    fn drop(&mut self) {}
}
impl ::std::ops::Drop for Other {}
fn main() {
    // Drop leading slash
    let a = "Drop crashed";
    let b = r#"Drop "raw""#;
    Drop::drop(&mut g);
    let c = 'D';
}
`;
const VUE = `<template>
  <p>Drop will start</p>
  <a href="https://github.com/Drop-OSS/drop">Drop-OSS</a>
  <DropWordmark /><span>{{ $dropFetch("/api") }}</span>
  <p>Drop keeps this line</p>
</template>
`;

function repo(t, exclude = {}) {
  const root = temporary(t);
  const put = (file, text) => {
    mkdirSync(path.dirname(path.join(root, file)), { recursive: true });
    writeFileSync(path.join(root, file), text);
  };
  put('tools/identity.json', '{"product":{"name":"Acme"}}\n');
  put('web/i18n/en.json', LOCALE);
  put('web/i18n/fr.json', FRENCH);
  put('app/src/lib.rs', RUST);
  put('web/page.vue', VUE);
  put('other/readme.md', 'Drop stays\n');
  const rule = (id, kind, files, extra = {}) => ({ id, kind, files, pattern, replacement: 'product.name', ...extra });
  put('tools/rename/rules.json', JSON.stringify({ version: 1, rules: [
    rule('locale', 'json', ['web/i18n/en.json'], { exclude: [
      { glob: 'web/i18n/*.json', keys: ['helpUsTranslate', '**.fsTitle', 'nested.list.1'], reason: 'test' }, ...(exclude.locale ?? [])] }),
    rule('elision', 'json', ['web/i18n/fr.json'], { pattern: '\\bde Drop(?![A-Za-z0-9_])', replacement: "d'${product.name}", exclude: [{ glob: 'web/i18n/fr.json', keys: ['aboutDrop'], reason: 'test' }] }),
    rule('locale-fr', 'json', ['web/i18n/fr.json'], { exclude: [{ glob: 'web/i18n/fr.json', keys: ['aboutDrop'], reason: 'test' }] }),
    rule('rust', 'rust', ['app/**/*.rs']),
    rule('text', 'text', ['web/**/*.vue'], { exclude: [{ path: 'web/page.vue', lines: ['<p>Drop keeps this line</p>'], reason: 'test' }, ...(exclude.text ?? [])] }),
  ] }));
  return root;
}
const run = (root, ...args) => spawnSync(process.execPath, [script, '--root', root, ...args], { encoding: 'utf8', timeout: 15_000 });
const changes = (root, extra = []) => JSON.parse(run(root, '--report', '--format', 'json', ...extra).stdout);
const text = (root, file) => readFileSync(path.join(root, file), 'utf8');

test('rename: JSON locale values are replaced in place, keys and layout stay', t => {
  const root = repo(t);
  assert.equal(run(root, '--apply').status, 0);
  const result = text(root, 'web/i18n/en.json');
  assert.match(result, /^  "title": "Acme",$/m);
  assert.match(result, /"greeting": "Welcome to Acme,\\nAcme is here"/);
  assert.match(result, /"other": "About Acme"/);
  assert.match(result, /"list": \["Acme", "Drop"\]/);
  assert.match(result, /"drop": "x"/);
  assert.equal(JSON.parse(result).title, 'Acme');
});

test('rename: key-path exclusions keep exact keys, wildcard suffixes and array elements', t => {
  const root = repo(t);
  run(root, '--apply');
  const result = JSON.parse(text(root, 'web/i18n/en.json'));
  assert.equal(result.helpUsTranslate, 'Help us translate Drop {arrow}');
  assert.equal(result.nested.fsTitle, 'Drop-style');
  assert.deepEqual(result.nested.list, ['Acme', 'Drop']);
  const skipped = changes(repo(t)).skipped.filter(entry => entry.rule === 'locale').map(entry => entry.key).sort();
  assert.deepEqual(skipped, ['helpUsTranslate', 'nested.fsTitle', 'nested.list.1']);
});

test('rename: Rust destructor forms and comments are untouched, strings are replaced', t => {
  const root = repo(t);
  run(root, '--apply');
  const result = text(root, 'app/src/lib.rs');
  assert.match(result, /^use std::ops::Drop;$/m);
  assert.match(result, /^impl Drop for Guard \{$/m);
  assert.match(result, /impl ::std::ops::Drop for Other/);
  assert.match(result, /^    Drop::drop\(&mut g\);$/m);
  assert.match(result, /\/\/ Drop leading slash/);
  assert.match(result, /let a = "Acme crashed";/);
  assert.match(result, /let b = r#"Acme "raw""#;/);
});

test('rename: text rules leave identifiers, upstream URLs and excluded lines alone', t => {
  const root = repo(t);
  run(root, '--apply');
  const result = text(root, 'web/page.vue');
  assert.match(result, /<p>Acme will start<\/p>/);
  assert.match(result, /href="https:\/\/github\.com\/Drop-OSS\/drop">Drop-OSS</);
  assert.match(result, /<DropWordmark \/><span>\{\{ \$dropFetch/);
  assert.match(result, /<p>Drop keeps this line<\/p>/);
  assert.equal(text(root, 'other/readme.md'), 'Drop stays\n');
});

test('rename: --apply is idempotent, a second --report is empty', t => {
  const root = repo(t);
  assert.ok(changes(root).changes.length > 0);
  assert.equal(run(root, '--apply').status, 0);
  assert.deepEqual(changes(root).changes, []);
  const once = ['web/i18n/en.json', 'app/src/lib.rs', 'web/page.vue'].map(file => text(root, file));
  const second = JSON.parse(run(root, '--apply', '--format', 'json').stdout);
  assert.deepEqual(second.written, []);
  assert.deepEqual(['web/i18n/en.json', 'app/src/lib.rs', 'web/page.vue'].map(file => text(root, file)), once);
});

test('rename: --report, also as the default mode, never writes', t => {
  const root = repo(t);
  const files = ['web/i18n/en.json', 'app/src/lib.rs', 'web/page.vue', 'tools/rename/rules.json'];
  const before = files.map(file => text(root, file));
  for (const args of [[], ['--report'], ['--report', '--format', 'json']]) {
    const result = run(root, ...args);
    assert.equal(result.status, 0, result.stderr);
    assert.match(result.stdout, /Rename report \(dry run\)|"changes"/);
  }
  assert.deepEqual(files.map(file => text(root, file)), before);
});

test('rename: the report names file, line, before, after and rule', t => {
  const root = repo(t);
  const entry = changes(root).changes.find(change => change.file === 'app/src/lib.rs');
  assert.deepEqual({ ...entry, count: undefined }, { rule: 'rust', file: 'app/src/lib.rs', line: 9, before: '    let a = "Drop crashed";', after: '    let a = "Acme crashed";', count: undefined });
  const markdown = run(root).stdout;
  assert.match(markdown, /- app\/src\/lib\.rs:9 \[rust\]/);
  assert.match(markdown, /Replacements per rule and file group/);
});

test('rename: a whole-file exclusion skips the file and is listed', t => {
  const root = repo(t, { text: [{ path: 'web/page.vue', reason: 'names an upstream resource' }] });
  const result = changes(root);
  assert.ok(!result.changes.some(change => change.file === 'web/page.vue'));
  assert.deepEqual(result.wholeFile.map(entry => entry.file), ['web/page.vue']);
});

test('rename: an exclusion line that no longer exists fails the run', t => {
  const root = repo(t, { text: [{ path: 'web/page.vue', lines: ['<p>gone</p>'], reason: 'test' }] });
  const result = run(root, '--report');
  assert.equal(result.status, 1);
  assert.match(result.stderr, /web\/page\.vue:1: exclusion line no longer present: <p>gone<\/p>/);
});

test('rename: invalid arguments and rules are input errors', t => {
  const root = repo(t);
  assert.equal(run(root, '--report', '--apply').status, 2);
  assert.equal(run(root, '--format', 'xml').status, 2);
  writeFileSync(path.join(root, 'tools/rename/rules.json'), '{"version":1,"rules":[{"id":"x"}]}');
  assert.equal(run(root).status, 2);
});

test('rename: a template replacement handles the French elision and runs before the general rule', t => {
  const root = repo(t);
  assert.equal(run(root, '--apply').status, 0);
  const result = JSON.parse(text(root, 'web/i18n/fr.json'));
  assert.equal(result.a, "Le serveur interne d'Acme.");
  assert.equal(result.b, 'Acme a échoué');
  assert.equal(result.aboutDrop, 'À propos de Drop');
  assert.deepEqual(changes(root).changes, []);
});

test('rename: the English article before the product name becomes "an" and runs before the general rule', t => {
  const root = repo(t);
  writeFileSync(path.join(root, 'web/setup.vue'), '<p>Set up a Drop instance. A Drop server needs a Dropbox.</p>\n');
  const rules = JSON.parse(text(root, 'tools/rename/rules.json'));
  rules.rules.unshift(
    { id: 'article', kind: 'text', files: ['web/**/*.vue'], pattern: '(?<![A-Za-z0-9_])a Drop(?![A-Za-z0-9_])', replacement: 'an ${product.name}' },
    { id: 'article-upper', kind: 'text', files: ['web/**/*.vue'], pattern: '(?<![A-Za-z0-9_])A Drop(?![A-Za-z0-9_])', replacement: 'An ${product.name}' });
  writeFileSync(path.join(root, 'tools/rename/rules.json'), JSON.stringify(rules));
  assert.equal(run(root, '--apply').status, 0);
  assert.equal(text(root, 'web/setup.vue'), '<p>Set up an Acme instance. An Acme server needs a Dropbox.</p>\n');
  assert.deepEqual(changes(root).changes, []);
});
