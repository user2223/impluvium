// SPDX-License-Identifier: 0BSD
import { readSelfCheckText } from '../src/loader.ts';
import { resolve, relative, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
const base = fileURLToPath(new URL('../', import.meta.url));
const prohibited = new Set(['node:fs','node:child_process','node:worker_threads','node:net','node:http','node:https','node:http2','node:dgram','node:tls','node:vm','node:module','node:inspector']);
export async function importInventory() {
  const graphs = {};
  for (const [graph, entry] of Object.entries({ local: 'tooling/stage.mjs', contract: 'tooling/check.mjs', container: 'container/run.mjs' })) {
    const pending = [resolve(base, entry)], visited = new Set(), edges = [];
    while (pending.length) {
      const file = pending.pop(); if (visited.has(file)) continue; visited.add(file);
      if (!file.startsWith(base)) throw Error('Import escapes reference root');
      const code = await readSelfCheckText(relative(base,file).split('\\').join('/'));
      if (/\b(?:eval|Function)\s*\(|\bimport\s*\(|\bimport\.meta\.resolve\b|\brequire\s*\(/.test(code)) throw Error('Disallowed or non-static runtime loading in ' + file);
      for (const match of code.matchAll(/^(?:import|export)\s*(type\s+)?(?:[^;]*?\bfrom\s*)?['"]([^'"]+)['"];?/gm)) {
        const specifier = match[2], kind = match[1] ? 'type' : 'runtime';
        const localName=relative(base,file).split('\\').join('/');
        if(prohibited.has(specifier)||specifier==='node:fs/promises'&&!['src/loader.ts','tooling/output.mjs'].includes(localName))throw Error('Forbidden builtin '+specifier+' in '+localName);
        if (!(specifier.startsWith('./') || specifier.startsWith('../') || specifier.startsWith('node:') || graph === 'container' && specifier === '@noble/hashes/blake3')) throw Error('Disallowed import ' + specifier);
        edges.push({ from: relative(base, file), specifier, kind });
        if (specifier.startsWith('.')) {
          if (!/\.(?:ts|mjs)$/.test(specifier)) throw Error('Explicit local extension required');
          pending.push(resolve(dirname(file), specifier));
        }
      }
    }
    graphs[graph] = { entry, modules: [...visited].map(path => relative(base, path)).sort(), edges };
  }
  const standardModules=[...new Set(Object.values(graphs).flatMap(graph=>graph.edges.filter(edge=>edge.specifier.startsWith('node:')).map(edge=>edge.specifier)))].sort();
  return { format: 'module-import-inventory-v1', graphs, standardModules,
    note: 'This source inventory lists runtime and type-only edges separately; dependency resolution is a separate validation step.',
    typeToolchain: ['pinned TypeScript', 'pinned Node type declarations', 'installed @noble/hashes declarations'] };
}
