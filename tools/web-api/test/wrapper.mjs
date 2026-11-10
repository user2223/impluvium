import { readFile } from 'node:fs/promises';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';
import { compiler } from '../compiler.mjs';
import { rootDirectory as root } from '../generate.mjs';

export async function loadWrapper(server) {
  await readFile(join(root, 'server/shared/api/generated/routes.ts'), 'utf8');
  const ts = compiler(root);
  const source = await readFile(join(root, 'server/composables/request.ts'), 'utf8');
  // Expose the unchanged lower transport only to tests of its full ofetch surface.
  let code = ts.transpileModule(source + '\nexport const $transport = dropTransport;\n', {
    compilerOptions: { target: ts.ScriptTarget.ESNext, module: ts.ModuleKind.ESNext },
  }).outputText;
  for (const [alias, file] of [
    ['~/shared/api/generated/routes', 'server/shared/api/generated/routes.ts'],
    ['~/shared/api/request', 'server/shared/api/request.ts'],
  ]) code = code.replaceAll(JSON.stringify(alias), JSON.stringify(pathToFileURL(join(root, file)).href));
  code = code.replaceAll('import.meta.server', String(server)).replaceAll('import.meta.client', String(!server));
  return import(`data:text/javascript;base64,${Buffer.from(code).toString('base64')}`);
}
