import path from 'node:path';
import { existsSync, readdirSync, readlinkSync, realpathSync } from 'node:fs';
import { equal, fail } from './protocol-contract.mjs';
import { fileDigest } from './protocol-io.mjs';

// Include the alias wrapper, inner compiler, linker and loadable tool libraries.
// Hash symlink targets as separate files and retain the exact alias structure.
export function windowsFiles(root) {
  root = realpathSync(root);
  const files = [];
  const visit = relative => {
    for (const entry of readdirSync(path.join(root, relative), { withFileTypes: true }).sort((a, b) => a.name.localeCompare(b.name))) {
      const name = path.posix.join(relative, entry.name), file = path.join(root, name);
      if (entry.isSymbolicLink()) {
        if (!realpathSync(file).startsWith(root + path.sep)) fail('Windows installation link escapes its root');
        files.push({ path: name, link: readlinkSync(file) });
      } else if (entry.isDirectory()) visit(name);
      else if (entry.isFile()) files.push({ path: name, sha256: fileDigest(file).sha256 });
      else fail('unsupported Windows installation entry');
    }
  };
  for (const directory of ['bin', 'lib', 'libexec']) if (existsSync(path.join(root, directory))) visit(directory);
  for (const name of ['bin/clang-target-wrapper.sh', 'bin/clang', 'bin/clang-23', 'bin/ld.lld', 'bin/llvm-ar']) {
    if (!existsSync(path.join(root, name))) fail(`Windows installation lacks ${name}`);
    const resolved = path.relative(root, realpathSync(path.join(root, name))).split(path.sep).join('/');
    if (!files.some(file => file.path === resolved && file.sha256)) fail('Windows inner executable is not covered');
  }
  return files;
}
export function verifyWindows(receipt, pins) {
  if (receipt?.version !== 2 || !equal(receipt.pins, pins) || !equal(receipt.files, windowsFiles(receipt.root))) fail('Windows inner tool or installation differs from verified archive');
}
export function verifyWindowsVersion(version, pins) {
  const expected = pins.LLVM_MINGW_CLANG_VERSION.replaceAll('.', '\\.');
  if (typeof version !== 'string' || !new RegExp(`\\bversion ${expected}(?:\\s|$)`).test(version)) fail('Windows tool version differs from shared pin');
}
