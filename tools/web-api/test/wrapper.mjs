import { readFile } from 'node:fs/promises';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';
import { compiler } from '../compiler.mjs';
import { rootDirectory as root } from '../generate.mjs';

export async function loadWrapper() {
  await readFile(join(root, 'server/shared/api/generated/routes.ts'), 'utf8');
  const ts = compiler(root);
  const source = await readFile(join(root, 'server/composables/request.ts'), 'utf8');
  // Load the production browser entry point with its normal typed API surface.
  let code = ts.transpileModule(source, {
    compilerOptions: { target: ts.ScriptTarget.ESNext, module: ts.ModuleKind.ESNext },
  }).outputText;
  for (const [alias, file] of [
    ['~/shared/api/generated/routes', 'server/shared/api/generated/routes.ts'],
    ['~/shared/api/request', 'server/shared/api/request.ts'],
    ['./browser-transport', 'server/composables/browser-transport.ts'],
    ['./session-context', 'server/composables/session-context.ts'],
  ]) code = code.replaceAll(JSON.stringify(alias), JSON.stringify(pathToFileURL(join(root, file)).href));
  return import(`data:text/javascript;base64,${Buffer.from(code).toString('base64')}`);
}
