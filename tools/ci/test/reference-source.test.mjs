import test from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import { mkdirSync, symlinkSync, writeFileSync } from 'node:fs';
import { temporary } from './helpers.mjs';
import { attest } from './protocol-source-fixture.mjs';
import { verifySource } from '../protocol-source.mjs';
const ref = 'engine/protocol/conformance/reference';
const put = (root, file, text = 'input') => {
  const target = path.join(root, file);
  mkdirSync(path.dirname(target), { recursive: true }); writeFileSync(target, text);
};
test('source proof exempts only the reference root node_modules directory', t => {
  const root = temporary(t); put(root, ref + '/package.json', '{}');
  const receipt = attest(root);
  put(root, ref + '/node_modules/installed/index.js', 'installed output');
  assert.equal(verifySource(root, receipt, receipt.commit), receipt);
});
for (const file of [ref + '/src/node_modules/hidden.ts', ref + '/dist/hidden.js',
  'engine/protocol/conformance/other/node_modules/hidden.js', ref + '/node_modules.txt']) {
  test(`source proof still rejects ${file}`, t => {
    const root = temporary(t); put(root, ref + '/package.json', '{}'); const receipt = attest(root);
    put(root, file);
    assert.throws(() => verifySource(root, receipt, receipt.commit), /unattested source input/);
  });
}
test('tracked inputs inside the exception and beside it cannot change', t => {
  const root = temporary(t); put(root, ref + '/node_modules/tracked.json', '{}');
  put(root, ref + '/src/input.ts', 'export {};');
  const receipt = attest(root);
  put(root, ref + '/node_modules/tracked.json', 'changed');
  assert.throws(() => verifySource(root, receipt, receipt.commit), /source content differs/);
  put(root, ref + '/node_modules/tracked.json', '{}'); put(root, ref + '/src/input.ts', 'changed');
  assert.throws(() => verifySource(root, receipt, receipt.commit), /source content differs/);
});
test('the exception cannot be a symlink or an ordinary file', t => {
  const root = temporary(t); put(root, ref + '/package.json', '{}'); const receipt = attest(root);
  const other = temporary(t);
  symlinkSync(other, path.join(root, ref, 'node_modules'));
  assert.throws(() => verifySource(root, receipt, receipt.commit), /unattested source input/);
});
test('an ordinary file at the exact node_modules path is not an output directory', t => {
  const root = temporary(t); put(root, ref + '/package.json', '{}'); const receipt = attest(root);
  put(root, ref + '/node_modules');
  assert.throws(() => verifySource(root, receipt, receipt.commit), /unattested source input/);
});
