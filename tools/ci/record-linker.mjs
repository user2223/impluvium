#!/usr/bin/env node
import { appendFileSync, readFileSync, realpathSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';

// Capture copies this small launcher beside a private per-run configuration.
// The delegated linker remains the previously resolved compiler driver.
const config = JSON.parse(readFileSync(fileURLToPath(import.meta.url) + '.json', 'utf8'));
const executable = realpathSync(config.executable);
const args = process.argv.slice(2);
if (!['host', 'target'].includes(config.context)) throw new Error('missing linker delegation context');
const result = spawnSync(config.executable, args, { stdio: 'inherit' });
appendFileSync(config.log, JSON.stringify({ executable, command: config.executable, args, cwd: process.cwd(),
  launcher: fileURLToPath(import.meta.url), context: config.context,
  status: result.status, signal: result.signal, error: result.error?.message ?? null }) + '\n');
process.exitCode = result.status ?? 1;
