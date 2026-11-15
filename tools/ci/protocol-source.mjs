import path from 'node:path';
import { createHash } from 'node:crypto';
import { lstatSync, readFileSync, readdirSync } from 'node:fs';
import { fail, hash } from './protocol-contract.mjs';
import { verifyNegativeLock } from './protocol-negative-lock.mjs';

export const gitObject = (type, bytes) => createHash('sha1').update(`${type} ${bytes.length}\0`).update(bytes).digest('hex');
const safe = file => typeof file === 'string' && file && !file.includes('\\') && !file.includes('\0') &&
  !path.posix.isAbsolute(file) && file.split('/').every(part => part && part !== '.' && part !== '..' && part !== '.git');
export const negativeCases = {
  gzip: ['engine/crates/protocol/Cargo.toml', 'connectrpc', 'gzip'],
  zstd: ['engine/crates/protocol/Cargo.toml', 'connectrpc', 'zstd'],
  'system-proxy': ['engine/crates/client/Cargo.toml', 'reqwest', 'system-proxy'],
  'native-tls': ['engine/crates/client/Cargo.toml', 'reqwest', 'native-tls'],
  ring: ['engine/crates/client/Cargo.toml', 'rustls', 'ring'],
};
export function mutateManifest(text, name) {
  const spec = negativeCases[name];
  if (!spec) fail('unknown negative case');
  const pattern = new RegExp(`^${spec[1]} = \\{[^\\n]+\\}$`, 'gm');
  const rows = text.match(pattern);
  if (rows?.length !== 1 || !rows[0].includes('default-features = false')) fail('negative manifest selection is missing or ambiguous');
  const row = rows[0];
  const changed = /features = \[[^\]]*\]/.test(row)
    ? row.replace(/features = \[([^\]]*)\]/, (_, body) => `features = [${body}${body.trim() ? ', ' : ''}"${spec[2]}"]`)
    : row.replace(/ \}$/, `, features = ["${spec[2]}"] }`);
  return text.replace(row, changed);
}

export function sourceTree(files) {
  const root = new Map();
  for (const file of files) {
    if (!safe(file.path) || !['100644', '100755'].includes(file.mode) || !/^[a-f0-9]{40}$/.test(file.blob ?? '') || !/^[a-f0-9]{64}$/.test(file.sha256 ?? '')) fail('invalid source file record');
    const parts = file.path.split('/'); let directory = root;
    for (const part of parts.slice(0, -1)) {
      if (!directory.has(part)) directory.set(part, new Map());
      directory = directory.get(part);
      if (!(directory instanceof Map)) fail('conflicting source paths');
    }
    if (directory.has(parts.at(-1))) fail('duplicate source path');
    directory.set(parts.at(-1), file);
  }
  const tree = directory => {
    const entries = [...directory].map(([name, value]) => ({ name, value, sort: name + (value instanceof Map ? '/' : '') }))
      .sort((a, b) => Buffer.compare(Buffer.from(a.sort), Buffer.from(b.sort)));
    return gitObject('tree', Buffer.concat(entries.map(({ name, value }) => {
      const mode = value instanceof Map ? '40000' : value.mode;
      const id = value instanceof Map ? tree(value) : value.blob;
      return Buffer.concat([Buffer.from(`${mode} ${name}\0`), Buffer.from(id, 'hex')]);
    })));
  };
  return tree(root);
}

// The commit object and reconstructed Git tree bind the file catalogue to a
// commit independently of the capture caller, including in exports without .git.
export function verifyReceipt(receipt, expectedSha) {
  if (receipt?.version !== 1 || !Array.isArray(receipt.files) || !receipt.files.length || receipt.commit !== expectedSha ||
      !/^[a-f0-9]{40}$/.test(expectedSha ?? '')) fail('source receipt commit differs');
  const commit = Buffer.from(receipt.commitObject ?? '', 'base64');
  if (gitObject('commit', commit) !== expectedSha || !commit.toString('utf8').startsWith(`tree ${sourceTree(receipt.files)}\n`)) fail('source receipt Git object proof differs');
  return receipt;
}

export function verifySource(root, receipt, expectedSha, negativeCase = null) {
  verifyReceipt(receipt, expectedSha);
  const overrides = new Map();
  if (receipt.mutation) {
    const spec = negativeCases[negativeCase], mutation = receipt.mutation;
    if (!spec || mutation.name !== negativeCase || mutation.path !== spec[0]) fail('source mutation is not the selected negative case');
    const original = Buffer.from(mutation.original, 'base64'), item = receipt.files.find(file => file.path === spec[0]);
    if (!item || gitObject('blob', original) !== item.blob || hash(original) !== item.sha256) fail('negative manifest original differs from Git');
    overrides.set(spec[0], Buffer.from(mutateManifest(original.toString('utf8'), negativeCase)));
    const lock = Buffer.from(mutation.lockOriginal ?? '', 'base64'), pinned = receipt.files.find(file => file.path === 'Cargo.lock');
    if (!pinned || gitObject('blob', lock) !== pinned.blob || hash(lock) !== pinned.sha256) fail('negative lock original differs from Git');
    const bytes = readFileSync(path.join(root, 'Cargo.lock'));
    if (hash(bytes) !== mutation.lockSha256) fail('derived negative lock changed');
    verifyNegativeLock(lock, bytes);
    overrides.set('Cargo.lock', bytes);
  } else if (negativeCase) fail('negative capture needs its explicit source mutation');
  const expected = new Set(receipt.files.map(file => file.path));
  for (const item of receipt.files) {
    const file = path.join(root, item.path), info = lstatSync(file);
    if (!info.isFile() || info.isSymbolicLink() || ((info.mode & 0o100) !== 0) !== (item.mode === '100755')) fail(`source file mode differs: ${item.path}`);
    const bytes = readFileSync(file);
    if (overrides.has(item.path) ? !bytes.equals(overrides.get(item.path)) :
      hash(bytes) !== item.sha256 || gitObject('blob', bytes) !== item.blob) fail(`source content differs: ${item.path}`);
  }
  // Exact output locations used by Cargo, pnpm and the protocol TS build.
  // A directory with the same name inside Rust sources is still an input.
  const outputs = new Set(['target', 'desktop/src-tauri/target', 'node_modules', 'engine/protocol/ts/node_modules',
    'engine/protocol/ts/dist', 'torrential/src/proto']);
  const visit = (directory, prefix = '') => {
    for (const entry of readdirSync(directory, { withFileTypes: true })) {
      const relative = prefix + entry.name;
      if (relative === '.git' && (entry.isFile() || entry.isDirectory())) continue;
      // Tracked inputs within output directories remain mandatory above.
      if (outputs.has(relative) && entry.isDirectory()) continue;
      if (entry.isDirectory()) visit(path.join(directory, entry.name), relative + '/');
      else if (!entry.isFile() || !expected.has(relative)) fail(`unattested source input: ${relative}`);
    }
  };
  visit(root);
  return receipt;
}
