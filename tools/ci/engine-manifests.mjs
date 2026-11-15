import path from 'node:path';
import { existsSync, readdirSync } from 'node:fs';
import { InputError, problem } from './common.mjs';

export function engineManifests(root, source, name = 'Cargo.toml') {
  if (source) return source.files.map(file => file.path).filter(file => file.startsWith('engine/') && file.endsWith('/' + name));
  // Licence checks also run before source attestation. Search every real
  // directory here; build-output names must not hide independent workspaces.
  const result = [];
  const visit = directory => {
    for (const entry of readdirSync(path.join(root, directory), { withFileTypes: true })) {
      const file = `${directory}/${entry.name}`;
      if (entry.isDirectory()) visit(file);
      else if (entry.name === name) {
        if (!entry.isFile()) throw new InputError(problem(file, 1, 'engine manifests must be regular files'));
        result.push(file);
      }
    }
  };
  if (existsSync(path.join(root, 'engine'))) visit('engine');
  return result.sort();
}
