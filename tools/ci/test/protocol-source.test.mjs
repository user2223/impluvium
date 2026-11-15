import test from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import { mkdirSync, writeFileSync } from 'node:fs';
import { temporary } from './helpers.mjs';
import { attest } from './protocol-source-fixture.mjs';
import { verifySource, mutateManifest, negativeCases, gitObject, sourceTree } from '../protocol-source.mjs';
import { legacyCandidate } from '../capture-protocol-workspace.mjs';

function source(t) {
  const root = temporary(t); mkdirSync(path.join(root, 'src'));
  writeFileSync(path.join(root, 'Cargo.lock'), 'unchanged lock');
  writeFileSync(path.join(root, 'Cargo.toml'), '[package]\nname="example"\n');
  writeFileSync(path.join(root, 'src/lib.rs'), 'pub fn example() {}');
  return { root, receipt: attest(root) };
}
test('Git object hashing matches the standard empty blob and tree identities', () => {
  assert.equal(gitObject('blob', Buffer.alloc(0)), 'e69de29bb2d1d6434b8b29ae775ad8c2e48c5391');
  assert.equal(sourceTree([]), '4b825dc642cb6eb9a060e54bf8d69288fbee4904');
});
test('export without Git validates the commit object, tree, file modes and source bytes', t => {
  const { root, receipt } = source(t); assert.equal(verifySource(root, receipt, receipt.commit), receipt);
});
for (const file of ['src/lib.rs', 'Cargo.toml']) test(`old build evidence rejects changed ${file} with an unchanged lock`, t => {
  const { root, receipt } = source(t); writeFileSync(path.join(root, file), 'compile_error!("changed");');
  assert.throws(() => verifySource(root, receipt, receipt.commit), /source content differs/);
});
test('a current export cannot be labelled as an unrelated base', t => {
  const { root, receipt } = source(t), fake = 'a'.repeat(40);
  assert.throws(() => legacyCandidate({ root, source: receipt, sourceSha: fake, baseSha: fake, paths: [] }), /source receipt commit/);
  const relabelled = { ...receipt, commit: fake };
  assert.throws(() => verifySource(root, relabelled, fake), /Git object proof/);
});
test('a file catalogue cannot be changed independently of its Git commit object', t => {
  const { root, receipt } = source(t); receipt.files[0].blob = 'b'.repeat(40);
  assert.throws(() => verifySource(root, receipt, receipt.commit), /Git object proof/);
});
test('unattested inputs and missing independent receipts fail closed', t => {
  const { root, receipt } = source(t); writeFileSync(path.join(root, 'src/injected.rs'), '');
  assert.throws(() => verifySource(root, receipt, receipt.commit), /unattested source input/);
  assert.throws(() => verifySource(root, null, receipt.commit), /source receipt/);
});
test('only exact output directories are exempt; tracked files inside them stay mandatory', t => {
  const { root, receipt } = source(t);
  mkdirSync(path.join(root, 'target')); writeFileSync(path.join(root, 'target/output'), 'build output');
  verifySource(root, receipt, receipt.commit);
  mkdirSync(path.join(root, 'src/target')); writeFileSync(path.join(root, 'src/target/input.rs'), 'unattested');
  assert.throws(() => verifySource(root, receipt, receipt.commit), /unattested source input/);
  const tracked = attest(root); writeFileSync(path.join(root, 'target/output'), 'changed');
  assert.throws(() => verifySource(root, tracked, tracked.commit), /source content differs/);
});
test('ordinary captures reject explicit negative mutation receipts', t => {
  const { root, receipt } = source(t);
  receipt.mutation = { name: 'ring', path: negativeCases.ring[0], original: '' };
  assert.throws(() => verifySource(root, receipt, receipt.commit), /not the selected negative case/);
});
test('the Git-free pre-drift proof rejects repaired committed bytes and untracked output files', t => {
  const root = temporary(t), file = path.join(root, 'engine/protocol/ts/src/generated/example.ts');
  mkdirSync(path.dirname(file), { recursive: true }); writeFileSync(file, 'export {}; \n');
  const receipt = attest(root);
  writeFileSync(file, 'export {};\n');
  assert.throws(() => verifySource(root, receipt, receipt.commit), /source content differs/);
  writeFileSync(file, 'export {}; \n'); writeFileSync(file + '.extra', 'untracked');
  assert.throws(() => verifySource(root, receipt, receipt.commit), /unattested source input/);
});
for (const [name, spec] of Object.entries(negativeCases)) test(`negative ${name} makes exactly the declared manifest feature change`, () => {
  const text = `${spec[1]} = { version = "=1.0.0", default-features = false, features = ["json"] }\n`;
  const changed = mutateManifest(text, name);
  assert.equal(changed, text.replace('["json"]', `["json", "${spec[2]}"]`));
});
