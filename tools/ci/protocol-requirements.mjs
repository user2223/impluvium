import path from 'node:path';
import { plainObject } from './common.mjs';
import { fail } from './protocol-contract.mjs';

const sorted = values => [...new Set(values)].sort();
const strings = values => Array.isArray(values) && values.every(value => typeof value === 'string' && value);
const key = row => JSON.stringify(row.slice(0, row[1] === 'dependency' ? 6 : 3));

export function workspaceRequirements(metadata, root) {
  const rows = [];
  for (const pkg of metadata.packages.filter(pkg => pkg.source == null && metadata.workspace_members.includes(pkg.id))) {
    const manifest = path.relative(root, pkg.manifest_path).split(path.sep).join('/');
    if (manifest.startsWith('engine/')) continue;
    if (path.isAbsolute(manifest) || manifest.split('/').includes('..') || !Array.isArray(pkg.dependencies) || !plainObject(pkg.features)) fail('incomplete local dependency declarations');
    for (const dep of pkg.dependencies) {
      if (typeof dep.name !== 'string' || !dep.name || (dep.rename !== null && typeof dep.rename !== 'string') ||
          ![null, 'normal', 'build', 'dev'].includes(dep.kind) || (dep.target !== null && typeof dep.target !== 'string') ||
          typeof dep.optional !== 'boolean' || typeof dep.uses_default_features !== 'boolean' || !strings(dep.features)) fail('incomplete declared dependency');
      rows.push([manifest, 'dependency', dep.name, dep.rename ?? dep.name, dep.kind ?? 'normal', dep.target,
        dep.optional, dep.uses_default_features, sorted(dep.features)]);
    }
    // Freeze local feature indirections too: a new default -> local-feature
    // edge can activate an unchanged dependency forwarding entry.
    for (const [name, values] of Object.entries(pkg.features)) {
      if (!strings(values)) fail('incomplete declared feature forwarding');
      rows.push([manifest, 'feature', name, sorted(values)]);
    }
  }
  return rows.sort((a, b) => JSON.stringify(a).localeCompare(JSON.stringify(b)));
}

export function requirementIssues(base, current) {
  const validate = (rows, baseline = false) => {
    if (!Array.isArray(rows) || (baseline && !rows.length)) fail('legacy requirements need recapture from the actual base');
    const table = new Map();
    for (const row of rows) {
      if (!Array.isArray(row) || typeof row[0] !== 'string' || row[0].startsWith('engine/') || path.posix.isAbsolute(row[0]) ||
          row[0].split('/').some(part => !part || part === '.' || part === '..') || (row[0] !== 'Cargo.toml' && !row[0].endsWith('/Cargo.toml')) ||
          (row[1] === 'dependency' ? row.length !== 9 || !row.slice(2, 4).every(value => typeof value === 'string' && value) ||
            !['normal', 'build', 'dev'].includes(row[4]) || (row[5] !== null && typeof row[5] !== 'string') ||
            typeof row[6] !== 'boolean' || typeof row[7] !== 'boolean' || !strings(row[8]) :
            row[1] !== 'feature' || row.length !== 4 || typeof row[2] !== 'string' || !row[2] || !strings(row[3]))) fail('invalid legacy requirement');
      const identity = key(row);
      if (table.has(identity)) fail('duplicate legacy requirement');
      table.set(identity, row);
    }
    return table;
  };
  const before = validate(base, true), after = validate(current), issues = [];
  for (const [identity, row] of after) {
    const old = before.get(identity), features = row.at(-1);
    const added = features.filter(feature => !old || !(old.at(-1).includes(feature) ||
      (feature.includes('?/') && old.at(-1).includes(feature.replace('?/', '/')))));
    const admitted = old && !added.length;
    if (!admitted || (row[1] === 'dependency' && ((old[6] && !row[6]) || (!old[7] && row[7])))) {
      const delta = [old ? `added features ${JSON.stringify(added)}` : `new declaration with features ${JSON.stringify(features)}`];
      if (old && row[1] === 'dependency') for (const [index, label] of [[6, 'optional'], [7, 'uses_default_features']]) {
        if (old[index] !== row[index]) delta.push(`${label}: ${old[index]} -> ${row[index]}`);
      }
      issues.push(`legacy requirement added or expanded: ${identity}; ${delta.join('; ')}; review and recapture the base before updating its requirements`);
    }
  }
  return issues;
}
