import test from 'node:test';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { closeSync, openSync, readFileSync, readSync, statSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { temporary } from './helpers.mjs';
import { captureFile, fileDigest, textLines } from '../protocol-io.mjs';

test('tree capture streams beyond the former 128 MiB buffer and preserves its final record and hash', async t => {
  const root = temporary(t), file = path.join(root, 'packages.tree');
  const script = '(async () => { const chunk = Buffer.alloc(1024 * 1024, 120); for (let i = 0; i < 129; i++) if (!process.stdout.write(chunk)) await require("events").once(process.stdout, "drain"); process.stdout.end("\\nlast-record\\n"); })().catch(() => { process.exitCode = 1; });';
  const result = await captureFile(process.execPath, ['-e', script], root, process.env, file);
  assert.equal(result.bytes, 129 * 1024 * 1024 + Buffer.byteLength('\nlast-record\n'));
  assert.deepEqual(fileDigest(file), result);
  const fd = openSync(file, 'r'), tail = Buffer.alloc(Buffer.byteLength('\nlast-record\n'));
  try { readSync(fd, tail, 0, tail.length, result.bytes - tail.length); }
  finally { closeSync(fd); }
  assert.equal(tail.toString(), '\nlast-record\n');
  // Exercise bounded line parsing separately; the bulk above deliberately
  // contains an oversized line, which must not be accepted as a tree record.
  assert.throws(() => [...textLines(file)], /line exceeds/);
  assert.equal(statSync(file + '.stderr').size, 0);
});

test('capture enforces the byte ceiling while retaining partial raw evidence', async t => {
  const root = temporary(t), file = path.join(root, 'packages.tree');
  await assert.rejects(captureFile(process.execPath, ['-e', 'process.stdout.write("123456789")'], root, process.env, file, 4), /byte limit/);
  assert.ok(statSync(file).size <= 4);
});

test('line and digest readers preserve UTF-8 boundaries, CRLF and a final unterminated record', t => {
  const file = path.join(temporary(t), 'packages.tree');
  const text = 'x'.repeat(65535) + 'é\r\nlast-record';
  writeFileSync(file, text);
  assert.deepEqual([...textLines(file)], ['x'.repeat(65535) + 'é', 'last-record']);
  assert.deepEqual(fileDigest(file), { bytes: Buffer.byteLength(text), sha256: createHash('sha256').update(readFileSync(file)).digest('hex') });
  assert.throws(() => [...textLines(file, 4)], /byte limit/);
  assert.throws(() => fileDigest(file, 4), /byte limit/);
});
