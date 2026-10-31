import { mkdirSync, realpathSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';
import { DOC_PATH, loadAndRender } from '../ci/identity-doc.mjs';

// Writes docs/dev/identity.md. Run it only in a disposable container: it executes repository code and writes a file.
const usage = `Usage: node tools/identity/write-doc.mjs [--root DIR] [--stdout]
Render ${DOC_PATH} from tools/identity.json and tools/env-register.json and write it under --root (default: the current directory).
--stdout prints the page instead of writing it.`;

if (process.argv[1] && realpathSync(process.argv[1]) === realpathSync(fileURLToPath(import.meta.url))) {
  try {
    const { values } = parseArgs({ options: { help: { type: 'boolean' }, root: { type: 'string' }, stdout: { type: 'boolean' } }, strict: true });
    if (values.help) console.log(usage);
    else {
      const root = path.resolve(values.root ?? '.');
      const text = loadAndRender(root);
      if (values.stdout) process.stdout.write(text);
      else {
        mkdirSync(path.join(root, path.posix.dirname(DOC_PATH)), { recursive: true });
        writeFileSync(path.join(root, DOC_PATH), text);
      }
    }
  } catch (error) {
    console.error(error.message);
    process.exitCode = 2;
  }
}
