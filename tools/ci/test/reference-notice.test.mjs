import test from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { readFileSync, readdirSync, lstatSync } from 'node:fs';
import { createHash } from 'node:crypto';

const root = fileURLToPath(new URL('../../../engine/protocol/', import.meta.url));
const sources = JSON.parse(readFileSync(path.join(root, 'testdata/foreign/SOURCES.json')));
test('reference notices retain each foreign source and the separate benchmark provenance', () => {
  const notice = readFileSync(path.join(root, 'NOTICE'), 'utf8').replace(/\s+/g, ' ');
  for (const text of ['benchmarks/hashchunk/PROVENANCE.md', 'testdata/fastcdc-constants.json', 'The MIT License',
    'Unicode 16.0.0', 'Unicode License V3', 'RFC 5869', 'RFC 8032', 'IETF Trust',
    'No complete NIST test file is redistributed', '2026-11-16', 'CC0-1.0',
    'testdata/foreign/UNICODE-LICENSE-V3.txt', 'testdata/foreign/BLAKE3-CC0.txt',
    'not relicensed as project-owned 0BSD work']) assert(notice.includes(text), text);
});
test('redistributed foreign notices match their recorded source digests', () => {
  for (const [file, source] of [
    ['FASTCDC-MIT.txt', 'FASTCDC-NOTICE.txt'],
    ['UNICODE-LICENSE-V3.txt', 'foreign/unicode-16.0.0/license.txt'],
    ['BLAKE3-CC0.txt', 'foreign/blake3/LICENSE_CC0'],
  ]) {
    const target = path.join(root, 'testdata/foreign', file), entry = sources.sources.find(row => row.path === source);
    assert(entry); assert(lstatSync(target).isFile()); assert(!lstatSync(target).isSymbolicLink());
    assert.equal(createHash('sha256').update(readFileSync(target)).digest('hex'), entry.sha256, file);
  }
});
test('numerical corpus delivery contains no complete foreign source archives or test files', () => {
  assert.deepEqual(readdirSync(path.join(root, 'testdata/foreign')).sort(), ['BLAKE3-CC0.txt', 'FASTCDC-MIT.txt', 'SOURCES.json', 'UNICODE-LICENSE-V3.txt']);
  for (const entry of readdirSync(path.join(root, 'testdata'), { withFileTypes: true })) {
    if (entry.name === 'foreign') continue;
    assert(entry.isFile() && (entry.name.endsWith('.json') || entry.name === 'coverage.md'), entry.name);
  }
  assert.equal(sources.usage.nist, 'Only selected individual message/hash pairs are copied; no complete NIST source file is redistributed.');
});
