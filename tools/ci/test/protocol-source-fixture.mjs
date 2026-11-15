import path from 'node:path';
import { lstatSync, readFileSync, readdirSync } from 'node:fs';
import { hash } from '../protocol-contract.mjs';
import { gitObject, sourceTree } from '../protocol-source.mjs';

// Synthetic data for lock validation tests; never used by Cargo.
export const productLock = 'version = 4\n\n[[package]]\nname = "example"\nversion = "1.0.0"\ndependencies = []\n\n[[package]]\nname = "helper"\nversion = "1.0.0"\n';

export function attest(root) {
  const files = [];
  const visit = (directory, prefix = '') => {
    for (const entry of readdirSync(directory, { withFileTypes: true }).sort((a, b) => a.name.localeCompare(b.name))) {
      const file = path.join(directory, entry.name), relative = prefix + entry.name;
      if (entry.isDirectory()) visit(file, relative + '/');
      else {
        const bytes = readFileSync(file);
        files.push({ path: relative, mode: lstatSync(file).mode & 0o100 ? '100755' : '100644', blob: gitObject('blob', bytes), sha256: hash(bytes) });
      }
    }
  };
  visit(root);
  const bytes = Buffer.from(`tree ${sourceTree(files)}\nauthor Test <test@example.invalid> 0 +0000\ncommitter Test <test@example.invalid> 0 +0000\n\nSynthetic source proof\n`);
  return { version: 1, commit: gitObject('commit', bytes), commitObject: bytes.toString('base64'), files };
}
