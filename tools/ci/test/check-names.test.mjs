import test from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import { readFileSync, writeFileSync } from 'node:fs';
import { copyOf, invoke, replace } from './helpers.mjs';

const repo = t => copyOf(t, 'names-repository');
const args = root => ['--root', root];
const baselinePath = root => path.join(root, 'tools/ci/names-baseline.json');

test('names: the fixture matches its baseline; destructor forms and allowed lines do not count', t => {
  invoke('check-names', args(repo(t)), 0);
});

test('names: a new line with an inherited name exceeds the baseline', t => {
  const root = repo(t);
  replace(root, 'web/page.vue', '<h1>Drop</h1>', '<h1>Drop</h1>\n<h2>Drop again</h2>');
  assert.match(invoke('check-names', args(root), 1).stderr, /web\/page\.vue.*4 lines for word exceed baseline 3/);
});

test('names: a pattern counts a line once, a second pattern on the same line counts separately', t => {
  const root = repo(t);
  replace(root, 'web/page.vue', '<h1>Drop</h1>', '<h1>Drop Drop drop-oss</h1>');
  assert.match(invoke('check-names', args(root), 1).stderr, /2 lines for oss exceed baseline 1/);
});

test('names: a new file with an inherited name fails', t => {
  const root = repo(t);
  writeFileSync(path.join(root, 'web/new.vue'), 'Drop\n');
  assert.match(invoke('check-names', args(root), 1).stderr, /web\/new\.vue.*1 lines for word exceed baseline 0/);
});

test('names: fewer names than the baseline ask for a lower baseline', t => {
  const root = repo(t);
  replace(root, 'web/page.vue', '<h1>Drop</h1>\n', '');
  assert.match(invoke('check-names', args(root), 1).stderr, /lower the baseline for web\/page\.vue word from 3 to 2/);
});

test('names: Rust destructor forms are not names, other uses of the word are', t => {
  const root = repo(t);
  writeFileSync(path.join(root, 'src/more.rs'), 'impl<T: Clone> Drop for Other<T> {\n    fn drop(&mut self) {}\n}\nuse std::ops::Drop;\nDrop::drop(&mut x);\n');
  invoke('check-names', args(root), 0);
  writeFileSync(path.join(root, 'src/more.rs'), 'impl Drop for Other {}\n');
  invoke('check-names', args(root), 0);
  // The same text outside Rust is an inherited name.
  writeFileSync(path.join(root, 'src/more.txt'), 'impl Drop for Other {}\n');
  assert.match(invoke('check-names', args(root), 1).stderr, /more\.txt/);
});

test('names: only destructor tokens are exempt in Rust, strings and comments on the same line still count', t => {
  const root = repo(t);
  writeFileSync(path.join(root, 'src/title.rs'), 'pub const TITLE: &str = "Drop for Linux";\n');
  assert.match(invoke('check-names', args(root), 1).stderr, /src\/title\.rs.*1 lines for word exceed baseline 0/);
  writeFileSync(path.join(root, 'src/title.rs'), 'impl Drop for Guard { fn name() -> &\'static str { "Drop" } }\n');
  assert.match(invoke('check-names', args(root), 1).stderr, /src\/title\.rs.*1 lines for word/);
  writeFileSync(path.join(root, 'src/title.rs'), 'impl Drop for Guard {} // Drop it\n');
  assert.match(invoke('check-names', args(root), 1).stderr, /src\/title\.rs.*1 lines for word/);
  writeFileSync(path.join(root, 'src/title.rs'), 'impl Drop for Guard {}\n');
  invoke('check-names', args(root), 0);
});

test('names: only the Drop token of a destructor is blanked, text between and around the tokens counts', t => {
  const root = repo(t);
  const file = path.join(root, 'src/title.rs');
  writeFileSync(file, 'impl /* Drop for Linux */ Drop for Guard {}\n');
  assert.match(invoke('check-names', args(root), 1).stderr, /src\/title\.rs.*1 lines for word/);
  writeFileSync(file, 'impl Drop for A {} const S: &str = "Drop"; impl Drop for B {}\n');
  assert.match(invoke('check-names', args(root), 1).stderr, /src\/title\.rs.*1 lines for word/);
  writeFileSync(file, 'impl Drop for A {} impl<T> std::ops::Drop for B<T> {} fn f() { Drop::drop(&mut x); }\n');
  invoke('check-names', args(root), 0);
});

test('names: the provenance files are exempt by exact lines only', t => {
  const entries = JSON.parse(readFileSync(new URL('../names-allow.json', import.meta.url), 'utf8')).entries;
  for (const file of ['README.md', 'CONTRIBUTING.md', 'SECURITY.md']) {
    const entry = entries.find(item => item.path === file);
    assert.ok(entry?.lines?.length, `${file} must be allowed by exact lines`);
  }
});

test('names: the allow-list takes whole files, globs and exact lines', t => {
  const root = repo(t);
  writeFileSync(path.join(root, 'docs/more.md'), 'Drop everywhere\n');
  assert.match(invoke('check-names', args(root), 1).stderr, /docs\/more\.md/);
  replace(root, 'tools/ci/names-allow.json', '{"path":"docs/ORIGIN.md"', '{"glob":"docs/**","reason":"Origin pages."},\n  {"path":"docs/ORIGIN.md"');
  invoke('check-names', args(root), 0);
  // An exact line allows that line only.
  replace(root, 'config.json', '  "name": "Drop"', '  "name": "Drop",\n  "other": "Drop"');
  invoke('check-names', args(root), 1);
});

test('names: an allow-list line that vanished is reported', t => {
  const root = repo(t);
  replace(root, 'config.json', '  "legacy": "Drop",\n', '');
  assert.match(invoke('check-names', args(root), 1).stderr, /config\.json:1: allow-list line no longer present/);
});

test('names: a malformed allow-list or baseline is an input error', t => {
  const root = repo(t);
  writeFileSync(path.join(root, 'tools/ci/names-allow.json'), '{"version":1,"entries":[{"path":"a","glob":"b","reason":"x"}]}');
  invoke('check-names', args(root), 2);
  const second = repo(t);
  writeFileSync(baselinePath(second), '{"version":1,"files":{"web/page.vue":{"unknown-pattern":1}}}');
  invoke('check-names', args(second), 2);
  const third = repo(t);
  writeFileSync(baselinePath(third), '{"version":2,"files":{}}');
  invoke('check-names', args(third), 2);
});

test('names: binary files do not count', t => {
  const root = repo(t);
  writeFileSync(path.join(root, 'blob.bin'), Buffer.from('Drop\0Drop'));
  invoke('check-names', args(root), 0);
});

test('names: previous-baseline lets counts shrink per file and pattern, never grow', t => {
  const root = repo(t);
  const previous = path.join(root, 'previous.json');
  const withPrevious = () => [...args(root), '--previous-baseline', previous];
  writeFileSync(previous, readFileSync(baselinePath(root)));
  invoke('check-names', withPrevious(), 0);
  // The tree and the baseline shrink: allowed.
  replace(root, 'web/page.vue', '<h1>Drop</h1>\n', '');
  replace(root, 'tools/ci/names-baseline.json', '"word":3', '"word":2');
  invoke('check-names', withPrevious(), 0);
  // A baseline that is higher than the base revision's one grows the debt.
  writeFileSync(previous, readFileSync(baselinePath(root), 'utf8').replace('"word":2', '"word":1'));
  assert.match(invoke('check-names', withPrevious(), 1).stderr, /web\/page\.vue: word baseline grew/);
  // A file or pattern that the base revision did not carry is growth as well.
  writeFileSync(previous, readFileSync(baselinePath(root), 'utf8').replace('"build.sh": {"build-arg":1,"rpm-package":1},', ''));
  assert.match(invoke('check-names', withPrevious(), 1).stderr, /build\.sh: build-arg baseline grew/);
});

test('names: --print-baseline prints the current counts to stdout and writes nothing', t => {
  const root = repo(t);
  const before = readFileSync(baselinePath(root), 'utf8');
  replace(root, 'web/page.vue', '<h1>Drop</h1>', '<h1>Drop</h1>\n<h2>Drop again</h2>');
  const result = invoke('check-names', [...args(root), '--print-baseline'], 0);
  const printed = JSON.parse(result.stdout);
  assert.equal(printed.version, 1);
  assert.deepEqual(printed.files['web/page.vue'], { 'desktop-title': 1, 'literal-name': 1, oss: 1, scheme: 1, word: 4 });
  assert.deepEqual(Object.keys(printed.files), ['build.sh', 'config.json', 'src/lib.rs', 'web/page.vue']);
  assert.equal(readFileSync(baselinePath(root), 'utf8'), before);
  // The printed text is a valid baseline for the changed tree.
  writeFileSync(baselinePath(root), result.stdout);
  invoke('check-names', args(root), 0);
});
