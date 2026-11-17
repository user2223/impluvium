import test from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import { mkdirSync, symlinkSync, writeFileSync } from 'node:fs';
import { temporary } from './helpers.mjs';
import { referencePath } from '../reference-boundary.mjs';
import { check, publicationPolicy, referencePublicationIssues } from '../check-reference-publication.mjs';

const policy = { version: 1, terms: ['reserved.fixture'] };
function fixture(t) {
  const root = temporary(t), ref = path.join(root, referencePath), data = path.join(root, 'engine/protocol/testdata');
  mkdirSync(ref, { recursive: true }); mkdirSync(data, { recursive: true });
  writeFileSync(path.join(ref, 'README.md'), 'Public reference package.');
  writeFileSync(path.join(data, 'index.json'), '{"cases":[]}');
  return { root, ref, data };
}
test('publication policy has a positive baseline and no built-in private vocabulary', t => {
  const { root } = fixture(t);
  assert.deepEqual(referencePublicationIssues(root, policy), []);
});
test('a word-list definition has no exemption from the publication check', t => {
  const { root, ref } = fixture(t);
  writeFileSync(path.join(ref, 'policy.ts'), 'export const words = ["reserved.fixture"];');
  assert.match(referencePublicationIssues(root, policy).join('\n'), /policy.ts:1: publication policy term 1/);
});
test('decoded JSON strings cannot hide a denied term behind escapes', t => {
  const { root, data } = fixture(t);
  writeFileSync(path.join(data, 'index.json'), '{"value":"\\u0072eserved.fixture"}');
  assert.match(referencePublicationIssues(root, policy).join('\n'), /index.json:1 .*publication policy term 1/);
});
test('only the exact package installation directory is exempt', t => {
  const { root, ref } = fixture(t);
  mkdirSync(path.join(ref, 'node_modules')); writeFileSync(path.join(ref, 'node_modules/output.txt'), 'reserved.fixture');
  assert.deepEqual(referencePublicationIssues(root, policy), []);
  mkdirSync(path.join(ref, 'src/node_modules'), { recursive: true }); writeFileSync(path.join(ref, 'src/node_modules/input.txt'), 'reserved.fixture');
  assert.ok(referencePublicationIssues(root, policy).length);
});
test('publication paths cannot escape through a symlink', t => {
  const { root, ref } = fixture(t);
  symlinkSync(path.join(ref, 'README.md'), path.join(ref, 'hidden.txt'));
  assert.throws(() => referencePublicationIssues(root, policy), /symlink/);
});
test('missing, empty or exception-bearing policies fail closed', () => {
  for (const value of [{ version: 1, terms: [] }, { version: 1, terms: [''] }, { ...policy, exceptions: [] },
    { version: 1, terms: ['same', 'SAME'] }]) assert.throws(() => publicationPolicy(value));
  assert.throws(() => check({ root: '.' }), /--policy is required/);
  assert.throws(() => check({ root: '.', policy: 'policy.json' }), /outside the source tree/);
});

test('a real external policy works and an external symlink cannot hide an internal policy', t => {
  const { root, ref } = fixture(t), external = temporary(t), file = path.join(external, 'policy.json');
  writeFileSync(file, JSON.stringify(policy));
  assert.deepEqual(check({ root, policy: file }), []);
  const internal = path.join(ref, 'policy.json'), link = path.join(external, 'link.json');
  writeFileSync(internal, JSON.stringify(policy)); symlinkSync(internal, link);
  assert.throws(() => check({ root, policy: link }), /regular file outside/);
});
test('unreadable text and malformed outer JSON cannot produce a clean publication report', t => {
  const { root, data } = fixture(t), file = path.join(data, 'index.json');
  writeFileSync(file, Buffer.from('ff', 'hex'));
  assert.throws(() => referencePublicationIssues(root, policy), /not UTF-8/);
  writeFileSync(file, '{');
  assert.throws(() => referencePublicationIssues(root, policy), /not analyzable/);
});
