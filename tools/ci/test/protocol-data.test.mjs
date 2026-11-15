import test from 'node:test';
import assert from 'node:assert/strict';
import { compactContract, expandContract } from '../protocol-data.mjs';

function fixture() {
  const pkg = name => ({ name, version: '1.0.0', source: 'registry' });
  const edge = alias => ({ alias, kind: 'normal', target: null });
  const row = (names, aliases) => ({ root: 'legacy/Cargo.toml', path: names.map(pkg), edges: aliases.map(edge), target: 'all', reason: `package:${names.at(-1)}`, removalOwner: 'legacy migration' });
  const packages = [{ ...pkg('core'), features: ['std'] }];
  return { version: 3, permissions: ['build', 'test', 'clippy'].map(run => ({ profile: 'core', target: 'linux', run, packages })),
    legacy: { baseSha: 'a'.repeat(40), requirements: [['legacy/Cargo.toml', 'feature', 'default', ['dep:transport']]],
      exceptions: [row(['root', 'left', 'shared', 'ring'], ['left', 'shared', 'crypto']), row(['root', 'right', 'shared', 'openssl'], ['right', 'shared', 'tls'])] } };
}
const canonical = data => ({ ...data, permissions: [...data.permissions].sort((a, b) => a.run.localeCompare(b.run)),
  legacy: { ...data.legacy, exceptions: [...data.legacy.exceptions].sort((a, b) => JSON.stringify(a).localeCompare(JSON.stringify(b))) } });
test('interned tuples, shared sets and complete path prefixes round-trip exactly', () => {
  const data = fixture(), compact = compactContract(data);
  assert.equal(compact.packageSets.length, 1);
  assert.deepEqual(canonical(expandContract(compact)), canonical(data));
  assert.equal(new Set(compact.permissions.map(entry => entry.set)).size, 1);
});
test('sharing an inner package never authorizes recombined inherited suffixes', () => {
  const compact = compactContract(fixture()), expanded = expandContract(compact);
  assert.deepEqual(expanded.legacy.exceptions.map(row => row.path.map(pkg => pkg.name)), [['root', 'left', 'shared', 'ring'], ['root', 'right', 'shared', 'openssl']]);
  assert.equal(expanded.legacy.exceptions.length, 2);
});
test('one identity with two feature sets is preserved without a union', () => {
  const data = fixture(); data.permissions[0].packages.push({ ...data.permissions[0].packages[0], features: ['json'] });
  const rows = expandContract(compactContract(data)).permissions.find(entry => entry.run === 'build').packages;
  assert.deepEqual(rows.map(row => row.features).sort(), [['json'], ['std']]);
});
test('invalid indices, cycles and missing package sets fail closed', () => {
  for (const mutate of [data => { data.permissions[0].set = 999; }, data => { data.legacy.nodes[0][0] = 0; }, data => { data.legacy.exceptions[0].leaf = -1; }, data => { data.packages[0][0] = 999; }, data => { data.legacy.requirements[0][1] = 999; }]) {
    const data = compactContract(fixture()); mutate(data); assert.throws(() => expandContract(data), /invalid interned/);
  }
});
test('input capture order does not change deterministic serialization', () => {
  const a = fixture(), b = fixture(); b.permissions.reverse(); b.legacy.exceptions.reverse();
  assert.equal(JSON.stringify(compactContract(a)), JSON.stringify(compactContract(b)));
});
