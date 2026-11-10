import { readFile } from 'node:fs/promises';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

export const nonComparableFile = 'tools/web-api/non-comparable.json';

function routeEntries(entries) {
  if (!Array.isArray(entries)) throw new Error('Nitro comparison exclusions must be an array.');
  const routes = new Map();
  for (const entry of entries) {
    if (!entry || typeof entry !== 'object' ||
        Object.keys(entry).sort().join(',') !== 'method,path,reason' ||
        !['GET', 'HEAD', 'POST', 'PUT', 'PATCH', 'DELETE', 'OPTIONS', 'CONNECT', 'TRACE'].includes(entry.method) ||
        typeof entry.path !== 'string' || !entry.path.startsWith('/') || /[\s?#]/.test(entry.path) ||
        typeof entry.reason !== 'string' || !entry.reason.trim() || /[\r\n]/.test(entry.reason)) {
      throw new Error('Each Nitro comparison exclusion needs an HTTP method, a path and a single-line reason.');
    }
    const key = `${entry.method} ${entry.path}`;
    if (routes.has(key)) throw new Error(`Duplicate Nitro comparison exclusion: ${key}`);
    routes.set(key, entry);
  }
  return routes;
}

export function validateNonComparable(entries, previous) {
  const routes = routeEntries(entries);
  if (previous !== undefined) {
    const baseline = routeEntries(previous);
    const added = [...routes.keys()].filter((key) => !baseline.has(key));
    if (added.length) throw new Error(`Nitro comparison exclusions may only shrink; added: ${added.join(', ')}`);
  }
  return routes;
}

/** Exclusions apply only to a complete Nitro response of any, never an untyped field. */
export function comparisonPolicy(entries) {
  const routes = validateNonComparable(entries);
  const seen = new Set();
  return {
    check(key, nitroIsAny) {
      seen.add(key);
      const entry = routes.get(key);
      if (entry && !nitroIsAny) return {
        skip: false,
        problems: ['Nitro no longer returns any; remove this comparison exclusion.'],
      };
      if (nitroIsAny && !entry) return {
        skip: false,
        problems: ['Nitro returns any without a documented comparison exclusion.'],
      };
      return { skip: !!entry, problems: [] };
    },
    finish() {
      return [...routes.keys()].filter((key) => !seen.has(key))
        .map((key) => `Unused Nitro comparison exclusion: ${key}`);
    },
  };
}

async function main() {
  const args = process.argv.slice(2);
  if (args.length && (args.length !== 2 || args[0] !== '--previous')) {
    throw new Error('Usage: node tools/web-api/non-comparable.mjs [--previous FILE]');
  }
  const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
  const entries = JSON.parse(await readFile(join(root, nonComparableFile), 'utf8'));
  const previous = args.length ? JSON.parse(await readFile(args[1], 'utf8')) : undefined;
  const routes = validateNonComparable(entries, previous);
  console.log(`Nitro comparison exclusions checked: ${routes.size}${previous ? '; no additions to the base revision' : ''}.`);
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main().catch((error) => { console.error(error.message); process.exitCode = 1; });
}
