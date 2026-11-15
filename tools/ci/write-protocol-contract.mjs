import path from 'node:path';
import { existsSync, writeFileSync, mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { cli, json, required, stringOption } from './common.mjs';
import { contract, equal, fail, matrixSelections } from './protocol-contract.mjs';
import { compactContract } from './protocol-data.mjs';
import { check } from './check-protocol-graph.mjs';
import { cargoMetadata } from './metadata.mjs';
import { permissionCandidate } from './protocol-evidence.mjs';

export function writeContract(options) {
  required(options, 'root', 'contract', 'captures', 'base', 'current', 'out', 'source-sha', 'base-sha');
  if (![options['source-sha'], options['base-sha']].every(value => /^[a-f0-9]{40}$/.test(value))) fail('writer requires full expected source and base commit IDs');
  const data = contract(options.contract), permissions = [];
  const base = json(options.base), current = json(options.current);
  if (base.sourceSha !== options['base-sha'] || base.baseSha !== options['base-sha'] || current.sourceSha !== options['source-sha'] ||
      base.source?.mutation || base.scope !== 'all-targets-conservative' || current.baseSha !== options['base-sha'] ||
      JSON.stringify(base.paths) !== JSON.stringify(current.paths)) fail('review a real base and identical conservative workspace paths before writing');
  const folders = [];
  for (const { profile, target, run } of matrixSelections) {
    const folder = path.join(options.captures, profile, target, run);
    const candidate = json(path.join(folder, 'permission-candidate.json'));
    if (candidate.profile !== profile || candidate.target !== target || candidate.run !== run) fail('candidate selection differs');
    permissions.push(candidate); folders.push(folder);
  }
  data.permissions = permissions; data.legacy = { baseSha: options['base-sha'], exceptions: base.paths, requirements: base.requirements };
  const scratch = mkdtempSync(path.join(tmpdir(), 'impluvium-contract-'));
  try {
    const file = path.join(scratch, 'contract.json'); writeFileSync(file, JSON.stringify(data));
    for (const [index, folder] of folders.entries()) {
      const issues = check({ root: options.root, contract: file, input: folder, 'source-sha': options['source-sha'] });
      if (issues.length) fail(issues.join('\n'));
      const expected = permissionCandidate(json(path.join(folder, 'graph.json')),
        cargoMetadata(path.join(folder, 'metadata.json')), json(path.join(folder, 'invocation.json')));
      if (!equal(permissions[index], expected)) fail('candidate differs from its complete observed package set');
    }
    for (const [root, workspace] of [[base.root, options.base], [options.root, options.current]]) {
      const issues = check({ root, workspace, contract: file, 'source-sha': workspace === options.base ? options['base-sha'] : options['source-sha'] });
      if (issues.length) fail(issues.join('\n'));
    }
    if (existsSync(options.out)) fail('contract output must be a fresh review file');
    writeFileSync(options.out, JSON.stringify(compactContract(data), null, 2) + '\n', { flag: 'wx' });
    return [];
  } finally { rmSync(scratch, { recursive: true, force: true }); }
}
await cli(import.meta.url, 'Usage: node tools/ci/write-protocol-contract.mjs --root DIR --contract FILE --source-sha SHA --base-sha SHA --captures DIR --base FILE --current FILE --out FILE\nValidate all 16 complete captures plus both workspace reports and write a deterministic compact review candidate. Never rewrites the input contract.', {
  root: stringOption, contract: stringOption, captures: stringOption, base: stringOption, current: stringOption, out: stringOption, 'source-sha': stringOption, 'base-sha': stringOption,
}, writeContract);
