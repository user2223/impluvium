import { readFile } from 'node:fs/promises';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { filesBelow, readInventory, redirectRules } from '../web-api/inventory.mjs';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
try {
  const inventory = await readInventory(root);
  const directory = join(root, 'server/shared/api/generated');
  const files = (await filesBelow(directory)).map((file) => file.slice(directory.length + 1)).sort();
  if (JSON.stringify(files) !== JSON.stringify(['dtos.ts', 'enums.ts', 'inventory.json', 'routes.ts'])) {
    throw new Error('Generated HTTP contract file set differs.');
  }
  const generated = JSON.parse(await readFile(join(directory, 'inventory.json'), 'utf8'));
  const identity = (routes) => routes.map(({ file, method, path }) => ({ file, method, path }));
  const rules = (items) => items.map(({ kind, path, target }) => ({ kind, path, target }));
  if (JSON.stringify(identity(inventory)) !== JSON.stringify(identity([...generated.handlers, ...generated.serverRoutes])) ||
      JSON.stringify(rules(redirectRules)) !== JSON.stringify(rules(generated.redirectRules))) {
    throw new Error('Generated route inventory differs; regenerate the HTTP contracts.');
  }
  for (const file of ['dtos.ts', 'enums.ts', 'routes.ts']) {
    const code = await readFile(join(directory, file), 'utf8');
    if (/\b(any|unknown)\b/.test(code)) throw new Error(`Unresolved generated contract in ${file}`);
    for (const match of code.matchAll(/\b(?:from|import)\s*(?:\(\s*)?["']([^"']+)["']/g)) {
      if (match[1] !== './dtos') throw new Error(`Generated contract imports a backend type: ${match[1]}`);
    }
  }
  console.log(`${inventory.length} handlers and ${redirectRules.length} redirect rule checked.`);
} catch (error) {
  console.error(error.message);
  process.exitCode = 1;
}
