import test from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { syncBuiltinESMExports } from 'node:module';
import promises from 'node:fs/promises';
import { copyFileSync, mkdirSync, writeFileSync, readFileSync, symlinkSync, linkSync, lstatSync, readdirSync, rmSync } from 'node:fs';
import { temporary } from './helpers.mjs';
import { expectedPolicy } from '../reference-boundary.mjs';

// Exercise project-owned I/O code in isolated copies, without dependency code,
// child processes, source edits, or writes to the delivered data trees.
const source = fileURLToPath(new URL('../../../engine/protocol/conformance/reference/', import.meta.url));
async function fixture(t) {
  const root = temporary(t), ref = path.join(root, 'protocol/conformance/reference');
  const data = path.join(root, 'protocol/testdata'), outside = path.join(root, 'outside'), out = path.join(root, 'out');
  for (const directory of [ref, path.join(ref, 'src'), path.join(ref, 'tooling'), data, outside, out]) mkdirSync(directory, { recursive: true });
  for (const file of ['src/loader.ts', 'src/locations.ts', 'src/primitives.ts', 'src/recipes.ts', 'src/ed25519.ts', 'tooling/output.mjs']) copyFileSync(path.join(source, file), path.join(ref, file));
  writeFileSync(path.join(ref, 'package.json'), '{"type":"module"}');
  for (const file of ['README.md', 'FORMAT.md', 'MODULES.md']) writeFileSync(path.join(ref, file), 'Documentation.');
  writeFileSync(path.join(data, 'allowed.json'), '{}');
  const external = path.join(outside, 'existing.json'); writeFileSync(external, 'original');
  const loader = await import(pathToFileURL(path.join(ref, 'src/loader.ts')).href);
  const { writeReport } = await import(pathToFileURL(path.join(ref, 'tooling/output.mjs')).href);
  return { root, ref, data, outside, out, external, loader, writeReport };
}

test('report replacement leaves an outside hardlink unchanged', async t => {
  const { out, external, writeReport } = await fixture(t), target = path.join(out, 'report.json');
  linkSync(external, target);
  assert.equal(lstatSync(target).ino, lstatSync(external).ino);
  await writeReport(out, 'report', { ok: true });
  assert.equal(readFileSync(external, 'utf8'), 'original');
  assert.deepEqual(JSON.parse(readFileSync(target, 'utf8')), { ok: true });
  assert.notEqual(lstatSync(target).ino, lstatSync(external).ino);
  assert.deepEqual(readdirSync(out), ['report.json']);
});

test('package documentation reads exclude metadata and source; self-check source reads use the exact list', async t => {
  const { ref, loader } = await fixture(t);
  for (const file of ['README.md', 'FORMAT.md', 'MODULES.md']) assert.equal(await loader.readPackageText(file), 'Documentation.');
  for (const file of ['src/loader.ts', 'package.json']) await assert.rejects(() => loader.readPackageText(file), /invalid_package_read/);
  assert.equal(await loader.readSelfCheckText('src/loader.ts'), readFileSync(path.join(ref, 'src/loader.ts'), 'utf8'));
  assert.deepEqual([...loader.selfCheckSourceFiles].sort(), Object.keys(expectedPolicy.runtime.modules).sort());
  assert.equal(loader.selfCheckSourceFiles.length, 31);
  for (const file of ['package.json', 'LICENSE', 'README.md', 'src/unlisted.ts']) await assert.rejects(() => loader.readSelfCheckText(file), /invalid_self_check_read/);
});

test('report replacement replaces a symbolic file target without changing its referent', async t => {
  const { out, external, writeReport } = await fixture(t), target = path.join(out, 'report.json');
  symlinkSync(external, target);
  await writeReport(out, 'report', { ok: true });
  assert.equal(readFileSync(external, 'utf8'), 'original');
  assert(lstatSync(target).isFile());
  assert(!lstatSync(target).isSymbolicLink());
  assert.deepEqual(JSON.parse(readFileSync(target, 'utf8')), { ok: true });
  assert.deepEqual(readdirSync(out), ['report.json']);
});

test('report output rejects existing directories and symbolic directory targets', async t => {
  const { out, outside, writeReport } = await fixture(t), target = path.join(out, 'report.json');
  mkdirSync(target);
  await assert.rejects(() => writeReport(out, 'report', {}), /output_target_directory/);
  rmSync(target, { recursive: true }); symlinkSync(outside, target);
  await assert.rejects(() => writeReport(out, 'report', {}), /output_target_directory/);
  assert.deepEqual(readdirSync(out), ['report.json']);
});

test('a target changed into a directory immediately before rename is preserved and temporary output is removed', async t => {
  const { out, writeReport } = await fixture(t), target = path.join(out, 'report.json');
  writeFileSync(target, 'previous report');
  const original = promises.rename; let intercepted = 0;
  promises.rename = async (from, to) => {
    if (to === target) {
      intercepted++;
      assert(lstatSync(target).isFile());
      rmSync(target); mkdirSync(target);
      writeFileSync(path.join(target, 'marker'), 'preserved');
    }
    return original(from, to);
  };
  syncBuiltinESMExports();
  try {
    await assert.rejects(() => writeReport(out, 'report', {}), error => ['EISDIR', 'ENOTDIR', 'EEXIST', 'EPERM'].includes(error.code));
  } finally { promises.rename = original; syncBuiltinESMExports(); }
  assert.equal(intercepted, 1, 'the real rename boundary must be intercepted');
  assert(lstatSync(target).isDirectory());
  assert.equal(readFileSync(path.join(target, 'marker'), 'utf8'), 'preserved');
  assert.deepEqual(readdirSync(out), ['report.json']);
});

test('reads reject escaping paths and symbolic file or directory components', async t => {
  const { data, outside, external, loader } = await fixture(t);
  assert.equal(String(await loader.readDataBytes('allowed.json')), '{}');
  for (const file of ['../outside/existing.json', external, 'node_modules/item', 'a//b', 'a/./b', 'a\\b']) await assert.rejects(() => loader.readDataBytes(file), /invalid_read_path/);
  symlinkSync(external, path.join(data, 'linked.json')); symlinkSync(outside, path.join(data, 'linked-directory'));
  for (const file of ['linked.json', 'linked-directory/existing.json']) await assert.rejects(() => loader.readDataBytes(file), /read_symlink/);
});

test('delivered inventory excludes installed and undeclared directories and rejects source symlinks', async t => {
  const { ref, outside, external, loader } = await fixture(t);
  for (const name of ['node_modules', 'ignored']) { mkdirSync(path.join(ref, name)); symlinkSync(outside, path.join(ref, name, 'linked')); }
  const files = await loader.listDeliveredFiles();
  assert(!files.some(row => /node_modules|ignored/.test(row.file)));
  symlinkSync(external, path.join(ref, 'src/linked.ts'));
  await assert.rejects(() => loader.listDeliveredFiles(), /read_symlink/);
});

test('output requires an argument, rejects source aliases and validates report names', async t => {
  const { root, ref, data, out, writeReport } = await fixture(t);
  await assert.rejects(() => writeReport(undefined, 'report', {}), /Required output directory/);
  for (const [name, referent] of [['data-alias', data], ['package-alias', ref]]) {
    const alias = path.join(root, name); symlinkSync(referent, alias);
    await assert.rejects(() => writeReport(path.join(alias, 'new'), 'report', {}), /output_source_containment/);
    assert(!readdirSync(referent).includes('new'));
  }
  await assert.rejects(() => writeReport(out, '../outside/existing', {}), /Invalid report name/);
  assert.deepEqual(readdirSync(out), []);
});
