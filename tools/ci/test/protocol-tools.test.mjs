import test from 'node:test';
import assert from 'node:assert/strict';
import { checkedArchive, toolContract } from '../protocol-tools.mjs';
import { cpSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { temporary } from './helpers.mjs';

for (const fault of ['download', 'checksum', 'lock', 'fetch', 'build', null]) {
  test(`tool bootstrap stops before use on ${fault ?? 'successful ordered preparation'}`, () => {
    const events = [];
    const operation = name => () => { events.push(name); if (fault === name) throw new Error(name); };
    const ops = Object.fromEntries(['download', 'extract', 'fetch', 'build', 'install'].map(name => [name, operation(name)]));
    ops.archiveHash = () => { events.push('checksum'); return fault === 'checksum' ? 'wrong' : 'expected'; };
    ops.sameLock = () => { events.push('lock'); return fault !== 'lock'; };
    const run = () => checkedArchive({ binary: 'plugin', crate: 'plugin', sha256: 'expected' }, ops);
    if (fault) {
      assert.throws(run);
      assert.equal(events.at(-1), fault);
      assert.ok(!events.includes('install'));
    } else {
      run();
      assert.deepEqual(events, ['download', 'checksum', 'extract', 'lock', 'fetch', 'build', 'install']);
    }
  });
}

test('generator source mapping checks actual workflow pins and frozen lock bytes', t => {
  const source = fileURLToPath(new URL('../../../', import.meta.url)), root = temporary(t);
  for (const relative of ['.gitea/workflows/ci.yml', 'tools/ci/codegen-tools.json',
    'engine/protocol/ts/package.json', 'tools/ci/generator-locks']) {
    const target = path.join(root, relative); mkdirSync(path.dirname(target), { recursive: true });
    cpSync(path.join(source, relative), target, { recursive: true });
  }
  assert.equal(toolContract(root).mapping.tools.length, 5);
  const file = path.join(root, 'tools/ci/codegen-tools.json'), data = JSON.parse(readFileSync(file));
  data.tools[1].lockSha256 = '0'.repeat(64); writeFileSync(file, JSON.stringify(data));
  assert.throws(() => toolContract(root), /frozen generator lock/);
  data.tools[1].lockSha256 = 'CONTAINER_REQUIRED'; writeFileSync(file, JSON.stringify(data));
  assert.throws(() => toolContract(root), /frozen generator lock/);
});
