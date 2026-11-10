import { cp, mkdir, mkdtemp, readFile, readdir, rm, symlink, writeFile } from 'node:fs/promises';
import { spawn } from 'node:child_process';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { checkConsumers } from './check-consumers.mjs';
import { filesBelow } from './inventory.mjs';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');

function run(command, args, cwd) {
  return new Promise((accept, reject) => {
    const child = spawn(command, args, { cwd, stdio: 'inherit', env: {
      ...process.env,
      IMPLUVIUM_GIT_REF: process.env.IMPLUVIUM_GIT_REF ?? 'typecheck',
      IMPLUVIUM_VERSION: process.env.IMPLUVIUM_VERSION ?? '0.0.0-test',
    } });
    child.on('error', reject);
    child.on('exit', (code, signal) => code === 0 ? accept() : reject(new Error(`${command} failed (${signal ?? code})`)));
  });
}

// Work on a disposable copy: the real backend and its generated types remain
// available for the ordinary server typecheck and the Nitro contract comparison.
async function main() {
  await checkConsumers(root);
  const temporary = await mkdtemp(join(tmpdir(), 'independent-ui-'));
  const excluded = new Set(['node_modules', '.nuxt', '.output', '.data', '.git', 'server', 'prisma']);
  try {
    const app = join(temporary, 'server');
    await mkdir(app);
    for (const entry of await readdir(join(root, 'server'))) {
      if (excluded.has(entry) || entry.startsWith('.env') || entry.startsWith('web-api-')) continue;
      await cp(join(root, 'server', entry), join(app, entry), { recursive: true });
    }
    await mkdir(join(app, 'server'));
    await mkdir(join(temporary, 'libraries'), { recursive: true });
    await cp(join(root, 'libraries/base'), join(temporary, 'libraries/base'), {
      recursive: true,
      filter: (path) => !path.split('/').some((part) => ['node_modules', '.nuxt', '.output'].includes(part)),
    });
    await symlink(join(root, 'server/node_modules'), join(app, 'node_modules'), 'dir');
    await symlink(join(root, 'libraries/base/node_modules'), join(temporary, 'libraries/base/node_modules'), 'dir');
    // Only prepare declarations; never generate/build or start backend plugins.
    await run('pnpm', ['exec', 'nuxt', 'prepare'], app);
    const generated = await filesBelow(join(app, '.nuxt'));
    const removed = new Set();
    for (const file of generated.filter((path) => path.endsWith('.d.ts'))) {
      const source = await readFile(file, 'utf8');
      if (/nitro[^/]*\.d\.ts$/.test(file) || /(?:interface InternalApi|declare module ["']nitropack\/types["'])/.test(source)) {
        removed.add(file);
        await rm(file);
      }
    }
    for (const file of generated.filter((path) => /\.(?:ts|json)$/.test(path) && !removed.has(path))) {
      const source = await readFile(file, 'utf8');
      // Nuxt's aggregate declarations may reference removed Nitro declarations.
      const clean = source.split('\n').filter((line) => ![...removed].some((target) =>
        line.includes(target.slice(target.lastIndexOf('/') + 1)))).join('\n');
      if (clean !== source) await writeFile(file, clean);
    }
    if ((await filesBelow(join(app, 'server'))).length) throw new Error('The isolated server tree is not empty');
    if ((await filesBelow(join(app, '.nuxt'))).some((file) => /nitro.*\.d\.ts$/.test(file))) {
      throw new Error('Generated Nitro declarations survived UI isolation');
    }
    await writeFile(join(app, 'tsconfig.ui.json'), JSON.stringify({
      extends: './.nuxt/tsconfig.app.json',
      compilerOptions: { allowImportingTsExtensions: true, exactOptionalPropertyTypes: false },
    }, null, 2));
    await run('pnpm', ['exec', 'vue-tsc', '--noEmit', '-p', 'tsconfig.ui.json'], app);
    console.log('All UI scripts and Vue templates checked with an empty server tree and no generated Nitro route types.');
  } finally {
    await rm(temporary, { recursive: true, force: true });
  }
}

main().catch((error) => { console.error(error.message); process.exitCode = 1; });
