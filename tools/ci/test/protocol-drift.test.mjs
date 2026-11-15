import test from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import { appendFileSync, copyFileSync, existsSync, mkdirSync, writeFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { temporary } from './helpers.mjs';
import { yamlData } from '../pnpm-licences.mjs';

const workflow = yamlData(fileURLToPath(new URL('../../../.gitea/workflows/ci.yml', import.meta.url)));
const block = workflow.jobs.checks.steps.find(step => step.name === 'Protocol drift (compare unchanged checked-in outputs)').run;
const generated = 'engine/crates/protocol/src/generated/buffa/example.rs';
for (const mutation of ['none', 'generation repairs committed drift', 'staged repair', 'assume-unchanged repair', 'skip-worktree repair', 'ignored untracked output']) test(`exact drift block: ${mutation}`, t => {
  const root = temporary(t), bin = temporary(t), file = path.join(root, generated);
  mkdirSync(path.dirname(file), { recursive: true });
  mkdirSync(path.join(root, 'tools/ci'), { recursive: true });
  for (const name of ['export-protocol-source', 'protocol-source', 'protocol-contract', 'protocol-data', 'protocol-negative-lock', 'toml', 'common']) {
    copyFileSync(new URL(`../${name}.mjs`, import.meta.url), path.join(root, 'tools/ci', `${name}.mjs`));
  }
  writeFileSync(file, mutation.includes('repair') ? 'pub struct Example; \n' : 'pub struct Example;\n');
  const git = args => {
    const result = spawnSync('git', ['-c', 'core.autocrlf=false', '-c', 'core.hooksPath=/dev/null',
      '-c', 'user.name=Test', '-c', 'user.email=test@example.invalid', ...args], { cwd: root, encoding: 'utf8' });
    assert.ifError(result.error); assert.equal(result.status, 0, result.stderr);
    return result.stdout.trim();
  };
  git(['init', '--quiet']); git(['config', 'core.autocrlf', 'false']);
  git(['add', '--force', '--', generated, 'tools/ci']); git(['commit', '--no-gpg-sign', '--quiet', '-m', 'Fixture']);
  const sha = git(['rev-parse', 'HEAD']);
  // These are the checkout-local output locations used before CI's drift step.
  for (const directory of ['node_modules/.pnpm', 'engine/protocol/ts/node_modules', 'target']) {
    mkdirSync(path.join(root, directory), { recursive: true }); writeFileSync(path.join(root, directory, 'fixture-output'), 'output');
  }
  // Stand in for the generators without invoking a compiler or code generator.
  // The committed bytes differ from the repaired bytes, exactly as a prior
  // generate step would leave them before the real drift check.
  writeFileSync(path.join(bin, 'cargo'), '#!/usr/bin/env node\nconst fs = require("node:fs");\n' +
    `if (process.argv.at(-1) === 'generate') fs.writeFileSync(${JSON.stringify(generated)}, ${JSON.stringify('pub struct Example;\n')});\n` +
    'else if (process.argv.at(-1) === "check-drift") fs.writeFileSync("drift-ran", "yes");\nelse process.exit(1);\n', { mode: 0o755 });
  const env = { ...process.env, GITHUB_SHA: sha, RUNNER_TEMP: bin, PATH: bin + path.delimiter + process.env.PATH };
  for (const flag of ['assume-unchanged', 'skip-worktree']) if (mutation === `${flag} repair`) {
    git(['update-index', `--${flag}`, '--', generated]);
    assert.match(git(['ls-files', '-v', '--', generated]), flag === 'assume-unchanged' ? /^h / : /^S /);
  }
  if (mutation.includes('repair')) {
    const generatedRun = spawnSync('cargo', ['run', '--locked', '--offline', '-p', 'impluvium-protocol-xtask', '--', 'generate'], { cwd: root, env });
    assert.ifError(generatedRun.error); assert.equal(generatedRun.status, 0);
    if (mutation === 'staged repair') git(['add', '--', generated]);
  }
  if (mutation === 'ignored untracked output') {
    appendFileSync(path.join(root, '.git/info/exclude'), '\n*.extra\n'); writeFileSync(file + '.extra', 'untracked');
  }
  const result = spawnSync('bash', ['--noprofile', '--norc', '-euo', 'pipefail', '-c', block], { cwd: root, env, encoding: 'utf8' });
  assert.ifError(result.error); assert.equal(result.signal, null);
  assert.equal(result.status === 0, mutation === 'none', result.stdout + result.stderr);
  assert.equal(existsSync(path.join(root, 'drift-ran')), mutation === 'none');
  if (mutation.includes('repair')) assert.match(result.stderr, /source content differs/);
  if (mutation === 'ignored untracked output') assert.match(result.stderr, /unattested source input/);
  assert.equal(existsSync(path.join(bin, 'protocol-drift-source.json')), mutation === 'none');
});
