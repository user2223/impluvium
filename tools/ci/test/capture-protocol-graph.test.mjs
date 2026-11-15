import test from 'node:test';
import assert from 'node:assert/strict';
import { parsePackages, cargoStream, rustCommands, testTranscript, toolCalls, words, identity, unusedTools } from '../protocol-evidence.mjs';
import { testCounts } from '../count-tests.mjs';
import { diagnosticLog } from '../protocol-diagnostics.mjs';
import { evidence } from './protocol-fixture.mjs';
import { mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { legacyCandidate } from '../capture-protocol-workspace.mjs';
import { attest } from './protocol-source-fixture.mjs';
import { temporary } from './helpers.mjs';

test('capture keeps opaque package IDs/features and actual C invocation context', t => {
  const value = evidence(t);
  assert.ok(value.graph.nodes.some(node => node.id === 'cc'));
  assert.ok(value.graph.nodes.some(node => node.id === 'buffa' && node.features.includes('std')));
  const compile = value.tools.calls.find(call => call.roles.includes('targetCC'));
  assert.equal(compile.cwd, value.tools.compilerCalls[0].cwd);
  assert.match(compile.statusEvidence, /compiler\.jsonl:1: exit status: 0/);
  assert.equal(compile.context, 'target');
  assert.deepEqual(parsePackages(readFileSync(path.join(value.input, 'packages.tree'), 'utf8'), value.metadata, value.invocation), value.graph);
});
test('capture refuses ambiguous same-name/version identities and unknown IDs', t => {
  const value = evidence(t);
  value.metadata.packages.push({ ...value.metadata.packages[1], id: 'different-source' });
  assert.throws(() => identity('buffa v0.9.2', value.metadata), /ambiguous/);
  assert.throws(() => identity('missing v1.0.0', value.metadata), /ambiguous/);
});
test('capture rejects depth-prefixed output in a flat package contract', t => {
  const value = evidence(t);
  assert.throws(() => parsePackages('0impluvium-protocol v0.1.0|\n3cc v1.10.0|\n', value.metadata, value.invocation), /identity|selected root/);
});
test('tree proc-macro annotations must agree with opaque package metadata', t => {
  const value = evidence(t), pkg = value.metadata.packages[1];
  assert.throws(() => identity('buffa v0.9.2 (proc-macro)', value.metadata), /ambiguous/);
  pkg.targets = [{ kind: ['proc-macro'] }];
  assert.equal(identity('buffa v0.9.2 (proc-macro)', value.metadata).id, pkg.id);
  assert.equal(identity(`buffa v0.9.2 (${path.dirname(pkg.manifest_path)}) (proc-macro)`, value.metadata).id, pkg.id);
});
test('Cargo streams retain exact JSON and separate test bodies, rejecting truncation and repeated build streams', () => {
  const json = '{"reason":"build-finished","success":true}\n';
  assert.equal(cargoStream(json + 'running 1 test\n').jsonl, json);
  assert.equal(cargoStream(json + 'running 1 test\n').tests, 'running 1 test\n');
  assert.throws(() => cargoStream(''), /build-finished/);
  assert.throws(() => cargoStream(json + json), /repeated/);
  assert.throws(() => cargoStream('{"reason":"build-finished","success":false}\n'), /succeed/);
  assert.equal(cargoStream('[blake3 1.8.7] running: "cc" "-c" "source.c"\n' + json).jsonl, json);
});
test('command evidence preserves quoted arguments and never evaluates substitutions', () => {
  assert.deepEqual(words('CC="/tools/compiler path" "cc" "-DNAME=hello world" "$(not-executed)"'), ['CC=/tools/compiler path', 'cc', '-DNAME=hello world', '$(not-executed)']);
});

test('Cargo command displays preserve multiline descriptions and shell-escaped punctuation', () => {
  const display = "CARGO_PKG_DESCRIPTION='A compiler'\\''s interface evaluated at\n    compile time.' CARGO_PKG_HOMEPAGE='' TOOL='quote'\\!'(...)' /tools/rustc --crate-name typenum";
  const records = rustCommands('   Compiling typenum\n     Running `' + display + '`\n');
  assert.equal(records[0].line, 2);
  assert.deepEqual(words(records[0].text), ["CARGO_PKG_DESCRIPTION=A compiler's interface evaluated at\n    compile time.", 'CARGO_PKG_HOMEPAGE=', 'TOOL=quote!(...)', '/tools/rustc', '--crate-name', 'typenum']);
  assert.throws(() => rustCommands(' Running `unfinished'), /truncated/);
  assert.throws(() => words("NAME='unfinished"), /quote/);
});

test('cc commands and failed capability probes can appear in Cargo stdout before build-finished', t => {
  const value = evidence(t), log = readFileSync(path.join(value.input, 'build.log'), 'utf8');
  const compiler = value.tools.compilerCalls[0];
  value.tools.compilerCalls.push({ ...compiler, args: ['-?'], status: 1 });
  // Source: Cargo 1.99.1 -vv stdout for blake3 1.8.7: an unsuccessful
  // compiler-family probe (-?) precedes successful compile/archive commands.
  const raw = `[blake3 1.8.7] running: "${value.tools.launchers.targetCC}" "-?"\n[blake3 1.8.7] exit status: 1\n` +
    log.split('\n').filter(line => line.startsWith('[blake3 ')).join('\n') + '\n';
  const stdout = raw + readFileSync(path.join(value.input, 'cargo.stdout'), 'utf8');
  const calls = toolCalls(log.split('\n').filter(line => !line.startsWith('[blake3 ')).join('\n'), stdout, value.tools, value.metadata, value.root, value.invocation.target);
  assert.ok(calls.some(call => call.file === 'cargo.stdout' && call.roles.includes('targetAR')));
  assert.ok(calls.some(call => call.statusEvidence.endsWith('exit status: 1')));
  assert.ok(calls.some(call => call.roles.includes('targetCC') && call.args.includes('-c') && call.statusEvidence.endsWith('exit status: 0')));
});

test('independent command and Cargo stream failures retain every raw source location', t => {
  const value = evidence(t), report = diagnosticLog();
  const log = '[blake3 1.8.7] running: "foreign-cc" "-c" "source.c"\n[blake3 1.8.7] running: "foreign-ar" "cqD" "lib.a" "source.o"\n';
  const stdout = 'invalid raw row\n{"missing":"reason"}\n' + readFileSync(path.join(value.input, 'cargo.stdout'), 'utf8');
  toolCalls(log, stdout, value.tools, value.metadata, value.root, value.invocation.target, [], report);
  assert.equal(report.errors.length, 4);
  assert.deepEqual(report.errors.filter(entry => entry.file === 'build.log').map(entry => entry.line), [1, 2]);
  assert.deepEqual(report.errors.filter(entry => entry.file === 'cargo.stdout').map(entry => entry.line), [1, 2]);
  assert.ok(report.errors.every(entry => entry.raw.length));
  assert.throws(() => report.finish(), /foreign-cc|evidence launcher/);
});

test('verbose Cargo test commands pair zero-test units, facade tests and ignored doctests', () => {
  const unit = '/results/target/debug/deps/impluvium_protocol-aaaaaaaa', facade = '/results/target/debug/deps/protocol_facade-bbbbbbbb';
  const stream = { records: [unit, facade].map(executable => ({ reason: 'compiler-artifact', executable })),
    tests: '\nrunning 0 tests\ntest result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out\n\nrunning 3 tests\ntest result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out\n\nrunning 8 tests\ntest result: ok. 0 passed; 0 failed; 8 ignored; 0 measured; 0 filtered out\n' };
  const log = " Running `CARGO_PKG_DESCRIPTION='types evaluated at\n compile time' " + unit + "`\n Running `CARGO_PKG_NAME=impluvium-protocol " + facade + "`\n   Doc-tests impluvium_protocol\n Running `/tools/rustdoc --test lib.rs`\n";
  const transcript = testTranscript(log, stream), owner = new Set(['impluvium-protocol']);
  const counts = testCounts(transcript, 'tests.txt', { tests: new Map([['impluvium_protocol', owner], ['protocol_facade', owner]]), docTests: new Map([['impluvium_protocol', owner]]) });
  assert.deepEqual(counts.issues, []);
  assert.deepEqual(counts.packages.get('impluvium-protocol').counts, { tests: 3, docTests: 0 });
  assert.throws(() => testTranscript(log, { ...stream, tests: stream.tests.slice(0, stream.tests.lastIndexOf('running 8')) }), /pair/);
});

test('an unused C++ alias stays unused when C and linker aliases resolve to the same binary', () => {
  const tools = { executables: { cargo: { path: '/cargo' }, targetCC: { path: '/clang' }, targetCXX: { path: '/clang' }, targetLinker: { path: '/clang' } } };
  assert.deepEqual(unusedTools(tools, [{ roles: ['targetCC'] }], [{ context: 'target', executable: '/clang' }]), ['targetCXX']);
});

test('custom build target main is accepted only at its exact Cargo artifact path', t => {
  const value = evidence(t);
  const pkg = { id: 'aws-lc-sys', name: 'aws-lc-sys', version: '0.47.0',
    targets: [{ kind: ['custom-build'], name: 'build-script-main' }] };
  value.metadata.packages.push(pkg);
  // Source: aws-lc-sys 0.47.0 Cargo metadata custom-build target is named
  // build-script-main (builder/main.rs); Cargo 1.99.1 build.log:235 runs
  // debug/build/aws-lc-sys-fdc374a2ae27cc87/build-script-main.
  const command = path.join(value.invocation.targetDir, 'debug/build/aws-lc-sys-fdc374a2ae27cc87/build-script-main');
  mkdirSync(path.dirname(command), { recursive: true });
  writeFileSync(command, 'synthetic build artifact');
  const artifact = { reason: 'compiler-artifact', package_id: pkg.id, target: { kind: ['custom-build'], name: 'build-script-main' }, filenames: [command], executable: null };
  const stdout = JSON.stringify(artifact) + '\n' + '{"reason":"build-finished","success":true}\n';
  const log = `Running \`TARGET=x86_64-pc-windows-gnullvm ${command}\`\n`;
  assert.deepEqual(toolCalls(log, stdout, value.tools, value.metadata, value.root, value.invocation.target), []);
  assert.throws(() => toolCalls(log, '{"reason":"build-finished","success":true}\n', value.tools, value.metadata, value.root, value.invocation.target), /unapproved tool/);
  for (const mutate of [a => { a.package_id = 'unreported-package'; }, a => { a.target.name = 'unreported-target'; }, a => { a.target.kind = ['bin']; }, a => { a.filenames = [command + '-other']; }]) {
    const changed = structuredClone(artifact); mutate(changed);
    assert.throws(() => toolCalls(log, JSON.stringify(changed) + '\n{"reason":"build-finished","success":true}\n', value.tools, value.metadata, value.root, value.invocation.target), /unapproved tool/);
  }
  const impostor = command.replace('build-script-main', 'build-script-build-impostor');
  writeFileSync(impostor, 'unreported program');
  assert.throws(() => toolCalls(`Running \`${impostor}\`\n`, stdout, value.tools, value.metadata, value.root, value.invocation.target), /unapproved tool/);
  rmSync(path.dirname(command), { recursive: true });
  assert.deepEqual(toolCalls(log, stdout, value.tools, value.metadata, value.root, value.invocation.target), []);
});

test('only the actual base source can propose legacy exceptions', t => {
  const root = temporary(t); writeFileSync(path.join(root, 'Cargo.lock'), 'source fixture');
  const source = attest(root), baseSha = source.commit, paths = [{ root: 'desktop/client/Cargo.toml' }];
  assert.deepEqual(legacyCandidate({ root, source, sourceSha: baseSha, baseSha, paths, requirements: [] }), { baseSha, exceptions: paths, requirements: [] });
  assert.equal(legacyCandidate({ root, source, sourceSha: baseSha, baseSha: 'a'.repeat(40), paths }), null);
});
