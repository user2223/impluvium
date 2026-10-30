import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fixture, invoke, temporary } from './helpers.mjs';

const args = (lock = fixture('Cargo.lock'), expect = fixture('locked-versions.json')) => ['--lock', lock, '--expect', expect];
function write(t, name, text) {
  const file = path.join(temporary(t), name);
  writeFileSync(file, text);
  return file;
}
const lock = (t, before, after) => {
  const source = readFileSync(fixture('Cargo.lock'), 'utf8');
  assert.ok(source.includes(before));
  return write(t, 'Cargo.lock', source.replace(before, after));
};

test('check-lock-versions passes when every package is locked at its expected version', () => {
  const result = invoke('check-lock-versions', args(), 0);
  assert.match(result.stdout, /framework: 2\.9\.5/);
  assert.match(result.stdout, /framework-utils: 2\.8\.2/);
});
test('check-lock-versions fails on a different locked version', t => {
  const result = invoke('check-lock-versions', args(lock(t, 'version = "2.8.2"', 'version = "2.10.0"')), 1);
  assert.match(result.stderr, /Cargo\.lock:\d+: framework-utils: locked at 2\.10\.0, expected 2\.8\.2/);
});
test('check-lock-versions fails when a package is missing or locked twice', t => {
  invoke('check-lock-versions', args(lock(t, 'name = "framework-utils"', 'name = "framework-tools"')), 1);
  const twice = readFileSync(fixture('Cargo.lock'), 'utf8') +
    '\n[[package]]\nname = "framework"\nversion = "2.10.1"\n';
  assert.match(invoke('check-lock-versions', args(write(t, 'Cargo.lock', twice)), 1).stderr, /locked at 2\.10\.1/);
});
test('check-lock-versions checks every expected package, not only locked ones', t => {
  const expect = write(t, 'expect.json', '{"version":1,"packages":{"framework":"2.9.5","absent":"1.0.0"}}');
  assert.match(invoke('check-lock-versions', args(fixture('Cargo.lock'), expect), 1).stderr, /absent: not in/);
});
test('check-lock-versions rejects malformed expectations and lock files', t => {
  for (const text of ['{"version":1,"packages":{}}', '{"version":1,"packages":{"framework":"2.9"}}',
    '{"version":2,"packages":{"framework":"2.9.5"}}', '{"version":1,"packages":{"framework":"~2.9.5"}}']) {
    invoke('check-lock-versions', args(fixture('Cargo.lock'), write(t, 'expect.json', text)), 2);
  }
  invoke('check-lock-versions', args(write(t, 'Cargo.lock', 'version = 4\n')), 2);
  invoke('check-lock-versions', args(write(t, 'Cargo.lock', '[[package]]\nname = "framework"\n')), 2);
  invoke('check-lock-versions', args(write(t, 'Cargo.lock', '[[package]\nname = "x"\n')), 2);
  invoke('check-lock-versions', ['--lock', fixture('Cargo.lock')], 2);
  invoke('check-lock-versions', args(path.join(temporary(t), 'absent.lock')), 2);
});
