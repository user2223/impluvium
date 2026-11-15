import test from 'node:test';
import assert from 'node:assert/strict';
import { writeFileSync } from 'node:fs';
import path from 'node:path';
import { acceptable, policy } from '../spdx.mjs';
import { temporary } from './helpers.mjs';
import { fileURLToPath } from 'node:url';

const rules = policy(fileURLToPath(new URL('../../../deny.toml', import.meta.url)));
for (const expression of [
  'MIT', 'MIT/Apache-2.0', 'Apache-2.0 / MIT', 'Unlicense/MIT',
  'MIT OR Apache-2.0 OR LGPL-2.1-or-later', 'MIT OR MPL-2.0',
  '(MIT OR LGPL-2.1-or-later) AND Apache-2.0', 'Apache-2.0 WITH LLVM-exception',
  'MIT OR (LGPL-2.1-or-later AND MPL-2.0)', 'ISC OR BSD-4-Clause',
  'MIT OR LGPL-2.1-or-later AND MPL-2.0',
  'MIT OR GPL-2.0-only WITH Linux-syscall-note',
]) test(`SPDX admits ${expression}`, () => assert.equal(acceptable(expression, rules), true));
for (const expression of [
  'LGPL-2.1-or-later', 'GPL-3.0/LGPL-2.1', 'MIT AND LGPL-2.1-or-later', 'MPL-2.0',
  '(MIT OR Apache-2.0) AND GPL-3.0-only', 'GPL-2.0-only WITH Classpath-exception-2.0',
  'LGPL-2.1-or-later OR MIT AND MPL-2.0',
  'MIT WITH LLVM-exception',
]) test(`SPDX rejects ${expression}`, () => assert.equal(acceptable(expression, rules), false));
for (const expression of ['', null, 'MIT OR Unknown-License', 'MIT AND', '(MIT', 'MIT)', 'MIT//Apache-2.0', 'MIT/Unknown-License',
  'MIT OR Apache-2.0 WITH Unknown-exception', 'MIT WITH Unknown-exception',
  '(MIT OR Apache-2.0) WITH LLVM-exception', 'MIT XOR Apache-2.0']) {
  test(`SPDX fails closed for ${expression}`, () => assert.throws(() => acceptable(expression, rules), /SPDX|missing/));
}
test('SPDX admission reads deny.toml rather than a second allow list', t => {
  const file = path.join(temporary(t), 'deny.toml');
  writeFileSync(file, '[licenses]\nallow = ["ISC"]\n');
  assert.equal(acceptable('ISC', policy(file)), true);
  assert.equal(acceptable('MIT', policy(file)), false);
  assert.equal(acceptable('ISC OR BSD-4-Clause', policy(file)), true);
  writeFileSync(file, '[licenses]\nallow = ["Unknown-License"]\n');
  assert.throws(() => policy(file), /unknown or malformed/);
});
