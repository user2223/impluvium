import test from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import { appendFileSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { copyOf, invoke, replace } from './helpers.mjs';

const repo = t => copyOf(t, 'env-repository');
const args = root => ['--root', root];
const registerFile = root => path.join(root, 'tools/env-register.json');
const lib = 'desktop/src-tauri/src/lib.rs';
const config = 'server/server/config.ts';
function write(root, file, text) {
  mkdirSync(path.dirname(path.join(root, file)), { recursive: true });
  writeFileSync(path.join(root, file), text);
}
function register(root, change) {
  const data = JSON.parse(readFileSync(registerFile(root), 'utf8'));
  change(data);
  writeFileSync(registerFile(root), JSON.stringify(data));
  return data;
}

test('env: every supported form passes (process.env forms, group import, macros, Dockerfile, example file)', t => {
  invoke('check-env', args(repo(t)), 0);
});

test('env: macros with braces or brackets and an example file with export pass when registered', t => {
  const root = repo(t);
  replace(root, lib, 'option_env!("EXAMPLE_BUILT")', 'option_env! { "EXAMPLE_BUILT" }');
  replace(root, lib, 'env!("EXAMPLE_MANIFEST")', 'env!["EXAMPLE_MANIFEST"]');
  replace(root, 'server/.env.example', 'EXAMPLE_DB=', 'export EXAMPLE_DB = ');
  invoke('check-env', args(root), 0);
});

test('env: a line of an example file that is neither a comment nor NAME=value is an input error', t => {
  const root = repo(t);
  appendFileSync(path.join(root, 'server/.env.example'), 'just some words\n');
  invoke('check-env', args(root), 2);
});

test('env: a self-import of the env module through a group with self also passes', t => {
  const root = repo(t);
  write(root, lib, 'use std::env::{self, args};\nfn f() {\n    let _ = env::var("EXAMPLE_LOG");\n    let _ = std::env::var_os("EXAMPLE_HOME");\n' +
    '    let _ = option_env!("EXAMPLE_BUILT");\n    let _ = env!("EXAMPLE_MANIFEST");\n    let _ = args();\n}\n');
  invoke('check-env', args(root), 0);
});

for (const [label, file, text] of [
  ['script', config, 'export const extra = process.env.EXAMPLE_NEW;\n'],
  ['indexed script', config, 'export const extra = process.env["EXAMPLE_NEW"];\n'],
  ['Rust env::var', lib, 'fn extra() { let _ = env::var("EXAMPLE_NEW"); }\n'],
  ['Rust qualified', lib, 'fn extra() { let _ = std::env::var_os("EXAMPLE_NEW"); }\n'],
  ['Rust macro', lib, 'const EXTRA: &str = env!("EXAMPLE_NEW");\n'],
  ['Dockerfile ARG', 'Dockerfile', 'ARG EXAMPLE_NEW\n'],
  ['Dockerfile ENV', 'Dockerfile', 'ENV EXAMPLE_NEW=1\n'],
  ['Rust macro with braces', lib, 'const A: Option<&str> = option_env! { "EXAMPLE_NEW" };\n'],
  ['Rust macro with brackets', lib, 'const A: &str = env!["EXAMPLE_NEW"];\n'],
  ['Rust macro with spaces', lib, 'const A: &str = env ! ( "EXAMPLE_NEW", "message" );\n'],
  ['example file', 'server/.env.example', 'EXAMPLE_NEW=1\n'],
  ['example file with export', 'server/.env.example', 'export EXAMPLE_NEW=1\n'],
  ['example file with spaces around =', 'server/.env.example', 'EXAMPLE_NEW = 1\n'],
  ['commented example declaration', 'server/.env.example', '# EXAMPLE_NEW = 1\n'],
  ['script with a comment between the tokens', config, 'export const extra = process /* deployment setting */ .env.EXAMPLE_NEW;\n'],
  ['script after a URL string', config, 'export const u = "https://example.com"; export const extra = process.env.EXAMPLE_NEW;\n'],
  ['script after a postfix increment and a division', config, 'let n = 1;\nexport const x = n++ / Number(process.env.EXAMPLE_NEW) / 2;\n'],
  ['script after a postfix decrement and a division', config, 'let n = 1;\nexport const x = n-- / Number(process.env.EXAMPLE_NEW) / 2;\n'],
  ['script after a division of a call result', config, 'export const x = f(1) / process.env.EXAMPLE_NEW / g(2);\n'],
  ['script after an astral character and a comment', config, 'export const icon = "\u{1F600}"; /* note */process.env.EXAMPLE_NEW;\n'],
  ['script after astral characters in a comment', config, '/* \u{1F600}\u{1F600} */ export const x = process.env.EXAMPLE_NEW;\n'],
  ['script inside a template expression', config, 'export const extra = `value ${process.env.EXAMPLE_NEW}`;\n'],
]) {
  test(`env: an unregistered name in a ${label} is an error`, t => {
    const root = repo(t);
    appendFileSync(path.join(root, file), text);
    assert.match(invoke('check-env', args(root), 1).stderr, new RegExp(`${file.replaceAll('.', '\\.')}:\\d+: EXAMPLE_NEW: environment name is not in the register`));
  });
}

test('env: ordinary process members, text with "process)" and regex literals are fine', t => {
  const root = repo(t);
  appendFileSync(path.join(root, config), 'export const os = process.platform;\nexport const arch = process.arch;\nexport const cwd = process.cwd();\n' +
    'process.exit(1);\nexport const text = `items to process)...`;\nexport const pattern = /["\'`]/g;\n// process.env.EXAMPLE_COMMENT is not a read\n' +
    '/* const all = process.env; */\nexport const ok = process.env.EXAMPLE_URL;\n');
  invoke('check-env', args(root), 0);
});

test('env: a read that only appears in a comment is no read site', t => {
  const root = repo(t);
  replace(root, config, 'export const token = process.env["EXAMPLE_TOKEN"];', '// export const token = process.env["EXAMPLE_TOKEN"];');
  assert.match(invoke('check-env', args(root), 1).stderr, /EXAMPLE_TOKEN: active entry with evidence "read" has no read site/);
});

test('env: an unterminated comment or template in a script is an input error', t => {
  const root = repo(t);
  appendFileSync(path.join(root, config), '/* never closed\n');
  invoke('check-env', args(root), 2);
  const second = repo(t);
  appendFileSync(path.join(second, config), 'const x = `never closed;\n');
  invoke('check-env', args(second), 2);
});

test('env: an active entry whose read disappeared is stale', t => {
  const root = repo(t);
  replace(root, config, 'export const token = process.env["EXAMPLE_TOKEN"];\n', '');
  assert.match(invoke('check-env', args(root), 1).stderr, /EXAMPLE_TOKEN: active entry with evidence "read" has no read site/);
});

test('env: a declaration alone is no read site, a consumer entry needs none', t => {
  const root = repo(t);
  register(root, data => { data.entries.find(entry => entry.name === 'EXAMPLE_PORT').evidence = 'read'; });
  assert.match(invoke('check-env', args(root), 1).stderr, /EXAMPLE_PORT: active entry with evidence "read" has no read site/);
});

for (const [label, text, message] of [
  ['destructuring', 'const { EXAMPLE_URL } = process.env;\n', /used as a whole/],
  ['alias', 'const env = process.env;\n', /used as a whole/],
  ['spread', 'export const copy = { ...process.env };\n', /used as a whole/],
  ['enumeration', 'export const names = Object.keys(process.env);\n', /used as a whole/],
  ['computed name', 'export const dynamic = process.env[name];\n', /needs a string literal name/],
  ['template name', 'export const dynamic = process.env[`EXAMPLE_${name}`];\n', /needs a string literal name/],
  ['bare process', 'const p = process;\n', /process used other than/],
  ['cast process', 'const p = process as NodeJS.Process;\nexport const b = p.env.EXAMPLE_NEW;\n', /process used other than/],
  ['spread process', 'export const copy = { ...process };\n', /process used other than/],
  ['destructured process', 'const { env } = process;\n', /process used other than/],
  ['bare process in a call', 'run(process);\n', /process used other than/],
  ['computed process access', 'export const e = process["env"].EXAMPLE_URL;\n', /process used other than/],
  ['computed process access with a comment', 'export const e = process /* c */ ["env"].EXAMPLE_URL;\n', /process used other than/],
  ['global object', 'export const e = globalThis.process.env.EXAMPLE_URL;\n', /through a global object/],
  ['module import', 'import process from "node:process";\n', /imported as a module/],
]) {
  test(`env: process.env form rejected: ${label}`, t => {
    const root = repo(t);
    appendFileSync(path.join(root, config), text);
    assert.match(invoke('check-env', args(root), 1).stderr, new RegExp(`config\\.ts:\\d+: .*${message.source}`));
  });
}

for (const [label, text, message] of [
  ['direct import', 'use std::env::var;\nfn f() { let _ = var("EXAMPLE_LOG"); }\n', /direct import of an environment reader/],
  ['direct import in a group', 'use std::env::{var, args};\n', /direct import/],
  ['direct import of the other reader in a group', 'use std::{fs, env::var_os};\n', /direct import/],
  ['aliased reader', 'use std::env::var as v;\n', /direct import/],
  ['multi-line group', 'use std::env::{\n    args,\n    var_os,\n};\n', /direct import/],
  ['glob import', 'use std::env::*;\n', /direct import/],
  ['std glob', 'use std::*;\n', /glob import or alias of std/],
  ['aliased module', 'use std::env as e;\n', /aliased import of std::env/],
  ['aliased module in a group', 'use std::{fs, env as e};\n', /aliased import of std::env/],
  ['enumeration', 'use std::env;\nfn f() { let _ = env::vars(); }\n', /enumerating all environment variables/],
  ['qualified enumeration', 'fn f() { let _ = std::env::vars_os(); }\n', /enumerating all environment variables/],
  ['non-literal argument', 'use std::env;\nfn f(n: &str) { let _ = env::var(n); }\n', /not a string literal/],
  ['reader without a call', 'use std::env;\nfn f() { let g = env::var; }\n', /not a string literal/],
  ['env:: without the import', 'fn f() { let _ = env::var("EXAMPLE_LOG"); }\n', /without `use std::env;`/],
  ['non-literal macro', 'use std::env;\nconst X: Option<&str> = option_env!(NAME);\n', /needs a string literal name/],
  ['non-literal macro with braces', 'const X: Option<&str> = option_env! { NAME };\n', /needs a string literal name/],
  ['self alias of std', 'use std::{self as s};\n', /glob import or alias of std/],
  ['self alias of std next to other items', 'use std::{self as s, fs};\n', /glob import or alias of std/],
  ['self alias of env', 'use std::env::{self as e};\n', /aliased import of std::env/],
  ['self alias of env in a nested group', 'use std::{env::{self as e}};\n', /aliased import of std::env/],
  ['self alias of env next to other items', 'use std::env::{args, self as e};\n', /aliased import of std::env/],
]) {
  test(`env: Rust form rejected: ${label}`, t => {
    const root = repo(t);
    write(root, lib, text);
    assert.match(invoke('check-env', args(root), 1).stderr, new RegExp(`lib\\.rs:\\d+: .*${message.source}`));
  });
}

test('env: the exception binds the line that builds the name and the line that reads it', t => {
  const root = repo(t);
  invoke('check-env', args(root), 0);
  const building = repo(t);
  replace(building, 'server/server/services.ts', 'EXAMPLE_SERVICE_${name', 'EXAMPLE_SERVICE_X_${name');
  const result = invoke('check-env', args(building), 1);
  assert.match(result.stderr, /services\.ts:3: .*register a pattern with this exact snippet/);
  assert.match(result.stderr, /EXAMPLE_SERVICE_\*: pattern exception matches no dynamic access/);
  const reading = repo(t);
  replace(reading, 'server/server/services.ts', 'if (!process.env[disableEnv]) {', 'if (!process.env[disableEnv] && ready) {');
  assert.match(invoke('check-env', args(reading), 1).stderr, /services\.ts:3: /);
});

test('env: a second dynamic read elsewhere is not covered by the exception', t => {
  const root = repo(t);
  write(root, 'server/server/other.ts', 'const key = `EXAMPLE_SERVICE_${name}`;\nexport const flag = process.env[key];\n');
  assert.match(invoke('check-env', args(root), 1).stderr, /other\.ts:2: /);
});

test('env: an exception whose file is gone is stale', t => {
  const root = repo(t);
  writeFileSync(path.join(root, 'server/server/services.ts'), 'export {};\n');
  assert.match(invoke('check-env', args(root), 1).stderr, /EXAMPLE_SERVICE_\*: pattern exception matches no dynamic access/);
});

test('env: IMPLUVIUM_ names must be registered anywhere in the repository, wildcards cover their family', t => {
  const root = repo(t);
  write(root, 'docs/notes.md', 'Use IMPLUVIUM_OTHER_DIR here.\n');
  assert.match(invoke('check-env', args(root), 1).stderr, /notes\.md:1: IMPLUVIUM_OTHER_DIR: IMPLUVIUM_ name is not in the register/);
  register(root, data => data.entries.push({ ...data.entries.find(entry => entry.name === 'IMPLUVIUM_EXAMPLE_DIR'), name: 'IMPLUVIUM_OTHER_*' }));
  invoke('check-env', args(root), 0);
  write(root, 'docs/notes.md', 'Use IMPLUVIUM_OTHER_DIR and IMPLUVIUM_OTHER_* here.\n');
  invoke('check-env', args(root), 0);
  // A wildcard reference must itself be registered.
  write(root, 'docs/notes.md', 'Use IMPLUVIUM_NOPE_* here.\n');
  assert.match(invoke('check-env', args(root), 1).stderr, /IMPLUVIUM_NOPE_\*: IMPLUVIUM_ name is not in the register/);
});

test('env: scripts and workflow files are not scanned, but only the three script roots are', t => {
  const root = repo(t);
  write(root, 'tools/helper.mjs', 'export const x = process.env.EXAMPLE_TOOL;\n');
  write(root, '.gitea/workflows/ci.yml', 'env:\n  EXAMPLE_CI: 1\n');
  write(root, 'scripts/run.ts', 'export const x = process.env.EXAMPLE_ELSEWHERE;\n');
  write(root, 'launch.sh', 'echo "$EXAMPLE_SHELL"\n');
  invoke('check-env', args(root), 0);
  write(root, 'desktop/main/app.vue', '<script>\nconst x = process.env.EXAMPLE_UI;\n</script>\n');
  assert.match(invoke('check-env', args(root), 1).stderr, /app\.vue:2: EXAMPLE_UI/);
});

test('env: only the script block of a Vue file is read; a multi-line template literal in an attribute is fine', t => {
  const root = repo(t);
  write(root, 'desktop/main/chart.vue', '<template>\n  <path :d="`\n    M ${x}\n  `" />\n  <p>{{ process.env.EXAMPLE_TEMPLATE }}</p>\n</template>\n<script setup lang="ts">\nconst a = 1;\n</script>\n');
  invoke('check-env', args(root), 0);
  write(root, 'desktop/main/chart.vue', '<template><p /></template>\n<script setup lang="ts">\nconst a = process.env.EXAMPLE_SCRIPT;\n</script>\n');
  assert.match(invoke('check-env', args(root), 1).stderr, /chart\.vue:3: EXAMPLE_SCRIPT/);
});

for (const [label, closing] of [['a space before >', '</script >'], ['upper case', '</SCRIPT>'], ['a newline before >', '</script\n>']]) {
  test(`env: the Vue script block is extracted with a closing tag with ${label}`, t => {
    const root = repo(t);
    write(root, 'desktop/main/page.vue', `<template><p /></template>\n<script setup lang="ts" generic="T extends Array<string>">\nconst x = process.env.EXAMPLE_NEW;\n${closing}\n`);
    assert.match(invoke('check-env', args(root), 1).stderr, /page\.vue:3: EXAMPLE_NEW/);
  });
}

test('env: a component whose name starts with script is not a script tag', t => {
  const root = repo(t);
  write(root, 'desktop/main/page.vue', '<script setup lang="ts">\nconst x = process.env.EXAMPLE_NEW;\n</script>\n<template>\n  <script-editor />\n  <script-editor>text</script-editor>\n</template>\n');
  assert.match(invoke('check-env', args(root), 1).stderr, /page\.vue:2: EXAMPLE_NEW/);
  write(root, 'desktop/main/page.vue', '<script setup lang="ts">\nconst x = 1;\n</script>\n<template><script-editor /></template>\n');
  invoke('check-env', args(root), 0);
});

test('env: a Vue script block that cannot be extracted is an input error', t => {
  const root = repo(t);
  write(root, 'desktop/main/page.vue', '<script setup lang="ts">\nconst x = process.env.EXAMPLE_NEW;\n');
  invoke('check-env', args(root), 2);
  write(root, 'desktop/main/page.vue', '<script setup lang="ts"\nconst x = 1;\n');
  invoke('check-env', args(root), 2);
});

test('env: comments in Rust are not reads', t => {
  const root = repo(t);
  write(root, lib, readFileSync(path.join(root, lib), 'utf8') + '/* std::env::vars() and env::var(name) */\n');
  invoke('check-env', args(root), 0);
});

test('env: a new non-standard name without the prefix fails against the base register', t => {
  const root = repo(t);
  const previous = path.join(root, 'previous.json');
  const without = names => {
    const data = JSON.parse(readFileSync(registerFile(root), 'utf8'));
    data.entries = data.entries.filter(entry => !names.includes(entry.name));
    writeFileSync(previous, JSON.stringify(data));
  };
  const compare = expected => invoke('check-env', [...args(root), '--previous-register', previous], expected);
  without([]);
  compare(0);
  without(['EXAMPLE_BASE']);
  assert.match(compare(1).stderr, /EXAMPLE_BASE: new environment name without the IMPLUVIUM_ prefix and not a standard name/);
  // A prefixed name is fine.
  without(['IMPLUVIUM_EXAMPLE_DIR']);
  compare(0);
  // So is a standard name.
  register(root, data => { data.entries.find(entry => entry.name === 'EXAMPLE_BASE').standard = true; });
  without(['EXAMPLE_BASE']);
  compare(0);
  // A retired name that returns counts as new.
  register(root, data => { data.entries.find(entry => entry.name === 'EXAMPLE_BASE').standard = false; });
  const old = JSON.parse(readFileSync(registerFile(root), 'utf8'));
  old.entries.find(entry => entry.name === 'EXAMPLE_BASE').status = 'retired';
  writeFileSync(previous, JSON.stringify(old));
  assert.match(compare(1).stderr, /EXAMPLE_BASE: new environment name/);
});

test('env: planning identifiers in the register are rejected', t => {
  const root = repo(t);
  register(root, data => { data.entries[0].description = 'See ABC-123.'; });
  assert.match(invoke('check-env', args(root), 1).stderr, /EXAMPLE_URL: the register is public/);
});

for (const [label, change] of [
  ['unknown status', data => { data.entries[0].status = 'maybe'; }],
  ['duplicate name', data => { data.entries.push(structuredClone(data.entries[0])); }],
  ['rename without a target', data => { data.entries[0].fate = { action: 'rename', when: 'Later.' }; }],
  ['target without rename', data => { data.entries[0].fate = { action: 'keep', to: 'X', when: 'Later.' }; }],
  ['evidence without a consumer', data => { data.entries[0].evidence = 'consumer'; }],
  ['unknown field', data => { data.entries[0].note = 'x'; }],
  ['pattern prefix mismatch', data => { data.entries.find(entry => entry.pattern).pattern.prefix = 'OTHER_'; }],
  ['pattern with one line', data => { data.entries.find(entry => entry.pattern).pattern.snippet.pop(); }],
  ['lower-case name', data => { data.entries[0].name = 'example_url'; }],
  ['wrong version', data => { data.version = 2; }],
]) {
  test(`env: malformed register is an input error: ${label}`, t => {
    const root = repo(t);
    register(root, change);
    invoke('check-env', args(root), 2);
  });
}

test('env: unreadable input is an input error, never a pass', t => {
  const comment = repo(t);
  write(comment, lib, 'use std::env;\n/* unterminated\n');
  invoke('check-env', args(comment), 2);
  const group = repo(t);
  write(group, lib, 'use std::{env, fs;\n');
  invoke('check-env', args(group), 2);
  const malformed = repo(t);
  write(malformed, lib, 'use std::env::{var;\n');
  invoke('check-env', args(malformed), 2);
  const docker = repo(t);
  appendFileSync(path.join(docker, 'Dockerfile'), 'ARG ${EXAMPLE_X}\n');
  invoke('check-env', args(docker), 2);
  const missing = repo(t);
  invoke('check-env', [...args(missing), '--register', path.join(missing, 'tools/nothing.json')], 2);
});
