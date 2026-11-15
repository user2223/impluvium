import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { verifyNegativeLock } from '../protocol-negative-lock.mjs';

const registry = 'registry+https://github.com/rust-lang/crates.io-index';
const packageTable = (name, dependencies = []) => `[[package]]\nname = "${name}"\nversion = "1.0.0"\nsource = "${registry}"\nchecksum = "${'a'.repeat(64)}"\ndependencies = ${JSON.stringify(dependencies)}\n`;
const first = packageTable('first'), second = packageTable('second');
const original = 'version = 4\n' + first + second;

test('unchanged locks and dependency-edge-only changes preserve the package set', () => {
  verifyNegativeLock(original, original);
  verifyNegativeLock(original, 'version = 4\n' + second + packageTable('first', ['second']));
  verifyNegativeLock(original, original.replace('dependencies = []\n', ''));
});
test('the actual product lock is accepted without any change', () => {
  const bytes = readFileSync(new URL('../../../Cargo.lock', import.meta.url));
  verifyNegativeLock(bytes, bytes);
});
for (const [name, dependency, edges] of [
  ['gzip', 'connectrpc', ['flate2']], ['zstd', 'connectrpc', ['zstd']],
  ['native-tls', 'reqwest', ['hyper-tls', 'native-tls', 'tokio-native-tls']],
]) test(`the measured ${name} edge changes preserve the actual product lock package set`, () => {
  const bytes = readFileSync(new URL('../../../Cargo.lock', import.meta.url), 'utf8');
  // Cargo's feature counterexamples add edges to packages already in the
  // workspace lock. Reproduce those changes in memory, without writing a lock.
  let matches = 0;
  const derived = bytes.split('[[package]]').map(table => {
    if (!table.includes(`\nname = "${dependency}"\n`)) return table;
    matches++;
    assert.match(table, /dependencies = \[/);
    return table.replace('dependencies = [', 'dependencies = [\n' + edges.map(edge => ` "${edge}",`).join('\n'));
  }).join('[[package]]');
  assert.ok(matches >= 1);
  verifyNegativeLock(bytes, derived);
});
for (const [name, changed] of [
  ['added package', original + packageTable('third')],
  ['removed package', 'version = 4\n' + first],
  ['changed name', original.replace('"first"', '"third"')],
  ['changed version', original.replace('"1.0.0"', '"2.0.0"')],
  ['changed source', original.replace(registry, 'registry+https://example.invalid/index')],
  ['removed source', original.replace(`source = "${registry}"\n`, '')],
  ['changed checksum', original.replace('a'.repeat(64), 'b'.repeat(64))],
  ['removed checksum', original.replace(`checksum = "${'a'.repeat(64)}"\n`, '')],
  ['new non-dependency field', original + 'replace = "other"\n'],
  ['changed lock format', original.replace('version = 4', 'version = 3')],
  ['changed top-level data', 'metadata = "changed"\n' + original],
  ['added package subtable', original + '[package.extra]\nflag = "changed"\n'],
  ['added package metadata', original + '[package.metadata]\nreviewed = false\n'],
]) test(`negative lock rejects ${name} with the prepared-lock diagnostic`, () => {
  assert.throws(() => verifyNegativeLock(original, changed), /reviewed prepared lock is required/);
});
for (const changed of ['', 'not TOML', 'version = 4\npackage = []', original + first, original.replace('dependencies = []', 'dependencies = [false]')]) {
  test(`invalid or duplicate package data fails closed: ${changed.slice(0, 40)}`, () => assert.throws(() => verifyNegativeLock(original, changed)));
}
