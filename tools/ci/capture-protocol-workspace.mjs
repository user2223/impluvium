import path from 'node:path';
import { mkdirSync, existsSync, writeFileSync, readFileSync, realpathSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { cli, json, required, stringOption } from './common.mjs';
import { cargoMetadata } from './metadata.mjs';
import { contract, externalOutput, fail, hash } from './protocol-contract.mjs';
import { workspacePaths } from './protocol-workspace.mjs';
import { verifySource } from './protocol-source.mjs';
import { workspaceRequirements } from './protocol-requirements.mjs';

export function legacyCandidate(report) {
  // Only an export of the actual base can propose baseline exceptions.
  if (!report.source || report.source.mutation) fail('baseline needs an independently attested unmodified export');
  verifySource(report.root, report.source, report.sourceSha);
  if (report.sourceSha !== report.baseSha) return null;
  return { baseSha: report.baseSha, exceptions: report.paths, requirements: report.requirements };
}

export function capture(options) {
  required(options, 'root', 'source-sha', 'base-sha', 'out', 'source-receipt', 'contract');
  if (![options['source-sha'], options['base-sha']].every(value => /^[a-f0-9]{40}$/.test(value))) fail('workspace capture needs actual source and baseline SHAs');
  const root = realpathSync(options.root), out = externalOutput(root, options.out);
  const data = contract(options.contract), source = verifySource(root, json(options['source-receipt']), options['source-sha']);
  if (existsSync(out)) fail('workspace results must be fresh');
  mkdirSync(out, { recursive: true });
  const report = { version: 3, scope: 'all-targets-conservative', source, sourceSha: options['source-sha'], baseSha: options['base-sha'], root,
    lockSha256: hash(readFileSync(path.join(root, 'Cargo.lock'))), metadata: {}, paths: [] };
  const args = ['metadata', '--locked', '--offline', '--all-features', '--format-version=1'];
  const file = path.join(out, 'all.json');
  const result = spawnSync('cargo', args, { cwd: root, encoding: 'utf8', maxBuffer: 128 * 1024 * 1024 });
  if (result.status !== 0 || result.error || result.signal) fail(`workspace metadata failed: ${result.stderr}`);
  writeFileSync(file, result.stdout, { flag: 'wx' });
  report.metadata.all = { file, arguments: args, sha256: hash(result.stdout) };
  const metadata = cargoMetadata(file, { resolved: true });
  report.paths = workspacePaths(metadata, 'all', root, data, source);
  report.requirements = workspaceRequirements(metadata, root);
  verifySource(root, source, options['source-sha']);
  writeFileSync(path.join(out, 'workspace.json'), JSON.stringify(report, null, 2) + '\n', { flag: 'wx' });
  const candidate = legacyCandidate(report);
  if (candidate) writeFileSync(path.join(out, 'legacy-candidate.json'), JSON.stringify(candidate, null, 2) + '\n', { flag: 'wx' });
  return [];
}
await cli(import.meta.url, `Usage: node tools/ci/capture-protocol-workspace.mjs --root DIR --source-sha SHA --base-sha SHA --contract FILE --source-receipt FILE --out DIR
Capture unfiltered all-feature workspace metadata and every inherited forbidden path conservatively, without a depth limit.
Capture the real base separately before reviewing legacy-candidate.json. Never automatically approve the current graph.
Exit: 0 captured, 2 usage, input or tool error.`, {
  root: stringOption, 'source-sha': stringOption, 'base-sha': stringOption, out: stringOption, 'source-receipt': stringOption, contract: stringOption,
}, capture);
