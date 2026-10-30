import assert from 'node:assert/strict';
import { cpSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

export const fixture = name => fileURLToPath(new URL(`./fixtures/${name}`, import.meta.url));
export function temporary(t) {
  const directory = mkdtempSync(path.join(tmpdir(), 'impluvium-ci-'));
  t.after(() => rmSync(directory, { recursive: true, force: true }));
  return directory;
}
export function repository(t) {
  const directory = temporary(t);
  cpSync(fixture('repository'), directory, { recursive: true });
  return directory;
}
export function replace(root, file, before, after) {
  const absolute = path.join(root, file);
  const source = readFileSync(absolute, 'utf8');
  assert.ok(source.includes(before), `test replacement missing in ${file}`);
  writeFileSync(absolute, source.replace(before, after));
}
export function invoke(tool, args, expected) {
  const script = fileURLToPath(new URL(`../${tool}.mjs`, import.meta.url));
  const result = spawnSync(process.execPath, [script, ...args], { encoding: 'utf8', timeout: 15_000 });
  assert.ifError(result.error);
  assert.equal(result.signal, null);
  assert.equal(result.status, expected, result.stdout + result.stderr);
  if (expected !== 0) assert.match(result.stderr, /:\d+: /);
  return result;
}
