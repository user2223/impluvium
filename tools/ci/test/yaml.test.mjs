import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { yaml } from '../yaml.mjs';
import { temporary } from './helpers.mjs';

test('yaml: sequence mapping first entries preserve block content and following keys', t => {
  const source = 'steps:\n  - run: |\n      # keep this comment in the shell block\n      cargo test --locked\n    env:\n      DROP_VERSION: runtime\n  - env:\n      MODE: test\n    run: cargo build --locked\n';
  const file = path.join(temporary(t), 'workflow.yml');
  writeFileSync(file, source);
  const steps = yaml(file).value.steps.value;
  assert.match(steps[0].value.run.value, /^# keep this comment in the shell block\ncargo test --locked/);
  assert.equal(steps[0].value.env.value.DROP_VERSION.value, 'runtime');
  assert.equal(steps[1].value.run.value, 'cargo build --locked');
  assert.equal(readFileSync(file, 'utf8'), source);
});
