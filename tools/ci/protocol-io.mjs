import { createHash } from 'node:crypto';
import { closeSync, createWriteStream, openSync, readSync, statSync } from 'node:fs';
import { spawn } from 'node:child_process';
import { Transform } from 'node:stream';
import { pipeline } from 'node:stream/promises';
import { StringDecoder } from 'node:string_decoder';
import { fail } from './protocol-contract.mjs';

export const treeLimit = 2 * 1024 * 1024 * 1024;
const lineLimit = 1024 * 1024;

// Keep raw expansion evidence without allocating a string for the whole tree.
export function* textLines(file, limit = treeLimit) {
  if (statSync(file).size > limit) fail(`evidence exceeds byte limit: ${file}`);
  const fd = openSync(file, 'r'), buffer = Buffer.alloc(64 * 1024), decoder = new StringDecoder('utf8');
  let pending = '', total = 0;
  try {
    let size;
    while ((size = readSync(fd, buffer, 0, buffer.length, null))) {
      total += size;
      if (total > limit) fail(`evidence exceeds byte limit: ${file}`);
      pending += decoder.write(buffer.subarray(0, size));
      let offset;
      while ((offset = pending.indexOf('\n')) >= 0) {
        if (Buffer.byteLength(pending.slice(0, offset)) > lineLimit) fail(`evidence line exceeds byte limit: ${file}`);
        yield pending.slice(0, offset).replace(/\r$/, '');
        pending = pending.slice(offset + 1);
      }
      if (Buffer.byteLength(pending) > lineLimit) fail(`evidence line exceeds byte limit: ${file}`);
    }
    pending += decoder.end();
    if (pending) yield pending.replace(/\r$/, '');
  } finally { closeSync(fd); }
}

export function fileDigest(file, limit = treeLimit) {
  const fd = openSync(file, 'r'), buffer = Buffer.alloc(64 * 1024), digest = createHash('sha256');
  let bytes = 0;
  try {
    let size;
    while ((size = readSync(fd, buffer, 0, buffer.length, null))) {
      bytes += size;
      if (bytes > limit) fail(`evidence exceeds byte limit: ${file}`);
      digest.update(buffer.subarray(0, size));
    }
    return { bytes, sha256: digest.digest('hex') };
  } finally { closeSync(fd); }
}

export function boundedStream(limit, digest) {
  let bytes = 0;
  return new Transform({ transform(chunk, encoding, callback) {
    bytes += chunk.length;
    if (bytes > limit) { callback(new Error('command output exceeds byte limit')); return; }
    digest?.update(chunk);
    callback(null, chunk);
  } });
}

export async function captureFile(command, args, cwd, env, file, limit = treeLimit) {
  const child = spawn(command, args, { cwd, env, stdio: ['ignore', 'pipe', 'pipe'] });
  const completion = new Promise((resolve, reject) => {
    child.once('error', reject);
    child.once('close', (status, signal) => {
      if (status !== 0 || signal) reject(new Error(`command failed (${status}, ${signal ?? 'no signal'}): ${command}`));
      else resolve();
    });
  });
  const digest = createHash('sha256');
  const streams = [
    pipeline(child.stdout, boundedStream(limit, digest), createWriteStream(file, { flags: 'wx' })),
    pipeline(child.stderr, boundedStream(16 * 1024 * 1024), createWriteStream(file + '.stderr', { flags: 'wx' })),
  ];
  try { await Promise.all([completion, ...streams]); }
  catch (error) {
    child.kill('SIGKILL');
    await Promise.allSettled([completion, ...streams]);
    fail(`${error.message}; inspect partial evidence at ${file}`);
  }
  return { bytes: statSync(file).size, sha256: digest.digest('hex') };
}
