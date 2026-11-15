import path from 'node:path';
import { existsSync, mkdirSync, writeFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { cli, required, stringOption } from './common.mjs';
import { externalOutput, fail } from './protocol-contract.mjs';

export function baseSelection(event, ref, prBase, before) {
  const value = event === 'pull_request' ? prBase : event === 'push' && ref.startsWith('refs/heads/') ? before : null;
  if (!value || /^0{40}$/.test(value)) return { status: 'unavailable', sha: null, reason: 'This event provides no comparison base.' };
  if (!/^[a-f0-9]{40}$/.test(value)) fail('event comparison base is not a commit SHA');
  return { status: 'available', sha: value };
}
export function capture(options) {
  required(options, 'root', 'event', 'ref', 'current-sha', 'out');
  if (!/^[a-f0-9]{40}$/.test(options['current-sha'])) fail('current-sha must be a commit SHA');
  const root = path.resolve(options.root), out = externalOutput(root, options.out);
  if (existsSync(out)) fail('base export directory must be fresh');
  mkdirSync(out, { recursive: true });
  const run = (program, args) => {
    const result = spawnSync(program, args, { cwd: root, encoding: 'utf8' });
    if (result.error || result.signal || result.status !== 0) fail(`base export failed: ${program}: ${result.stderr}`);
    return result.stdout.trim();
  };
  if (run('git', ['rev-parse', 'HEAD']) !== options['current-sha']) fail('checkout and declared source SHA differ');
  const base = baseSelection(options.event, options.ref, options['pr-base'], options.before);
  if (base.status === 'available') {
    run('git', ['fetch', '--no-tags', '--depth=1', 'origin', base.sha]);
    if (run('git', ['rev-parse', '--verify', `${base.sha}^{commit}`]) !== base.sha) fail('base object differs');
    base.exportRoot = path.join(out, 'base');
    mkdirSync(base.exportRoot);
    const archive = path.join(out, 'base.tar');
    run('git', ['archive', '--format=tar', '-o', archive, base.sha]);
    run('tar', ['-xf', archive, '-C', base.exportRoot]);
  }
  writeFileSync(path.join(out, 'source.json'), JSON.stringify({ version: 1, event: options.event, currentSha: options['current-sha'], base }, null, 2) + '\n', { flag: 'wx' });
  return [];
}
await cli(import.meta.url, `Usage: node tools/ci/protocol-base.mjs --root DIR --event EVENT --ref REF --current-sha SHA --out DIR [--pr-base SHA] [--before SHA]
Export the actual event base separately and record its SHA or an explicit no-comparison reason. Never use HEAD as a substitute.
Exit: 0 captured, 2 usage, input or export error.`, {
  root: stringOption, event: stringOption, ref: stringOption, 'current-sha': stringOption, out: stringOption, 'pr-base': stringOption, before: stringOption,
}, capture);
