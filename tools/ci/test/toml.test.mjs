import test from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import { writeFileSync } from 'node:fs';
import { temporary } from './helpers.mjs';
import { parseToml, toml } from '../toml.mjs';

test('TOML retains decimal policy thresholds', t => {
  const file = path.join(temporary(t), 'deny.toml');
  writeFileSync(file, '[licenses]\nallow = ["MIT"]\nconfidence-threshold = 0.9\n');
  assert.equal(toml(file).data.licenses['confidence-threshold'], '0.9');
  for (const value of ['0.bad', '1..2', '01.2']) {
    writeFileSync(file, `value = ${value}\n`);
    assert.throws(() => toml(file));
  }
});
test('subtables and nested array tables belong to the last array element', () => {
  const { data } = parseToml('[[package]]\nname = "first"\n[package.extra]\nflag = "a"\n[[package.extra.items]]\nvalue = "one"\n[[package]]\nname = "second"\n[package.extra]\nflag = "b"\n');
  assert.equal(data.package[0].extra.flag, 'a');
  assert.equal(data.package[0].extra.items[0].value, 'one');
  assert.equal(data.package[1].extra.flag, 'b');
  assert.deepEqual(Object.keys(data.package), ['0', '1']);
});
test('manifest and licence subtables retain normal table semantics', () => {
  const { data } = parseToml('[dependencies.transport]\nversion = "1"\n[licenses.private]\nignore = true\n');
  assert.equal(data.dependencies.transport.version, '1');
  assert.equal(data.licenses.private.ignore, true);
  assert.throws(() => parseToml('[[package]]\nname = "one"\n[package.extra]\na = true\n[package.extra]\nb = true\n'));
  assert.throws(() => parseToml('[[package]]\nname = "one"\nvalues = []\nvalues.hidden = "lost"\n'));
});
