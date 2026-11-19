import { spawn } from 'node:child_process';
import { copyFile, cp, mkdir, readdir, readFile, rm, writeFile } from 'node:fs/promises';
import { findPackageJSON } from 'node:module';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { flagCodepoints } from '../../server/utils/locale-flags.ts';
import { createSpaManifest, validateSpaManifest } from '../../server/server/utils/spa-contract.ts';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const app = join(root, 'server');
const [profile] = process.argv.slice(2);
if (!['server', 'spa', 'assemble'].includes(profile) || process.argv.length !== 3) {
  throw new Error('Usage: node tools/web-spa/build.mjs server|spa|assemble');
}

// The flags of the language selection are static files under /flags, copied from the emoji package
// for exactly the shipped languages, so that they load without sign-in (also on the setup page).
async function copyFlags(output) {
  const manifest = findPackageJSON('@discordapp/twemoji', pathToFileURL(join(app, 'package.json')).href);
  if (!manifest) throw new Error('The emoji package is not installed');
  const source = join(dirname(manifest), 'dist', 'svg');
  const target = join(output, 'flags');
  await mkdir(target, { recursive: true });
  for (const codepoint of flagCodepoints) {
    await copyFile(join(source, `${codepoint}.svg`), join(target, `${codepoint}.svg`));
  }
}

if (profile === 'assemble') {
  const source = join(app, '.output/spa/public');
  await validateSpaManifest(source);
  const output = join(app, '.output/server');
  await rm(join(output, 'public'), { recursive: true, force: true });
  await cp(source, join(output, 'public'), { recursive: true });
  const entry = join(output, 'server/index.mjs');
  const code = await readFile(entry, 'utf8');
  const preflight = await readFile(join(root, 'tools/web-spa/preflight.mjs'), 'utf8');
  if (code.startsWith('// Static output preflight')) throw new Error('Output is already assembled; rebuild the server first');
  await writeFile(join(output, 'server/nitro-entry.mjs'), code);
  await writeFile(entry, '// Static output preflight\n' + preflight +
    '\nif (!process.argv.includes("--check-static-output")) await import("./nitro-entry.mjs");\n');
} else {
  if (profile === 'spa') {
    const empty = join(app, '.spa-server');
    await mkdir(empty, { recursive: true });
    if ((await readdir(empty)).length) throw new Error('The static export requires an empty server tree');
    await rm(join(app, '.output/spa'), { recursive: true, force: true });
  }
  const code = await new Promise((accept, reject) => {
    const child = spawn(join(app, 'node_modules/.bin/nuxt'), [profile === 'spa' ? 'generate' : 'build'], {
      cwd: app, stdio: 'inherit', env: { ...process.env, IMPLUVIUM_BUILD_PROFILE: profile },
    });
    child.on('error', reject);
    child.on('exit', (code, signal) => signal ? reject(new Error(`Nuxt ended with ${signal}`)) : accept(code));
  });
  if (code !== 0) process.exitCode = code ?? 1;
  else if (profile === 'spa') {
    const output = join(app, '.output/spa/public');
    await copyFlags(output);
    const manifest = await createSpaManifest(output);
    await writeFile(join(output, 'csp-hashes.json'), JSON.stringify(manifest, null, 2) + '\n');
    await validateSpaManifest(output);
    console.log(`Static output: ${Object.keys(manifest.files).length} files, ${Object.keys(manifest.scripts).length} HTML files, ${new Set(Object.values(manifest.scripts).flat()).size} script hashes.`);
  }
}
