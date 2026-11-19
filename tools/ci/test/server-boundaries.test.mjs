// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { check, resolvedIssues, serverPackages, workflowIssues } from '../check-server-boundaries.mjs';
import { check as clippy } from '../clippy-ratchet.mjs';
import { check as countTests } from '../count-tests.mjs';
import { count as names } from '../check-names.mjs';
import { invoke, replace, temporary } from './helpers.mjs';

const repository = fileURLToPath(new URL('../../../', import.meta.url));
const read = file => readFileSync(path.join(repository, file), 'utf8');
function put(root, file, source) {
  const absolute = path.join(root, file);
  mkdirSync(path.dirname(absolute), { recursive: true });
  writeFileSync(absolute, source);
}
function fixture(t) {
  const root = temporary(t);
  put(root, 'Cargo.toml', `[workspace]\nresolver = "3"\nmembers = [${serverPackages.map(name => `"server/crates/${name}"`).join(', ')}]\ndefault-members = []\n`);
  for (const name of serverPackages) {
    const file = `server/crates/${name}/Cargo.toml`;
    put(root, file, read(file));
  }
  put(root, 'engine/crates/protocol/Cargo.toml', '[package]\nname = "impluvium-protocol"\n');
  for (const file of ['tools/ci/clippy-baseline.json', 'tools/ci/server-tests.json', 'tools/ci/server-dependencies.json', '.gitea/workflows/ci.yml']) put(root, file, read(file));
  return root;
}
const manifest = name => `server/crates/${name}/Cargo.toml`;
const append = (root, file, text) => put(root, file, readFileSync(path.join(root, file), 'utf8') + text);
const dependency = (root, owner, source, section = 'dependencies') => append(root, manifest(owner), `\n[${section}]\n${source}\n`);
function admit(root, owner, name, target) {
  const file = 'tools/ci/server-dependencies.json', data = JSON.parse(readFileSync(path.join(root, file)));
  data.packages[owner].push({ package: name, ...(target ? { path: target } : {}) });
  put(root, file, JSON.stringify(data));
}

test('the real workspace and synthetic composition pass the server boundary checker', t => {
  assert.deepEqual(check({ root: repository }), []);
  assert.deepEqual(check({ root: fixture(t) }), []);
});
test('the server scaffold adds no inherited product names', () => {
  // Only the one line of the schema fixture is exempt, exactly as in tools/ci/names-allow.json.
  const fixtureEntry = JSON.parse(readFileSync(path.join(repository, 'tools/ci/names-allow.json'), 'utf8')).entries
    .find(entry => entry.path === 'server/crates/server-db/tests/fixtures/legacy-baseline.sql');
  const allowed = [{ lines: fixtureEntry.lines, matches: name => name === 'server-db/tests/fixtures/legacy-baseline.sql' }];
  assert.deepEqual(names(path.join(repository, 'server/crates'), allowed), { result: {}, issues: [] });
});
for (const [owner, source, section] of [
  ['server-domain', 'axum = "0.8"', 'dependencies'],
  ['server-domain', 'web = { package = "tower", version = "0.5" }', 'dev-dependencies'],
  ['server-api-types', 'db = { package = "sqlx", version = "0.8", optional = true }', 'target.\'cfg(unix)\'.dependencies'],
  ['server-domain', 'sqlx = "0.8"', 'target.\'cfg(windows)\'.build-dependencies'],
  ['server-domain', 'reqwest = "0.13"', 'dependencies'],
  ['server-domain', 'actix-web = "4"', 'dependencies'],
  ['server-domain', 'web = { package = "actix-web", version = "4" }', 'build-dependencies'],
  ['server-domain', 'unknown-framework = "1"', 'target.\'cfg(unix)\'.dev-dependencies'],
]) test(`forbidden ${owner} ${section} edge makes the checker fail: ${source}`, t => {
  const root = fixture(t); dependency(root, owner, source, section);
  assert.match(check({ root }).join('\n'), /not admitted/);
});
test('server boundary CLI returns failure for an intentionally forbidden dependency', t => {
  const root = fixture(t);
  invoke('check-server-boundaries', ['--root', root], 0);
  dependency(root, 'server-domain', 'axum = "0.8"');
  assert.match(invoke('check-server-boundaries', ['--root', root], 1).stderr, /not admitted/);
});
test('framework ownership permits the HTTP and database adapters and separate development tools', t => {
  const root = fixture(t);
  // server-http already declares and is admitted for axum and tower.
  append(root, manifest('server-db'), 'sqlx = "0.8"\n');
  admit(root, 'server-db', 'sqlx');
  put(root, 'tools/server-xtask/Cargo.toml', '[package]\nname = "server-xtask"\n[dependencies]\nsqlx = "0.8"\n');
  assert.deepEqual(check({ root }), []);
  append(root, manifest('server-http'), 'sqlx = "0.8"\n');
  assert.match(check({ root }).join('\n'), /sqlx is not admitted/);
});
test('renamed workspace dependencies cannot hide a forbidden framework', t => {
  const root = fixture(t);
  append(root, 'Cargo.toml', '\n[workspace.dependencies]\nweb = { package = "axum", version = "0.8" }\n');
  dependency(root, 'server-domain', 'web = { workspace = true }');
  assert.match(check({ root }).join('\n'), /dependency axum is not admitted/);
});
test('a new local helper requires explicit admission', t => {
  const root = fixture(t);
  dependency(root, 'server-domain', 'helper = { path = "../../../helper" }');
  put(root, 'helper/Cargo.toml', '[package]\nname = "helper"\n[dependencies]\nsqlx = "0.8"\n');
  const issues = check({ root }).join('\n');
  assert.match(issues, /dependency helper is not admitted/);
});
test('engine direction follows renamed, target-specific and intermediate edges', t => {
  const root = fixture(t);
  put(root, 'engine/core/Cargo.toml', '[package]\nname = "core"\n[target.\'cfg(windows)\'.dev-dependencies]\nhelper = { path = "../../helper" }\n');
  put(root, 'helper/Cargo.toml', '[package]\nname = "helper"\n[dependencies]\napplication = { package = "server-domain", path = "../server/crates/server-domain" }\n');
  assert.match(check({ root }).join('\n'), /engine dependencies must not point to the server/);
});
test('server to engine edges are allowed', t => {
  const root = fixture(t);
  put(root, 'engine/core/Cargo.toml', '[package]\nname = "core"\n');
  append(root, manifest('server-http'), 'core = { path = "../../../engine/core" }\n');
  admit(root, 'server-http', 'core', 'engine/core/Cargo.toml');
  assert.deepEqual(check({ root }), []);
});
test('admitted local composition permits aliases and workspace inheritance', t => {
  const root = fixture(t);
  replace(root, manifest('server-bin'), 'server-http = { path = "../server-http", version = "=0.1.0" }',
    'adapter = { workspace = true }');
  append(root, 'Cargo.toml', '\n[workspace.dependencies]\nadapter = { package = "server-http", path = "server/crates/server-http" }\n');
  assert.deepEqual(check({ root }), []);
});
test('a registry namesake cannot reuse admission for a local dependency', t => {
  const root = fixture(t);
  append(root, manifest('server-bin'), 'other = { package = "server-http", version = "0.1" }\n');
  assert.match(check({ root }).join('\n'), /dependency server-http is not admitted/);
});
test('the server layers cannot be reversed or reduced to dev-only composition', t => {
  const root = fixture(t);
  dependency(root, 'server-domain', 'adapter = { package = "server-http", path = "../server-http" }');
  assert.match(check({ root }).join('\n'), /forbidden server layer/);
  const domain = 'server-domain = { path = "../server-domain", version = "=0.1.0" }\n';
  replace(root, manifest('server-db'), domain, '');
  replace(root, manifest('server-db'), '[dev-dependencies]\n', `[dev-dependencies]\n${domain}`);
  assert.match(check({ root }).join('\n'), /missing product dependency on server-domain/);
});
test('missing workspace members, default expansion and positive warning budgets fail', t => {
  const root = fixture(t);
  replace(root, 'Cargo.toml', '"server/crates/server-bin"', '"other"');
  replace(root, 'Cargo.toml', 'default-members = []', 'default-members = ["server/crates/server-http"]');
  replace(root, 'tools/ci/clippy-baseline.json', '"server-domain": 0', '"server-domain": 1');
  const issues = check({ root }).join('\n');
  assert.match(issues, /missing named server workspace member/);
  assert.match(issues, /not added to default-members/);
  assert.match(issues, /requires a zero warning budget/);
});
for (const [before, after, message] of [
  ['-p server-api-types', '', /cargo fmt: expected exactly/],
  ['--package server-api-types', '', /clippy-ratchet: expected exactly/],
  ['--package server-bin', '--package unknown', /clippy-ratchet: expected exactly/],
  ['--exclude impluvium-protocol', '--exclude server-domain', /clippy-ratchet: expected exactly/],
  ['--exclude impluvium-protocol', '--exclude impluvium-client', /clippy-ratchet: expected exactly/],
  ['-p server-bin \\\n            -- -D warnings', '-p server-bin --exclude impluvium-protocol \\\n            -- -D warnings', /cargo clippy: expected exactly/],
  ['tools/ci/server-tests.json "${previous[@]}"', 'tools/ci/test-baseline.json "${previous[@]}"', /count-tests: incorrect baseline/],
  ['-- -D warnings', '-- --cap-lints warn', /must deny warnings/],
  ['run: node tools/ci/check-server-boundaries.mjs --root .', 'run: echo skipped', /must run the server boundary checker/],
  ['--root . --metadata "$RUNNER_TEMP/server-metadata.json"', '--root .', /must capture locked offline metadata/],
  ['cargo metadata --locked --offline --format-version=1 > "$RUNNER_TEMP/server-metadata.json"',
    'cargo metadata --locked --format-version=1 > "$RUNNER_TEMP/server-metadata.json"', /must capture locked offline metadata/],
]) test(`workflow counterexample: ${before}`, t => {
  const root = fixture(t), file = '.gitea/workflows/ci.yml';
  replace(root, file, before, after);
  assert.match(workflowIssues(path.join(root, file)).join('\n'), message);
});

function resolvedFixture(t) {
  const root = fixture(t), file = path.join(root, 'resolved.json');
  const data = { packages: [], workspace_members: [], resolve: { nodes: [] } };
  const add = (name, features = []) => {
    const id = `opaque:${data.packages.length}`;
    data.packages.push({ id, name, manifest_path: path.join(root,
      serverPackages.includes(name) ? manifest(name) : `registry/${name}/Cargo.toml`) });
    data.resolve.nodes.push({ id, dependencies: [], deps: [], features });
    if (serverPackages.includes(name)) data.workspace_members.push(id);
    return id;
  };
  const ids = Object.fromEntries(serverPackages.map(name => [name, add(name)]));
  const edge = (from, to, kind = null, target = null) => {
    const node = data.resolve.nodes.find(node => node.id === from);
    if (!node.dependencies.includes(to)) node.dependencies.push(to);
    node.deps.push({ name: `renamed_${node.deps.length}`, pkg: to, dep_kinds: [{ kind, target }] });
  };
  edge(ids['server-bin'], ids['server-http']);
  edge(ids['server-bin'], ids['server-db']);
  edge(ids['server-http'], ids['server-domain']);
  edge(ids['server-http'], ids['server-api-types']);
  edge(ids['server-db'], ids['server-domain']);
  const write = () => put(root, 'resolved.json', JSON.stringify(data));
  const run = () => { write(); return resolvedIssues(file, root); };
  return { root, file, data, ids, add, edge, write, run };
}
test('actix-web in the domain fails closed admission with resolved metadata supplied', t => {
  const f = resolvedFixture(t);
  dependency(f.root, 'server-domain', 'actix-web = "4"');
  f.edge(f.ids['server-domain'], f.add('actix-web')); f.write();
  assert.match(check({ root: f.root, metadata: f.file }).join('\n'), /dependency actix-web is not admitted/);
});
test('admitted sea-orm in HTTP still fails when normal registry edges reach sqlx', t => {
  const f = resolvedFixture(t);
  append(f.root, manifest('server-http'), 'sea-orm = { version = "1.1", default-features = false, features = ["sqlx-postgres", "runtime-tokio-rustls"] }\n');
  admit(f.root, 'server-http', 'sea-orm');
  const orm = f.add('sea-orm'), sqlx = f.add('sqlx');
  f.edge(f.ids['server-http'], orm); f.edge(orm, sqlx); f.edge(sqlx, f.add('sqlx-postgres')); f.write();
  assert.match(check({ root: f.root, metadata: f.file }).join('\n'), /server-http: sqlx is reachable.*server-http -> sea-orm -> sqlx/);
});
test('resolved ownership permits SQL only from the database and HTTP frameworks from HTTP', t => {
  const f = resolvedFixture(t);
  f.edge(f.ids['server-db'], f.add('sqlx'));
  for (const name of ['axum', 'tower', 'hyper', 'hyper-util']) f.edge(f.ids['server-http'], f.add(name, ['server']));
  assert.deepEqual(f.run(), []);
});
test('the binary reaches sqlx and its family through the database crate', t => {
  const f = resolvedFixture(t), sqlx = f.add('sqlx');
  f.edge(f.ids['server-db'], sqlx); f.edge(sqlx, f.add('sqlx-core'));
  assert.deepEqual(f.run(), []);
});
test('a direct binary edge to sqlx fails even when the database crate reaches it first', t => {
  const f = resolvedFixture(t), sqlx = f.add('sqlx');
  f.edge(f.ids['server-db'], sqlx); f.edge(f.ids['server-bin'], sqlx);
  assert.match(f.run().join('\n'), /server-bin: sqlx is reachable.*server-bin -> sqlx/);
  const g = resolvedFixture(t), other = g.add('sqlx-postgres');
  g.edge(g.ids['server-db'], other); g.edge(g.ids['server-bin'], other);
  assert.match(g.run().join('\n'), /server-bin: sqlx is reachable.*server-bin -> sqlx-postgres/);
});
test('sqlx through another package fails even when the database crate also reaches it', t => {
  const f = resolvedFixture(t), sqlx = f.add('sqlx'), helper = f.add('helper');
  f.edge(f.ids['server-db'], sqlx); f.edge(f.ids['server-http'], helper); f.edge(helper, sqlx);
  const issues = f.run().join('\n');
  assert.match(issues, /server-http: sqlx is reachable.*server-http -> helper -> sqlx/);
  assert.match(issues, /server-bin: sqlx is reachable.*server-http -> helper -> sqlx/);
});
for (const name of ['axum', 'tower', 'hyper', 'hyper-util']) test(`resolved direct ${name} server dependency outside HTTP fails`, t => {
  const f = resolvedFixture(t);
  f.edge(f.ids['server-domain'], f.add(name, ['server']));
  assert.match(f.run().join('\n'), /only server-http may directly depend/);
});
test('resolved hyper without its server feature is permitted', t => {
  const f = resolvedFixture(t);
  f.edge(f.ids['server-bin'], f.add('hyper', ['client']));
  assert.deepEqual(f.run(), []);
});
for (const kind of ['build', 'dev']) test(`resolved sqlx through ${kind} dependencies is outside the normal product graph`, t => {
  const f = resolvedFixture(t), helper = f.add('helper');
  f.edge(f.ids['server-http'], helper, kind); f.edge(helper, f.add('sqlx-core'));
  assert.deepEqual(f.run(), []);
});
test('normal target-specific sqlx family edges fail even without the sqlx facade', t => {
  const f = resolvedFixture(t);
  f.edge(f.ids['server-http'], f.add('sqlx-postgres'), null, 'cfg(windows)');
  assert.match(f.run().join('\n'), /sqlx is reachable/);
});
test('resolved input fails closed on missing roots, dep_kinds, features or hidden edges', t => {
  for (const mutate of [
    f => { f.data.workspace_members.pop(); },
    f => { f.data.resolve.nodes.find(node => node.id === f.ids['server-http']).deps[0].dep_kinds = []; },
    f => { delete f.data.resolve.nodes[0].features; },
    f => { f.data.resolve.nodes.find(node => node.id === f.ids['server-http']).deps = []; },
  ]) {
    const f = resolvedFixture(t); mutate(f);
    assert.throws(f.run, /missing server workspace root|complete resolved deps/);
  }
});
test('dependency admission fails closed on a missing crate or malformed entry', t => {
  for (const mutate of [
    data => { delete data.packages['server-domain']; },
    data => { data.packages['server-domain'] = ['actix-web']; },
  ]) {
    const root = fixture(t), file = 'tools/ci/server-dependencies.json';
    const data = JSON.parse(readFileSync(path.join(root, file))); mutate(data); put(root, file, JSON.stringify(data));
    assert.throws(() => check({ root }), /expected dependency admission|valid entries/);
  }
});

function reports(t) {
  const root = fixture(t), metadata = path.join(root, 'metadata.json'), input = path.join(root, 'report');
  const packages = serverPackages.map(name => ({ id: name, name, targets: [{
    name: name === 'server-bin' ? 'impluvium-server' : name.replaceAll('-', '_'),
    kind: [name === 'server-bin' ? 'bin' : 'lib'],
  }] }));
  writeFileSync(metadata, JSON.stringify({ packages, workspace_members: serverPackages }));
  return { root, metadata, input, packages, package: serverPackages };
}
const artifacts = names => [...names.map(package_id => ({ reason: 'compiler-artifact', package_id })), { reason: 'build-finished', success: true }];
const jsonl = items => items.map(item => JSON.stringify(item)).join('\n');
const zeroSuite = target => `     Running unittests src/lib.rs (target/debug/deps/${target}-0123456789abcdef)\nrunning 0 tests\ntest result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n`;

test('server Clippy reports require all selected artifacts, baseline entries and zero diagnostics', t => {
  const f = reports(t), options = { ...f, baseline: path.join(f.root, 'tools/ci/clippy-baseline.json') };
  writeFileSync(f.input, jsonl(artifacts(serverPackages)));
  assert.deepEqual(clippy(options), []);
  assert.match(clippy({ ...options, package: serverPackages.slice(1) }).join('\n'), /outside --package selection/);
  writeFileSync(f.input, jsonl(artifacts(serverPackages.slice(1))));
  assert.match(clippy(options).join('\n'), /server-api-types: count missing/);
  writeFileSync(f.input, jsonl([{ reason: 'compiler-message', package_id: 'server-domain',
    message: { level: 'warning', code: { code: 'clippy::example' } } }, ...artifacts(serverPackages)]));
  assert.match(clippy(options).join('\n'), /exceeds baseline 0/);
  const baseline = JSON.parse(readFileSync(options.baseline)); delete baseline.packages['server-domain'];
  writeFileSync(options.baseline, JSON.stringify(baseline));
  assert.match(clippy(options).join('\n'), /missing from baseline/);
});
test('server test reports require zero suites to be observed and reject missing entries and ambiguous names', t => {
  const f = reports(t), options = { ...f, input: [f.input], baseline: path.join(f.root, 'tools/ci/server-tests.json') };
  // The fixture copies the repository's baseline; pin every package to zero so the case does not depend on real counts.
  const zero = JSON.parse(readFileSync(options.baseline));
  for (const name of Object.keys(zero.packages)) zero.packages[name] = { tests: { exact: 0 }, docTests: { exact: 0 } };
  writeFileSync(options.baseline, JSON.stringify(zero));
  const suites = f.packages.map(pkg => zeroSuite(pkg.targets[0].name));
  writeFileSync(f.input, suites.join(''));
  assert.deepEqual(countTests(options), []);
  assert.match(countTests({ ...options, package: serverPackages.slice(1) }).join('\n'), /outside --package selection/);
  writeFileSync(f.input, suites.slice(1).join(''));
  assert.match(countTests(options).join('\n'), /server-api-types: package missing/);
  const baseline = JSON.parse(readFileSync(options.baseline)); delete baseline.packages['server-domain'];
  writeFileSync(options.baseline, JSON.stringify(baseline));
  assert.match(countTests(options).join('\n'), /server-domain: observed package missing from baseline/);
  f.packages[1].targets.push({ name: f.packages[0].targets[0].name, kind: ['test'] });
  writeFileSync(f.metadata, JSON.stringify({ packages: f.packages, workspace_members: serverPackages }));
  writeFileSync(f.input, suites.join(''));
  assert.throws(() => countTests(options), /cannot uniquely attribute tests target/);
});
