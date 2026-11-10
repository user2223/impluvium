import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { cp, mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';

const output = resolve(process.argv[2] ?? 'server/.output/server');
const temporary = await mkdtemp(join(tmpdir(), 'static-startup-'));
try {
  const entry = join(temporary, 'server/index.mjs');
  const publicOutput = join(temporary, 'public');
  await mkdir(join(temporary, 'server'));
  // Check the deployed bootstrap and bytes. The --check-static-output path must
  // work without importing Nitro, its packages or any backend service.
  await cp(join(output, 'server/index.mjs'), entry);
  await cp(join(output, 'public'), publicOutput, { recursive: true });
  const manifestFile = join(publicOutput, 'csp-hashes.json');
  const original = await readFile(manifestFile, 'utf8');
  const execute = () => new Promise((accept, reject) => {
    const child = spawn(process.execPath, [entry, '--check-static-output'], { timeout: 30000 });
    let log = '';
    child.stdout.on('data', (bytes) => { log += bytes; });
    child.stderr.on('data', (bytes) => { log += bytes; });
    child.on('error', reject);
    child.on('exit', (code, signal) => signal
      ? reject(new Error(`Static startup check was terminated by ${signal}`)) : accept({ code, log }));
  });
  const healthy = await execute();
  assert.equal(healthy.code, 0, healthy.log);
  for (const mode of ['missing', 'invalid-json', 'invalid-schema', 'wrong-build-hash']) {
    await writeFile(manifestFile, original);
    let expected;
    if (mode === 'missing') {
      await rm(manifestFile);
      expected = /ENOENT[\s\S]*csp-hashes\.json/;
    } else if (mode === 'invalid-json') {
      await writeFile(manifestFile, '{');
      expected = /SyntaxError/;
    } else if (mode === 'invalid-schema') {
      await writeFile(manifestFile, '{}');
      expected = /Missing or invalid static output manifest/;
    } else {
      const changed = JSON.parse(original);
      if (changed.scripts['index.html'].length) {
        const before = changed.scripts['index.html'];
        changed.scripts['index.html'] = before.map((hash) =>
          hash.slice(0, 8) + (hash[8] === 'A' ? 'B' : 'A') + hash.slice(9));
        assert.notDeepEqual(changed.scripts['index.html'], before, 'The negative control changed nothing');
        expected = /script hashes do not match the manifest/;
      } else {
        // New framework output may contain no inline scripts. Its HTML bytes
        // are still bound to the manifest and must fail an incorrect hash.
        const before = changed.files['index.html'].sha256;
        changed.files['index.html'].sha256 = (before[0] === 'A' ? 'B' : 'A') + before.slice(1);
        assert.notEqual(changed.files['index.html'].sha256, before);
        expected = /does not match its manifest/;
      }
      await writeFile(manifestFile, JSON.stringify(changed));
    }
    const rejected = await execute();
    assert.notEqual(rejected.code, 0, `Startup accepted ${mode}`);
    assert.match(rejected.log, expected, `${mode} failed for an unrelated reason: ${rejected.log}`);
  }
  await writeFile(manifestFile, original);
  const restored = await execute();
  assert.equal(restored.code, 0, restored.log);
  console.log('Deployed startup checks passed: valid, missing, malformed, invalid-schema, incorrect build hashes, restored.');
} finally {
  await rm(temporary, { recursive: true, force: true });
}
