import { lstatSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, realpathSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { cli, InputError, json, required, stringOption } from './common.mjs';

const CONFIG = 'version: v2\nmodules:\n  - path: proto\nbreaking:\n  use:\n    - WIRE_JSON\n';
const SHA = /^[a-f0-9]{40}$/;

function metadata(file) {
  try { return lstatSync(file); }
  catch (error) {
    if (error.code === 'ENOENT') return null;
    throw error;
  }
}

function within(parent, child) {
  const relative = path.relative(parent, child);
  return relative === '' || (!path.isAbsolute(relative) && relative !== '..' && !relative.startsWith(`..${path.sep}`));
}

function copySchema(source, target) {
  const info = metadata(source);
  if (!info?.isDirectory() || info.isSymbolicLink()) throw new InputError(`expected schema directory: ${source}`);
  mkdirSync(target, { recursive: true });
  let count = 0;
  for (const entry of readdirSync(source, { withFileTypes: true })) {
    const input = path.join(source, entry.name);
    const output = path.join(target, entry.name);
    if (entry.isDirectory()) count += copySchema(input, output);
    else if (entry.isFile()) {
      writeFileSync(output, readFileSync(input), { flag: 'wx' });
      if (entry.name.endsWith('.proto')) count++;
    } else throw new InputError(`unsupported schema file type: ${input}`);
  }
  return count;
}

function prepare(source, target) {
  if (copySchema(path.join(source, 'proto'), path.join(target, 'proto')) === 0) {
    throw new InputError(`schema is empty: ${source}`);
  }
  // Historical schemas need no generator configuration or BSR dependency.
  writeFileSync(path.join(target, 'buf.yaml'), CONFIG, { flag: 'wx' });
}

function invoke(cwd, args, out, name) {
  const result = spawnSync('buf', args, { cwd, encoding: 'utf8', maxBuffer: 64 * 1024 * 1024 });
  writeFileSync(path.join(out, `${name}.stdout`), result.stdout ?? '', { flag: 'wx' });
  writeFileSync(path.join(out, `${name}.stderr`), result.stderr ?? '', { flag: 'wx' });
  writeFileSync(path.join(out, `${name}.json`), JSON.stringify({
    command: ['buf', ...args], cwd, status: result.status, signal: result.signal,
  }, null, 2) + '\n', { flag: 'wx' });
  if (result.error || result.signal || result.status === null) {
    throw new InputError(`buf ${name} could not complete: ${result.error?.message ?? result.signal}`);
  }
  return result;
}

function build(cwd, image, out, name) {
  const result = invoke(cwd, ['build', '-o', image], out, name);
  if (result.status !== 0 || !metadata(image)?.isFile()) throw new InputError(`buf ${name} failed (status ${result.status})`);
}

export function diagnostics(stdout, stderr) {
  const result = [];
  for (const stream of [stdout, stderr]) {
    if (!stream.trim()) continue;
    let records;
    try {
      const parsed = JSON.parse(stream);
      records = Array.isArray(parsed) ? parsed : [parsed];
    } catch {
      try { records = stream.trim().split(/\r?\n/).map(line => JSON.parse(line)); }
      catch { throw new InputError('buf breaking returned malformed JSON diagnostics'); }
    }
    for (const record of records) {
      if (!record || typeof record.path !== 'string' || !record.path ||
          typeof record.type !== 'string' || !record.type || typeof record.message !== 'string' || !record.message) {
        throw new InputError('buf breaking returned an invalid file annotation');
      }
      result.push(record);
    }
  }
  return result;
}

function compare(cwd, image, out, name) {
  const result = invoke(cwd, ['breaking', '--against', image, '--error-format=json'], out, name);
  if (![0, 100].includes(result.status)) throw new InputError(`buf ${name} failed (status ${result.status})`);
  const annotations = diagnostics(result.stdout, result.stderr);
  if ((result.status === 100) !== (annotations.length > 0)) {
    throw new InputError(`buf ${name}: exit status and diagnostics disagree`);
  }
  return { status: result.status, annotations };
}

function sourceRecord(file) {
  const source = json(file);
  if (source.version !== 1 || !['pull_request', 'push', 'workflow_dispatch'].includes(source.event) ||
      !SHA.test(source.currentSha ?? '') || !source.base || !['available', 'unavailable'].includes(source.base.status)) {
    throw new InputError('source record needs version, event, currentSha and base.status');
  }
  if (source.base.status === 'available' && !SHA.test(source.base.sha ?? '')) {
    throw new InputError('available base needs its actual SHA');
  }
  if (source.base.status === 'available' &&
      (typeof source.base.exportRoot !== 'string' || !path.isAbsolute(source.base.exportRoot) ||
       !metadata(source.base.exportRoot)?.isDirectory())) {
    throw new InputError('available base needs an existing absolute exportRoot');
  }
  if (source.base.status === 'unavailable' &&
      (source.base.sha !== null || typeof source.base.reason !== 'string' || !source.base.reason.trim())) {
    throw new InputError('unavailable base needs sha: null and an explicit reason');
  }
  return source;
}

export function check(options) {
  required(options, 'mode', 'current', 'out');
  if (!['report', 'selftest'].includes(options.mode)) throw new InputError('--mode must be report or selftest');
  if (options.mode === 'report') required(options, 'base', 'source');
  else if (options.base || options.source) throw new InputError('selftest does not use --base or --source');
  const current = realpathSync(options.current);
  const base = options.base ? path.resolve(options.base) : null;
  const source = options.mode === 'report' ? sourceRecord(options.source) : null;
  if (source?.base.status === 'available' && base !== path.join(path.resolve(source.base.exportRoot), 'engine/protocol')) {
    throw new InputError('--base must name engine/protocol within the declared base exportRoot');
  }
  const destination = path.resolve(options.out);
  // Resolve existing ancestors so an output symlink cannot redirect writes into an export.
  let ancestor = destination;
  const suffix = [];
  while (!metadata(ancestor)) {
    suffix.unshift(path.basename(ancestor));
    ancestor = path.dirname(ancestor);
  }
  const out = path.join(realpathSync(ancestor), ...suffix);
  const protectedPaths = [realpathSync(path.resolve(current, '../..'))];
  if (source?.base.status === 'available') protectedPaths.push(realpathSync(source.base.exportRoot));
  if (base && metadata(base)) protectedPaths.push(realpathSync(base));
  for (const source of protectedPaths) {
    if (within(source, out) || within(out, source)) throw new InputError('output must be separate from both source exports');
  }
  if (metadata(out)) throw new InputError('--out must be a fresh results directory');
  mkdirSync(out, { recursive: true });
  const scratch = mkdtempSync(path.join(tmpdir(), 'impluvium-breaking-'));
  try {
    const active = path.join(scratch, 'current');
    prepare(current, active);
    let report;
    if (options.mode === 'report') {
      if (source.base.status === 'unavailable') {
        report = { result: 'no-comparison', reason: source.base.reason, source };
      } else {
        if (metadata(base) && !metadata(base).isDirectory()) throw new InputError('base protocol path is not a directory');
        if (!metadata(base) || !metadata(path.join(base, 'proto'))) {
          report = { result: 'no-comparison', reason: 'The base export has no protocol schema.', source };
        } else {
          const previous = path.join(scratch, 'base');
          prepare(base, previous);
          const image = path.join(out, 'base.binpb');
          build(previous, image, out, 'build-base');
          const comparison = compare(active, image, out, 'compare-base');
          report = { result: comparison.status === 0 ? 'compatible' : 'reported-incompatibility', source, ...comparison };
        }
      }
    } else {
      const image = path.join(out, 'current.binpb');
      build(active, image, out, 'build-current');
      if (compare(active, image, out, 'compare-unchanged').status !== 0) {
        throw new InputError('unchanged schema failed the breaking control');
      }
      const mutated = path.join(scratch, 'mutated');
      prepare(current, mutated);
      const file = path.join(mutated, 'proto/impluvium/v2/auth.proto');
      const original = readFileSync(file, 'utf8');
      const message = /message StartPairingRequest\s*\{[^}]*\}/g;
      const blocks = original.match(message);
      if (blocks?.length !== 1 || (blocks[0].match(/^  string device_name = 1;$/gm) ?? []).length !== 1) {
        throw new InputError('selftest field StartPairingRequest.device_name = 1 is missing or ambiguous');
      }
      const changed = original.replace(blocks[0], blocks[0].replace(/^  string device_name = 1;\r?\n/m, ''));
      if (changed === original) throw new InputError('selftest did not delete its field');
      writeFileSync(file, changed);
      const comparison = compare(mutated, image, out, 'compare-deleted-field');
      const expected = comparison.annotations.some(record =>
        record.path.endsWith('impluvium/v2/auth.proto') &&
        /field "1" with name "device_name" on message "StartPairingRequest" was deleted/.test(record.message));
      if (comparison.status !== 100 || !expected || comparison.annotations.some(record =>
        !/field "1" with name "device_name" on message "StartPairingRequest" was deleted/.test(record.message))) {
        throw new InputError('selftest did not report the expected field deletion exclusively');
      }
      report = { result: 'selftest-passed', ...comparison };
    }
    writeFileSync(path.join(out, 'result.json'), JSON.stringify({ version: 1, ...report }, null, 2) + '\n', { flag: 'wx' });
    console.log(`Protocol breaking: ${report.result}${report.reason ? ` (${report.reason})` : ''}`);
    return [];
  } finally {
    rmSync(scratch, { recursive: true, force: true });
  }
}

await cli(import.meta.url, `Usage: node tools/ci/check-protocol-breaking.mjs --mode report|selftest --current DIR --out DIR
Report additionally needs --base DIR --source FILE. Source schema: {version:1,event,currentSha,base:{status,sha,exportRoot?,reason?}}.
Report records incompatibilities without failing before the schema freeze. Selftest requires a clean comparison and
the specific deletion of StartPairingRequest.device_name. Both modes write only fresh scratch/results directories.
Exit: 0 completed (including explicit no-comparison), 2 usage, input or tool failure.`, {
  mode: stringOption, current: stringOption, base: stringOption, source: stringOption, out: stringOption,
}, check);
