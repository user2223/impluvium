#!/usr/bin/env node
import { appendFileSync, readFileSync, realpathSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';

// cc's command display omits cwd; record the actual delegated invocation.
const launcher = fileURLToPath(import.meta.url);
const config = JSON.parse(readFileSync(launcher + '.json', 'utf8'));
const args = process.argv.slice(2);
const context = (config.target.includes('windows') && config.role.startsWith('target')) ||
  [process.env.OUT_DIR ?? '', ...args].some(value => value.includes(`/${config.target}/`)) ? 'target' : 'host';
const result = spawnSync(config.executable, args, { stdio: 'inherit' });
appendFileSync(config.log, JSON.stringify({ launcher, executable: realpathSync(config.executable), command: config.executable,
  args, cwd: process.cwd(), context, role: context + config.role.replace(/^(host|target)/, ''),
  packageName: process.env.CARGO_PKG_NAME, packageVersion: process.env.CARGO_PKG_VERSION,
  manifestDir: process.env.CARGO_MANIFEST_DIR, status: result.status, signal: result.signal, error: result.error?.message ?? null }) + '\n');
process.exitCode = result.status ?? 1;
