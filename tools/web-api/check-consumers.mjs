import { readFile } from 'node:fs/promises';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { filesBelow } from './inventory.mjs';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
export const uiDirectories = ['components', 'pages', 'composables', 'utils', 'layouts', 'plugins', 'middleware', 'shared'];
const serverRenderingAccess = [
  /\b(?:import\.meta|process)\s*\.\s*(?:server|client)\b/,
  /\b(?:useRequestHeaders?|getRequestHeaders?|useRequestEvent|useRequestFetch|useRequestURL|useResponseHeader)\b/,
  /\b(?:onServerPrefetch|useSSRContext|onPrehydrate|useHydration|defineLazyHydrationComponent)\b/,
  /\b(?:ssrContext|isHydrating|serverRendered)\b/,
  /\bhydrate-on-[\w-]+\b/,
  /\b(?:api-fetch-transfer|api-fetch-credentials)\b/,
  /<\/?(?:ClientOnly|client-only|NuxtClientFallback|nuxt-client-fallback)\b/,
];

export function checkConsumer(source, file) {
  if (/\$dropFetch\b/.test(source)) throw new Error(`Obsolete fetch access in ${file}`);
  if (/\$apiFetch\s*</.test(source)) throw new Error(`Free fetch return type in ${file}`);
  if (serverRenderingAccess.some((pattern) => pattern.test(source))) {
    throw new Error(`Server rendering or hydration access in browser UI: ${file}`);
  }
  if (/\b(?:TransferEntry|findTransfer|transferBodyIdentity|isTransferResponse|mergeRequestHeaders)\b/.test(source)) {
    throw new Error(`Obsolete response transfer helper in browser UI: ${file}`);
  }
  for (const match of source.matchAll(/\b(?:from|import)\s*(?:\(\s*)?["']([^"']+)["']/g)) {
    const name = match[1];
    const relative = name.startsWith('.') ? resolve(dirname(file), name) : '';
    if (/^(?:~\/|@\/)(?:server|prisma)(?:\/|$)|^nitropack(?:\/|$)/.test(name) ||
        /\/server\/(?:server|prisma)(?:\/|$)/.test(relative)) {
      throw new Error(`Backend import in ${file}: ${name}`);
    }
  }
}

export async function checkConsumers(directory = root) {
  const paths = (await Promise.all(uiDirectories.map((name) => filesBelow(join(directory, 'server', name))))).flat();
  paths.push(join(directory, 'server/app.vue'), join(directory, 'server/error.vue'));
  let count = 0;
  for (const file of paths.filter((path) => /\.(?:vue|[cm]?[jt]sx?)$/.test(path))) {
    checkConsumer(await readFile(file, 'utf8'), file);
    count++;
  }
  return count;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  checkConsumers().then((count) => console.log(`${count} UI source files use independent HTTP contracts.`))
    .catch((error) => { console.error(error.message); process.exitCode = 1; });
}
