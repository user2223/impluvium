import test from 'node:test';
import assert from 'node:assert/strict';
import { invoke } from './helpers.mjs';

for (const tool of ['check-pins', 'check-features', 'check-licence-boundary', 'clippy-ratchet', 'tsc-ratchet', 'count-tests']) {
  test(`${tool}: help and unknown-option exit conventions`, () => {
    assert.match(invoke(tool, ['--help'], 0).stdout, /Usage:.*\n[\s\S]*Exit:/);
    invoke(tool, ['--unknown-option'], 2);
  });
}
