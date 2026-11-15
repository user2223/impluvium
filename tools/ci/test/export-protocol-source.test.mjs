import test from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import { chmodSync, cpSync, mkdirSync, readFileSync, rmSync, symlinkSync, writeFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { temporary } from './helpers.mjs';
import { attest, productLock } from './protocol-source-fixture.mjs';
import { createReceipt, deriveNegativeLock, exportSource } from '../export-protocol-source.mjs';
import { gitObject, mutateManifest, verifySource } from '../protocol-source.mjs';
import { hash } from '../protocol-contract.mjs';

function fixture(t) {
  const repository = temporary(t), source = temporary(t), evidence = temporary(t);
  const put = (relative, bytes, mode = 0o664) => {
    const file = path.join(source, relative); mkdirSync(path.dirname(file), { recursive: true });
    writeFileSync(file, bytes); chmodSync(file, mode);
  };
  put('a/leaf.rs', 'pub fn leaf() {}\n');
  put('a.c', '/* File sorts before the directory a/ in Git. */\n');
  put('src/lib.rs', 'pub fn example() {}\n');
  put('script.sh', '#!/bin/sh\nexit 0\n', 0o775);
  put('Cargo.lock', productLock);
  put('engine/crates/client/Cargo.toml', 'rustls = { version = "=0.23.46", default-features = false, features = ["std", "tls12", "aws-lc-rs"] }\n');
  cpSync(source, repository, { recursive: true });
  const proof = attest(source), commit = proof.commit, raw = Buffer.from(proof.commitObject, 'base64');
  const files = [...proof.files].sort((a, b) => Buffer.compare(Buffer.from(a.path), Buffer.from(b.path)));
  // Supply the Git reader's exact object responses without creating a Git
  // repository. The other reader must discover these same records on disk.
  const readGit = args => {
    if (args[0] === 'ls-tree') return Buffer.from(files.map(file => `${file.mode} blob ${file.blob}\t${file.path}\0`).join(''));
    if (args[0] === 'cat-file' && args[1] === 'commit' && args[2] === commit) return raw;
    if (args[0] === 'cat-file' && args[1] === 'blob') {
      const file = files.find(file => file.blob === args[2]);
      if (file) return readFileSync(path.join(repository, file.path));
    }
    assert.fail(`unexpected Git object request: ${args.join(' ')}`);
  };
  const object = path.join(evidence, 'commit.raw'); writeFileSync(object, raw);
  return { repository, source, evidence, commit, object, raw, readGit };
}

test('Git and pristine-export forms write byte-identical receipts', t => {
  const value = fixture(t), fromGit = path.join(value.evidence, 'git.json'), fromExport = path.join(value.evidence, 'export.json');
  exportSource({ repository: value.repository, commit: value.commit, receipt: fromGit }, value.readGit);
  exportSource({ repository: value.source, commit: value.commit, 'commit-object': value.object, receipt: fromExport });
  assert.deepEqual(readFileSync(fromExport), readFileSync(fromGit));
});
for (const relative of ['engine/crates/protocol/tests/target/main.rs', 'engine/crates/protocol/src/bin/dist/main.rs']) {
  test(`both attestation forms reject an untracked automatically discovered Cargo target: ${relative}`, t => {
    const value = fixture(t), file = path.join(value.repository, relative);
    mkdirSync(path.dirname(file), { recursive: true }); writeFileSync(file, 'compile_error!("unattested");');
    assert.throws(() => exportSource({ repository: value.repository, commit: value.commit, receipt: path.join(value.evidence, 'source.json') }, value.readGit), /unattested source input/);
    const extra = path.join(value.source, relative);
    mkdirSync(path.dirname(extra), { recursive: true }); writeFileSync(extra, 'compile_error!("unattested");');
    assert.throws(() => exportSource({ repository: value.source, commit: value.commit, 'commit-object': value.object,
      receipt: path.join(value.evidence, 'export.json') }), /source tree differs/);
  });
}
test('empty directories are ignored because Git does not record them', t => {
  const value = fixture(t), original = createReceipt(value.source, value.commit, value.object).receipt;
  mkdirSync(path.join(value.source, 'empty/nested'), { recursive: true });
  assert.deepEqual(createReceipt(value.source, value.commit, value.object).receipt, original);
});
for (const relative of ['extra.txt', 'target/extra.rs', 'node_modules/extra.js']) {
  test(`the pristine-export catalogue has no exclusions: ${relative}`, t => {
    const value = fixture(t), file = path.join(value.source, relative);
    mkdirSync(path.dirname(file), { recursive: true }); writeFileSync(file, 'extra');
    assert.throws(() => createReceipt(value.source, value.commit, value.object), /source tree differs/);
  });
}
test('a changed byte and a missing file each fail the tree proof', t => {
  const value = fixture(t), file = path.join(value.source, 'src/lib.rs'), original = readFileSync(file);
  const changed = Buffer.from(original); changed[0] ^= 1; writeFileSync(file, changed);
  assert.throws(() => createReceipt(value.source, value.commit, value.object), /source tree differs/);
  writeFileSync(file, original); rmSync(file);
  assert.throws(() => createReceipt(value.source, value.commit, value.object), /source tree differs/);
});
test('a false commit object fails before a source receipt can be issued', t => {
  const value = fixture(t); writeFileSync(value.object, 'not the commit object');
  assert.throws(() => createReceipt(value.source, value.commit, value.object), /source commit object differs/);
});
test('another commit with the same tree cannot stand in for the requested commit ID', t => {
  const value = fixture(t), other = Buffer.concat([value.raw, Buffer.from('Another source\n')]);
  assert.notEqual(gitObject('commit', other), value.commit);
  assert.equal(other.toString('utf8').split('\n')[0], value.raw.toString('utf8').split('\n')[0]);
  writeFileSync(value.object, other);
  assert.throws(() => createReceipt(value.source, value.commit, value.object), /source commit object differs/);
});
test('changing the owner executable bit changes the tree', t => {
  const value = fixture(t); chmodSync(path.join(value.source, 'script.sh'), 0o664);
  assert.throws(() => createReceipt(value.source, value.commit, value.object), /source tree differs/);
});
test('only the owner executable bit matters for archive modes', t => {
  const value = fixture(t);
  chmodSync(path.join(value.source, 'src/lib.rs'), 0o675);
  chmodSync(path.join(value.source, 'script.sh'), 0o755);
  const receipt = createReceipt(value.source, value.commit, value.object).receipt;
  assert.deepEqual(receipt, createReceipt(value.repository, value.commit, undefined, value.readGit).receipt);
  assert.equal(verifySource(value.source, receipt, value.commit), receipt);
});
test('symlinks are rejected even below names used for build outputs', t => {
  const value = fixture(t); mkdirSync(path.join(value.source, 'target'));
  symlinkSync('../src/lib.rs', path.join(value.source, 'target/link'));
  assert.throws(() => createReceipt(value.source, value.commit, value.object), /symlink or special file/);
});
test('special files are rejected before attempting to read their bytes', t => {
  const value = fixture(t), result = spawnSync('mkfifo', [path.join(value.source, 'pipe')], { encoding: 'utf8' });
  assert.ifError(result.error); assert.equal(result.status, 0, result.stderr);
  assert.throws(() => createReceipt(value.source, value.commit, value.object), /symlink or special file/);
});
test('a fresh base export is copied from attested bytes without Git', t => {
  const value = fixture(t), out = path.join(value.evidence, 'base'), receipt = path.join(value.evidence, 'base.json');
  exportSource({ repository: value.source, commit: value.commit, 'commit-object': value.object, out, receipt });
  const actual = JSON.parse(readFileSync(receipt));
  assert.deepEqual(actual, createReceipt(value.repository, value.commit, undefined, value.readGit).receipt);
  verifySource(out, actual, value.commit);
});
test('a negative case derived from a pristine export keeps the same verification path', t => {
  const value = fixture(t), out = path.join(value.evidence, 'negative'), receipt = path.join(value.evidence, 'negative.json');
  let resolutions = 0;
  exportSource({ repository: value.source, commit: value.commit, 'commit-object': value.object, out, receipt, 'negative-case': 'ring' }, undefined, root => {
    assert.equal(root, out); resolutions++;
    assert.match(readFileSync(path.join(root, 'engine/crates/client/Cargo.toml'), 'utf8'), /"ring"/);
    // A feature-only case can leave the product lock byte-identical.
  });
  assert.equal(resolutions, 1);
  assert.equal(readFileSync(path.join(out, 'engine/crates/client/Cargo.toml'), 'utf8'),
    mutateManifest(readFileSync(path.join(value.source, 'engine/crates/client/Cargo.toml'), 'utf8'), 'ring'));
  assert.deepEqual(readFileSync(path.join(out, 'Cargo.lock')), readFileSync(path.join(value.source, 'Cargo.lock')));
  const proof = JSON.parse(readFileSync(receipt));
  verifySource(out, proof, value.commit, 'ring');
  writeFileSync(path.join(out, 'Cargo.lock'), productLock + '# changed after derivation\n');
  assert.throws(() => verifySource(out, proof, value.commit, 'ring'), /derived negative lock changed/);
});
test('a derived lock is checked before issuing a negative receipt and again when replaying it', t => {
  const value = fixture(t), out = path.join(value.evidence, 'negative'), receipt = path.join(value.evidence, 'negative.json');
  const options = { repository: value.source, commit: value.commit, 'commit-object': value.object, out, receipt, 'negative-case': 'ring' };
  assert.throws(() => exportSource(options, undefined, root => {
    writeFileSync(path.join(root, 'Cargo.lock'), productLock.replace('1.0.0', '2.0.0'));
  }), /reviewed prepared lock is required/);
  rmSync(out, { recursive: true });
  exportSource(options, undefined, () => {});
  const proof = JSON.parse(readFileSync(receipt));
  const changed = productLock.replace('1.0.0', '2.0.0');
  writeFileSync(path.join(out, 'Cargo.lock'), changed);
  proof.mutation.lockSha256 = hash(Buffer.from(changed));
  assert.throws(() => verifySource(out, proof, value.commit, 'ring'), /reviewed prepared lock is required/);
  proof.mutation.lockOriginal = Buffer.from(changed).toString('base64');
  assert.throws(() => verifySource(out, proof, value.commit, 'ring'), /negative lock original differs from Git/);
});
test('edge-only lock derivation is replayed while all unrelated source bytes remain mandatory', t => {
  const value = fixture(t), out = path.join(value.evidence, 'negative'), receipt = path.join(value.evidence, 'negative.json');
  const derived = productLock.replace('dependencies = []', 'dependencies = ["helper"]');
  exportSource({ repository: value.source, commit: value.commit, 'commit-object': value.object, out, receipt, 'negative-case': 'ring' }, undefined, root => {
    writeFileSync(path.join(root, 'Cargo.lock'), derived);
  });
  const proof = JSON.parse(readFileSync(receipt));
  verifySource(out, proof, value.commit, 'ring');
  assert.equal(readFileSync(path.join(value.source, 'Cargo.lock'), 'utf8'), productLock);
  writeFileSync(path.join(out, 'src/lib.rs'), 'changed');
  assert.throws(() => verifySource(out, proof, value.commit, 'ring'), /source content differs/);
});
test('the single lock derivation is offline and intentionally unlocked', () => {
  deriveNegativeLock('/source', (command, args, options) => {
    assert.equal(command, 'cargo');
    assert.deepEqual(args, ['metadata', '--offline', '--format-version=1', '--manifest-path', '/source/Cargo.toml']);
    assert.equal(options.cwd, '/source');
    return { status: 0 };
  });
  for (const result of [{ status: 1 }, { status: null, signal: 'SIGTERM' }, { error: new Error('missing Cargo') }]) {
    assert.throws(() => deriveNegativeLock('/source', () => result), /offline negative lock resolution failed/);
  }
});
