import test from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { check, checkDiagnostics, productIssues, legacyIssues } from '../check-protocol-graph.mjs';
import { evidence } from './protocol-fixture.mjs';
import { parsePackages, permissionCandidate, toolCalls, unusedTools } from '../protocol-evidence.mjs';
import { linux, windows, selection } from '../protocol-contract.mjs';
import { gitObject } from '../protocol-source.mjs';
import { requireNegativeDiagnostic } from '../protocol-negative.mjs';

test('protocol graph accepts complete consistent raw and normalized native evidence', t => assert.deepEqual(check(evidence(t).options), []));
test('protocol graph accepts verified Windows tools and actual target/CRT/linker evidence', t => assert.deepEqual(check(evidence(t, windows).options), []));
for (const relative of ['tests/target/main.rs', 'src/bin/dist/main.rs']) test(`old complete evidence rejects a newly discovered Rust target ${relative}`, t => {
  const value = evidence(t), file = path.join(value.root, 'engine/crates/protocol', relative);
  // Cargo discovers tests/<name>/main.rs and src/bin/<name>/main.rs without
  // explicit target entries in the package manifest.
  mkdirSync(path.dirname(file), { recursive: true }); writeFileSync(file, 'compile_error!("unattested");');
  assert.throws(() => check(value.options), /unattested source input/);
});
test('same source tree with a different expected commit rejects earlier product evidence', t => {
  const value = evidence(t), raw = Buffer.from(value.invocation.source.commitObject, 'base64');
  const other = gitObject('commit', Buffer.concat([raw, Buffer.from('Another commit message\n')]));
  assert.notEqual(other, value.invocation.sourceSha);
  assert.throws(() => check({ ...value.options, 'source-sha': other }), /source commit differs from expected/);
  const missing = { ...value.options }; delete missing['source-sha'];
  assert.throws(() => check(missing), /source-sha is required/);
});
test('a misleading contract path cannot replace a missing structured rejection in complete evidence', t => {
  const value = evidence(t), file = path.join(value.input, 'forbidden feature rustls/ring/contract.json');
  mkdirSync(path.dirname(file), { recursive: true });
  value.data.permissions[0].packages[0].features.push('unobserved');
  writeFileSync(file, JSON.stringify(value.data));
  const options = { ...value.options, contract: file }, diagnostics = checkDiagnostics(options);
  assert.ok(diagnostics.some(issue => issue.code === 'policy'));
  assert.match(check(options).join('\n'), /forbidden feature rustls\/ring/);
  assert.throws(() => requireNegativeDiagnostic('ring', diagnostics), /intended rejection/);
});
for (const role of ['CC', 'AR']) test(`C package cannot hide missing ${role} commands as an unused tool`, t => {
  const value = evidence(t), file = path.join(value.input, 'build.log');
  const lines = readFileSync(file, 'utf8').split('\n');
  const index = lines.findIndex(line => line.includes(value.tools.launchers[`target${role}`]));
  lines.splice(index, 2);
  writeFileSync(file, lines.join('\n'));
  value.tools.calls = toolCalls(lines.join('\n'), readFileSync(path.join(value.input, 'cargo.stdout'), 'utf8'), value.tools, value.metadata, value.root, linux);
  value.tools.unused = unusedTools(value.tools, value.tools.calls, value.tools.linkerCalls);
  value.save('tools.json', value.tools); value.refresh();
  assert.throws(() => check(value.options), new RegExp(`missing actual ${role}`));
});
for (const [name, mutate, expected] of [
  ['missing static CRT', value => { value.invocation.environment.CARGO_TARGET_X86_64_PC_WINDOWS_GNULLVM_RUSTFLAGS = null; value.refresh(); }, /CRT/],
  ['different archive pin', value => { value.tools.windows.pins.LLVM_MINGW_SHA256 = '0'.repeat(64); value.save('tools.json', value.tools); value.refresh(); }, /pin/],
  ['different installed compiler wrapper', value => { writeFileSync(value.tools.executables.targetCC.path, 'changed'); }, /inner tool|installation/],
  ['different inner compiler behind unchanged aliases', value => { writeFileSync(path.join(value.tools.windows.root, 'bin/clang-23'), 'changed'); }, /inner tool|installation/],
  ['different tool version', value => { value.tools.executables.targetCC.version = 'clang version 99.0.0'; value.save('tools.json', value.tools); value.refresh(); }, /version differs/],
  ['version with a matching prefix only', value => { value.tools.executables.targetCC.version = 'clang version 23.1.40'; value.save('tools.json', value.tools); value.refresh(); }, /version differs/],
]) test(`Windows graph rejects ${name}`, t => { const value = evidence(t, windows); mutate(value); assert.throws(() => check(value.options), expected); });
for (const [name, mutate, reason] of [
  ['empty permissions', value => { value.data.permissions = []; writeFileSync(value.contractFile, JSON.stringify(value.data)); }, /permissions/],
  ['incomplete capture', value => { value.save('capture-status.json', { version: 1, status: 'failed' }); }, /did not complete/],
  ['unresolved diagnostics', value => { value.save('diagnostics.json', { version: 1, errors: [{ file: 'packages.tree', line: 2, message: 'invalid row' }] }); }, /diagnostics/],
  ['wrong tree byte count', value => { value.invocation.treeFiles['packages.tree'].bytes++; value.save('invocation.json', value.invocation); }, /size\/hash/],
  ['missing feature permission', value => { delete value.data.permissions[0].packages[0].features; writeFileSync(value.contractFile, JSON.stringify(value.data)); }, /permission/],
  ['foreign profile build evidence', value => { value.invocation.profile = 'server'; value.refresh(); }, /arguments differ/],
  ['wrong target', value => { value.invocation.target = 'x86_64-pc-windows-gnu'; value.refresh(); }, /unknown profile, target/],
  ['debug output disabled', value => { value.invocation.environment.CC_ENABLE_DEBUG_OUTPUT = '0'; value.refresh(); }, /environment/],
  ['ambient Rust flags', value => { value.invocation.environment.RUSTFLAGS = '-Ctarget-feature=+crt-static'; value.refresh(); }, /environment/],
  ['changed lock', value => { writeFileSync(path.join(value.root, 'Cargo.lock'), 'changed'); }, /lock/],
  ['truncated stdout', value => { writeFileSync(path.join(value.input, 'cargo.stdout'), ''); value.refresh(); }, /build-finished/],
  ['raw tool evidence removed', value => { writeFileSync(path.join(value.input, 'build.log'), ''); value.refresh(); }, /commands differ/],
  ['claimed tool call without raw evidence', value => { value.tools.calls.push({ roles: ['targetCXX'] }); value.save('tools.json', value.tools); value.refresh(); }, /commands differ/],
  ['changed normalized graph', value => { value.graph.nodes.pop(); value.save('graph.json', value.graph); value.refresh(); }, /normalized graph/],
  ['wrong artifact features', value => {
    const lines = readFileSync(path.join(value.input, 'cargo.stdout'), 'utf8').trim().split('\n').map(line => JSON.parse(line));
    lines[0].features = ['server'];
    const text = lines.map(line => JSON.stringify(line)).join('\n') + '\n';
    writeFileSync(path.join(value.input, 'cargo.stdout'), text); writeFileSync(path.join(value.input, 'cargo.jsonl'), text); value.refresh();
  }, /artifact features/],
  ['unused tool misreported', value => { value.tools.unused = []; value.save('tools.json', value.tools); value.refresh(); }, /unused/],
  ['foreign linker', value => {
    value.tools.linkerCalls[0].executable = value.tools.executables.hostCC.path;
    writeFileSync(path.join(value.input, 'linker.jsonl'), JSON.stringify(value.tools.linkerCalls[0]) + '\n');
    value.save('tools.json', value.tools); value.refresh();
  }, /linker/],
  ['linker role changed in configuration', value => {
    const file = path.join(value.input, 'target-linker.mjs.json');
    const config = JSON.parse(readFileSync(file, 'utf8')); config.context = 'host';
    writeFileSync(file, JSON.stringify(config));
  }, /linker delegation/],
  ['linker launcher differs from its claimed role', value => {
    value.tools.linkerCalls[0].launcher = path.join(value.input, 'host-linker.mjs');
    writeFileSync(path.join(value.input, 'linker.jsonl'), JSON.stringify(value.tools.linkerCalls[0]) + '\n');
    value.save('tools.json', value.tools); value.refresh();
  }, /linker invocation/],
]) test(`protocol graph rejects ${name}`, t => { const value = evidence(t); mutate(value); assert.throws(() => check(value.options), reason); });

for (const [name, feature, expected] of [
  ['connectrpc', 'gzip', 'connectrpc/gzip'], ['connectrpc', 'zstd', 'connectrpc/zstd'],
  ['reqwest', 'system-proxy', 'reqwest/system-proxy'], ['connectrpc', 'client-tls', 'connectrpc/client-tls'],
  ['rustls', 'ring', 'rustls/ring'], ['hyper-rustls', 'native-tokio', 'hyper-rustls/native-tokio'],
]) test(`protocol graph rejects ${expected} even in a permissive licensed graph`, t => {
  const value = evidence(t), id = `mutation:${name}`;
  value.metadata.packages.push({ id, name, version: value.data.versions[name], source: 'registry', manifest_path: '/registry/manifest' });
  value.graph.nodes.push({ id, features: [feature] });
  value.data.permissions = [permissionCandidate(value.graph, value.metadata, value.invocation)];
  assert.ok(productIssues(value.data, value.graph, value.metadata, value.invocation).some(issue => issue.includes(expected)));
});
for (const [name, feature] of [['reqwest', 'system-proxy'], ['rustls', 'ring']]) {
  test(`${name}/${feature} is rejected from raw resolved features and matching artifacts with an unchanged source and lock`, t => {
    const value = evidence(t), id = `resolved:${name}`, file = path.join(value.input, 'packages.tree');
    const lock = readFileSync(path.join(value.root, 'Cargo.lock'));
    const source = JSON.stringify(value.invocation.source);
    value.metadata.packages.push({ id, name, version: value.data.versions[name], source: 'registry',
      manifest_path: path.join(value.root, '.registry', name, 'Cargo.toml'),
      targets: [{ name, kind: ['lib'] }], dependencies: [] });
    value.metadata.resolve.nodes.push({ id, features: [feature], dependencies: [], deps: [] });
    const raw = readFileSync(file, 'utf8') + `${name} v${value.data.versions[name]}|${feature}\n`;
    writeFileSync(file, raw);
    const graph = parsePackages(raw, value.metadata, value.invocation);
    value.save('metadata.json', value.metadata); value.save('graph.json', graph);
    const records = readFileSync(path.join(value.input, 'cargo.stdout'), 'utf8').trim().split('\n').map(line => JSON.parse(line));
    records.splice(-1, 0, { reason: 'compiler-artifact', package_id: id, target: { kind: ['lib'] }, features: [feature],
      filenames: [path.join(value.invocation.targetDir, linux, 'debug/deps', name + '.rlib')], executable: null });
    const saveStream = () => {
      const bytes = records.map(record => JSON.stringify(record)).join('\n') + '\n';
      writeFileSync(path.join(value.input, 'cargo.stdout'), bytes); writeFileSync(path.join(value.input, 'cargo.jsonl'), bytes);
      value.refresh();
    };
    // Even permissions admitting the tuple cannot override the feature ban.
    value.data.permissions = [permissionCandidate(graph, value.metadata, value.invocation)];
    writeFileSync(value.contractFile, JSON.stringify(value.data)); saveStream();
    assert.ok(check(value.options).some(issue => issue.endsWith(`forbidden feature ${name}/${feature}`)));
    assert.deepEqual(readFileSync(path.join(value.root, 'Cargo.lock')), lock);
    assert.equal(JSON.stringify(value.invocation.source), source);
    records.at(-2).features = []; saveStream();
    assert.throws(() => check(value.options), /artifact features do not match graph/);
  });
}
for (const name of ['ring', 'native-tls', 'openssl', 'prost', 'flate2', 'zstd']) test(`protocol graph rejects package ${name} independently of licence`, t => {
  const value = evidence(t), id = `mutation:${name}`;
  value.metadata.packages.push({ id, name, version: '1.0.0', source: 'registry', manifest_path: '/registry/manifest' });
  value.graph.nodes.push({ id, features: [] });
  value.data.permissions = [permissionCandidate(value.graph, value.metadata, value.invocation)];
  assert.ok(productIssues(value.data, value.graph, value.metadata, value.invocation).some(issue => issue.includes(`forbidden package ${name}`)));
});
test('legacy paths are complete, frozen, versioned and never transferable to engine roots', t => {
  const value = evidence(t), row = { root: 'desktop/client/Cargo.toml', path: [{ name: 'ring', version: '1.0.0' }], target: 'all', removalOwner: 'desktop client migration' };
  const requirements = [['desktop/client/Cargo.toml', 'feature', 'default', []]];
  const data = { legacy: { baseSha: 'b'.repeat(40), exceptions: [row], requirements } };
  assert.deepEqual(legacyIssues(data, { version: 3, scope: 'all-targets-conservative', baseSha: data.legacy.baseSha, paths: [row], requirements }), []);
  assert.ok(legacyIssues(data, { version: 3, scope: 'all-targets-conservative', baseSha: data.legacy.baseSha, paths: [{ ...row, path: [{ name: 'ring', version: '2.0.0' }] }], requirements }).length);
  assert.throws(() => legacyIssues({ legacy: { baseSha: null, exceptions: null } }, {}), /reviewed base/);
  assert.throws(() => legacyIssues({ legacy: { baseSha: data.legacy.baseSha, exceptions: [{ ...row, root: 'engine/core/Cargo.toml' }] } }, { version: 3, scope: 'all-targets-conservative', baseSha: data.legacy.baseSha, paths: [] }), /engine legacy exception/);
  const id = 'legacy:ring';
  value.metadata.packages.push({ id, name: 'ring', version: '1.0.0', source: 'registry', manifest_path: '/registry/ring' });
  value.graph.nodes.push({ id, features: [] });
  value.data.legacy = data.legacy;
  value.data.permissions = [permissionCandidate(value.graph, value.metadata, value.invocation)];
  assert.ok(productIssues(value.data, value.graph, value.metadata, value.invocation).some(issue => issue.includes('ring')));
});
test('all four profiles have separate no-default-feature selections for both targets', t => {
  const value = evidence(t);
  for (const profile of ['core', 'server', 'client', 'combined']) for (const target of [linux, windows]) {
    const args = selection(value.data, profile, target, 'build', '/fresh/target');
    assert.ok(args.cargo.includes('--no-default-features'));
    assert.ok(args.cargo.includes(target));
    assert.equal(args.packages[args.packages.indexOf('--prefix') + 1], 'none');
    assert.equal(args.packages[args.packages.indexOf('--edges') + 1], 'normal,build');
    assert.ok(args.packages.includes('--no-dedupe'));
    assert.equal(Object.hasOwn(args, 'features'), false);
    assert.equal(args.metadata.includes('--filter-platform'), false);
    for (const run of ['test', 'clippy']) {
      const calls = selection(value.data, profile, target, run, '/fresh/target');
      assert.equal(calls.packages[calls.packages.indexOf('--edges') + 1], 'normal,build,dev');
    }
    for (const name of value.data.profiles[profile].roots) assert.ok(args.cargo.includes(name));
  }
});

for (const profile of ['core', 'server', 'client', 'combined']) for (const target of [linux, windows]) {
  test(`product policy accepts a complete permissive selection: ${profile}/${target}`, t => {
    const value = evidence(t, target);
    value.invocation.profile = profile;
    const client = ['client', 'combined'].includes(profile), server = ['server', 'combined'].includes(profile);
    value.graph.nodes.find(node => node.id === 'root').features = [...(client ? ['client'] : []), ...(server ? ['server'] : [])];
    const features = {
      connectrpc: ['json', ...(server ? ['axum'] : []), ...(client ? ['client'] : [])],
      reqwest: ['rustls', 'http2', 'stream', 'json'], rustls: ['std', 'tls12', 'aws-lc-rs'],
      'hyper-rustls': ['http1', 'http2', 'tls12', 'aws-lc-rs'], 'hyper-util': ['client-legacy', 'http1', 'http2', 'tokio'],
    };
    const names = profile === 'core' ? [] : ['connectrpc', ...(['client', 'combined'].includes(profile) ?
      ['reqwest', 'hyper-rustls', 'rustls', 'hyper-util', 'aws-lc-rs', 'aws-lc-sys', 'rustls-platform-verifier'] : [])];
    for (const name of names) {
      const id = `synthetic:${name}`;
      value.metadata.packages.push({ id, name, version: value.data.versions[name], source: 'registry', manifest_path: '/registry/' + name });
      value.graph.nodes.push({ id, features: features[name] ?? [] });
    }
    value.data.permissions = [permissionCandidate(value.graph, value.metadata, value.invocation)];
    assert.deepEqual(productIssues(value.data, value.graph, value.metadata, value.invocation), []);
  });
}

test('observed package-feature rows must be a subset of reviewed complete rows', t => {
  const value = evidence(t), extra = { ...value.data.permissions[0].packages[0], features: ['future-reviewed-feature'] };
  value.data.permissions[0].packages.push(extra);
  assert.deepEqual(productIssues(value.data, value.graph, value.metadata, value.invocation), []);
  const permitted = value.data.permissions[0].packages.find(pkg => pkg.name === 'buffa');
  permitted.features = ['std'];
  assert.ok(productIssues(value.data, value.graph, value.metadata, value.invocation).some(issue => issue.includes('outside the reviewed')));
});

test('a changed runtime version fails even if someone adds it to the permission list', t => {
  const value = evidence(t);
  value.metadata.packages.find(pkg => pkg.id === 'buffa').version = '0.9.3';
  value.data.permissions = [permissionCandidate(value.graph, value.metadata, value.invocation)];
  assert.ok(productIssues(value.data, value.graph, value.metadata, value.invocation).some(issue => issue.includes('runtime pin differs')));
});

test('host-only artifacts cannot satisfy target runtime requirements', t => {
  const value = evidence(t), artifacts = value.graph.nodes.map(node => ({ package_id: node.id, features: node.features,
    target: { kind: ['lib'] }, filenames: [path.join(value.invocation.targetDir, ...(node.id === 'buffa' ? [] : [linux]), 'debug/deps', node.id + '.rlib')] }));
  assert.ok(productIssues(value.data, value.graph, value.metadata, value.invocation, artifacts).some(issue => issue.includes('required runtime package missing: buffa')));
});

test('a triple in the capture ancestor cannot turn host artifacts into target evidence', t => {
  const value = evidence(t, windows);
  // Source: Windows linker.jsonl for all four profiles places host build
  // products below results/profile/<triple>/build/target/debug, while
  // target artifacts live below target/<triple>/debug.
  value.invocation.targetDir = path.join(value.input, windows, 'build/target');
  const artifacts = value.graph.nodes.map(node => ({ package_id: node.id, features: node.features,
    target: { kind: ['lib'] }, filenames: [path.join(value.invocation.targetDir,
      ...(node.id === 'buffa' ? [] : [windows]), 'debug/deps', node.id + '.rlib')] }));
  assert.ok(productIssues(value.data, value.graph, value.metadata, value.invocation, artifacts)
    .some(issue => issue.includes('required runtime package missing: buffa')));
});

test('the featureless core root still requires a target artifact through the complete checker', t => {
  const value = evidence(t, windows), file = path.join(value.input, 'cargo.stdout');
  const records = readFileSync(file, 'utf8').trim().split('\n').map(JSON.parse);
  records.find(record => record.package_id === 'root').filenames = [path.join(value.invocation.targetDir, 'debug/deps/root.rlib')];
  const text = records.map(record => JSON.stringify(record)).join('\n') + '\n';
  writeFileSync(file, text); writeFileSync(path.join(value.input, 'cargo.jsonl'), text); value.refresh();
  assert.match(check(value.options).join('\n'), /selected root has no target artifact: root/);
});
for (const profile of ['client', 'combined']) test(`${profile} requires every selected root including impluvium-client on the target`, t => {
  const value = evidence(t, windows), client = { ...value.metadata.packages[0], id: 'client-root', name: 'impluvium-client' };
  value.metadata.packages.push(client); value.graph.nodes.push({ id: client.id, features: [] });
  value.invocation.profile = profile; value.invocation.rootIds.push(client.id);
  value.data.permissions = [permissionCandidate(value.graph, value.metadata, value.invocation)];
  const artifacts = value.graph.nodes.map(node => ({ package_id: node.id, target: { kind: ['lib'] }, features: node.features,
    filenames: [path.join(value.invocation.targetDir, ...(node.id === client.id ? [] : [windows]), 'debug/deps', node.id + '.rlib')] }));
  assert.ok(productIssues(value.data, value.graph, value.metadata, value.invocation, artifacts).some(issue => issue === 'selected root has no target artifact: client-root'));
});

test('complete checker rejects source or manifest changes without a lock change', t => {
  const value = evidence(t);
  writeFileSync(path.join(value.root, 'rust-toolchain.toml'), '[toolchain]\nchannel="changed"\n');
  assert.throws(() => check(value.options), /source content differs/);
});

test('a forbidden system-library adapter reaches policy rejection without inventing C compilation', t => {
  const value = evidence(t), id = 'ssl';
  value.metadata.packages.push({ ...value.metadata.packages[1], id, name: 'openssl-sys', version: '0.9.117',
    manifest_path: '/registry/openssl-sys/Cargo.toml', dependencies: [{ name: 'cc', kind: 'build' }] });
  value.graph.nodes.push({ id, features: [] });
  value.graph.nodes.sort((a, b) => JSON.stringify(a).localeCompare(JSON.stringify(b)));
  value.save('metadata.json', value.metadata); value.save('graph.json', value.graph);
  const tree = path.join(value.input, 'packages.tree');
  writeFileSync(tree, readFileSync(tree, 'utf8') + 'openssl-sys v0.9.117|\n');
  const records = readFileSync(path.join(value.input, 'cargo.stdout'), 'utf8').trim().split('\n').map(JSON.parse);
  records.splice(records.length - 1, 0, { reason: 'compiler-artifact', package_id: id, target: { kind: ['lib'] }, features: [],
    filenames: [path.join(value.invocation.targetDir, linux, 'debug/deps/ssl.rlib')], executable: null });
  const text = records.map(record => JSON.stringify(record)).join('\n') + '\n';
  writeFileSync(path.join(value.input, 'cargo.stdout'), text); writeFileSync(path.join(value.input, 'cargo.jsonl'), text);
  value.data.permissions = [permissionCandidate(value.graph, value.metadata, value.invocation)];
  writeFileSync(value.contractFile, JSON.stringify(value.data)); value.refresh();
  assert.match(check(value.options).join('\n'), /forbidden package openssl-sys/);
});
