import path from 'node:path';
import { lstatSync, mkdirSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { cli, required, stringOption } from './common.mjs';
import { externalOutput, fail, hash } from './protocol-contract.mjs';
import { gitObject, mutateManifest, negativeCases, sourceTree, verifySource } from './protocol-source.mjs';
import { verifyNegativeLock } from './protocol-negative-lock.mjs';

export function deriveNegativeLock(root, run = spawnSync) {
  // Resolve once without --locked, using only the product's fetched packages.
  // Every later metadata, tree and build command uses --locked --offline.
  const result = run('cargo', ['metadata', '--offline', '--format-version=1', '--manifest-path', path.join(root, 'Cargo.toml')],
    { cwd: root, stdio: ['ignore', 'ignore', 'inherit'] });
  if (result.status !== 0 || result.error || result.signal) fail('offline negative lock resolution failed; this is not a successful counterexample');
}

export function createReceipt(repository, commit, commitObjectFile, readGit) {
  if (!/^[a-f0-9]{40}$/.test(commit)) fail('source export needs a full commit ID');
  const git = readGit ?? (args => {
    const result = spawnSync('git', ['-C', repository, ...args], { maxBuffer: 64 * 1024 * 1024 });
    if (result.status !== 0 || result.error || result.signal) fail('cannot read source Git objects');
    return result.stdout;
  });
  if (commitObjectFile !== undefined && !lstatSync(commitObjectFile).isFile()) fail('commit object must be a regular file');
  const commitObject = commitObjectFile === undefined ? git(['cat-file', 'commit', commit]) : readFileSync(commitObjectFile);
  if (gitObject('commit', commitObject) !== commit) fail('source commit object differs');
  const contents = new Map();
  let files;
  if (commitObjectFile !== undefined) {
    if (!lstatSync(path.resolve(repository)).isDirectory()) fail('source export must be a regular directory');
    files = [];
    const visit = (directory, prefix = '') => {
      for (const name of readdirSync(directory)) {
        const file = path.join(directory, name), relative = prefix + name, info = lstatSync(file);
        if (info.isDirectory()) visit(file, relative + '/');
        else if (info.isFile()) {
          const bytes = readFileSync(file); contents.set(relative, bytes);
          files.push({ path: relative, mode: info.mode & 0o100 ? '100755' : '100644', blob: gitObject('blob', bytes), sha256: hash(bytes) });
        } else fail(`source export contains a symlink or special file: ${relative}`);
      }
    };
    // Empty directories have no Git tree entry. Every file is included, even
    // below build-output names; this input must be an untouched source export.
    visit(repository);
    files.sort((a, b) => Buffer.compare(Buffer.from(a.path), Buffer.from(b.path)));
  } else files = git(['ls-tree', '-rz', '--full-tree', commit]).toString('utf8').split('\0').filter(Boolean).map(row => {
    const match = /^(100644|100755) blob ([a-f0-9]{40})\t(.+)$/.exec(row);
    if (!match) fail('source export supports regular Git files only');
    const bytes = git(['cat-file', 'blob', match[2]]); contents.set(match[3], bytes);
    return { path: match[3], mode: match[1], blob: match[2], sha256: hash(bytes) };
  });
  const receipt = { version: 1, commit, commitObject: commitObject.toString('base64'), files };
  if (!commitObject.toString('utf8').startsWith(`tree ${sourceTree(files)}\n`)) fail('source tree differs');
  return { receipt, contents };
}
export function exportSource(options, readGit, deriveLock = deriveNegativeLock) {
  required(options, 'repository', 'commit', 'receipt');
  const { receipt, contents } = createReceipt(options.repository, options.commit, options['commit-object'], readGit);
  const receiptFile = externalOutput(options.repository, options.receipt);
  if (options['negative-case']) {
    const name = options['negative-case'], spec = negativeCases[name];
    if (!spec || !options.out) fail('negative export needs a known case and fresh output directory');
    verifySource(options.repository, receipt, options.commit);
    const original = contents.get(spec[0]), lock = contents.get('Cargo.lock');
    if (!original || !lock) fail('negative export needs the original manifest and product lock');
    receipt.mutation = { name, path: spec[0], original: original.toString('base64'), lockOriginal: lock.toString('base64') };
    contents.set(spec[0], Buffer.from(mutateManifest(original.toString('utf8'), name)));
  }
  let root = options.repository;
  if (options.out) {
    root = externalOutput(options.repository, options.out); mkdirSync(root);
    for (const item of receipt.files) {
      const file = path.join(root, item.path); mkdirSync(path.dirname(file), { recursive: true });
      writeFileSync(file, contents.get(item.path), { flag: 'wx', mode: parseInt(item.mode, 8) });
    }
  }
  if (receipt.mutation) {
    deriveLock(root);
    const derived = readFileSync(path.join(root, 'Cargo.lock'));
    verifyNegativeLock(contents.get('Cargo.lock'), derived);
    receipt.mutation.lockSha256 = hash(derived);
  }
  verifySource(root, receipt, options.commit, options['negative-case'] ?? null);
  writeFileSync(receiptFile, JSON.stringify(receipt, null, 2) + '\n', { flag: 'wx' });
  return [];
}
await cli(import.meta.url, 'Usage: node tools/ci/export-protocol-source.mjs --repository DIR --commit SHA --receipt FILE [--commit-object FILE] [--out DIR] [--negative-case NAME]\nRead Git objects, or an untouched source export plus its raw commit object, and attest exact source bytes. Outputs must be outside the source directory. Negative exports derive and validate their lock offline using Cargo.', {
  repository: stringOption, commit: stringOption, receipt: stringOption, out: stringOption, 'negative-case': stringOption, 'commit-object': stringOption,
}, exportSource);
