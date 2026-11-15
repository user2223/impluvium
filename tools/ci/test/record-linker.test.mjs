import test from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import { cpSync, mkdirSync, readFileSync, realpathSync, writeFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { temporary } from './helpers.mjs';

for (const profile of ['core', 'server', 'client', 'combined']) {
  test(`Windows ${profile} host linker ignores triples in ancestor directories`, t => {
    const root = temporary(t), target = 'x86_64-pc-windows-gnullvm';
    const directory = path.join(root, profile, target, 'build');
    mkdirSync(directory, { recursive: true });
    const launcher = path.join(directory, 'host-linker.mjs'), log = path.join(directory, 'linker.jsonl');
    cpSync(new URL('../record-linker.mjs', import.meta.url), launcher);
    writeFileSync(launcher + '.json', JSON.stringify({ context: 'host', target, executable: process.execPath, log }));
    // Source: Cargo 1.99.1 Windows build linker.jsonl, all four profiles:
    // host cc links build_script_build below target/debug/build even when
    // the results directory itself contains the Windows triple.
    const args = ['-m64', '-o', path.join(directory, 'target/debug/build/quote-56c010621f6ef8d4/build_script_build-56c010621f6ef8d4')];
    // Node stands in for the driver and exits before reading its linker arguments.
    const result = spawnSync(process.execPath, [launcher, '-e', 'process.exit(0)', '--', ...args], { encoding: 'utf8' });
    assert.ifError(result.error);
    assert.equal(result.status, 0, result.stderr);
    const call = JSON.parse(readFileSync(log, 'utf8'));
    assert.equal(call.context, 'host');
    assert.equal(call.launcher, launcher);
    assert.equal(call.executable, realpathSync(process.execPath));
    assert.deepEqual(call.args.slice(3), args);
  });
}

test('native and cross target delegation do not depend on output layout', t => {
  const root = temporary(t), launcher = path.join(root, 'target-linker.mjs'), log = path.join(root, 'linker.jsonl');
  cpSync(new URL('../record-linker.mjs', import.meta.url), launcher);
  for (const target of ['x86_64-unknown-linux-gnu', 'x86_64-pc-windows-gnullvm']) {
    writeFileSync(launcher + '.json', JSON.stringify({ context: 'target', target, executable: process.execPath, log }));
    // Source: combined Linux test linker.jsonl includes a rustdoc link in
    // a temporary rustdoctest directory outside Cargo's target directory.
    const result = spawnSync(process.execPath, [launcher, '-e', 'process.exit(0)', '--', '-o', '/tmp/rustdoctest-example/rust_out'], { encoding: 'utf8' });
    assert.equal(result.status, 0, result.stderr);
  }
  assert.deepEqual(readFileSync(log, 'utf8').trim().split('\n').map(line => JSON.parse(line).context), ['target', 'target']);
});
