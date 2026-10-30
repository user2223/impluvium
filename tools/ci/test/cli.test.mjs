import test from 'node:test';
import assert from 'node:assert/strict';
import { invoke, temporary } from './helpers.mjs';
import { symlinkSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import path from 'node:path';

for (const tool of ['check-pins', 'check-features', 'check-licence-boundary', 'clippy-ratchet', 'tsc-ratchet', 'count-tests', 'check-lock-versions']) {
  test(`${tool}: help and unknown-option exit conventions`, () => {
    assert.match(invoke(tool, ['--help'], 0).stdout, /Usage:.*\n[\s\S]*Exit:/);
    invoke(tool, ['--unknown-option'], 2);
  });
}

test('cli: tools execute through a symlinked directory', t => {
  const directory = path.join(temporary(t), 'linked-ci');
  symlinkSync(fileURLToPath(new URL('../', import.meta.url)), directory, 'dir');
  for (const tool of ['check-pins', 'check-features', 'check-licence-boundary', 'clippy-ratchet', 'tsc-ratchet', 'count-tests', 'check-lock-versions']) {
    const script = path.join(directory, `${tool}.mjs`);
    const help = spawnSync(process.execPath, [script, '--help'], { encoding: 'utf8', timeout: 15_000 });
    assert.ifError(help.error);
    assert.equal(help.status, 0, help.stderr);
    assert.match(help.stdout, /Usage:/);
    const invalid = spawnSync(process.execPath, [script, '--unknown-option'], { encoding: 'utf8', timeout: 15_000 });
    assert.ifError(invalid.error);
    assert.equal(invalid.status, 2, invalid.stdout + invalid.stderr);
  }
});
