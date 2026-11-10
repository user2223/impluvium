import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { createRequire } from 'node:module';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createProgram, formatDiagnostics, materializer, problemCollector, walk } from './compiler.mjs';
import { binaryBodies, bodies, formBodies, queries, responses } from './contracts.mjs';
import { filesBelow, readInventory, redirectRules } from './inventory.mjs';

export const rootDirectory = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
export const generatedDirectory = 'server/shared/api/generated';
export const jsonTypes = `
export type JsonValue = string | number | boolean | null | JsonValue[] | JsonObject;
export type JsonObject = { [key: string]: JsonValue };
`;

function shortName(file) {
  return file.replace('server/server/api/v1/', '').replace('server/server/', '').replace(/\.ts$/, '');
}

function nullish(ts, type) {
  return type.isUnion() ? type.types.some((part) => nullish(ts, part))
    : !!(type.flags & (ts.TypeFlags.Void | ts.TypeFlags.Undefined | ts.TypeFlags.Null));
}

function onlyNullish(ts, type) {
  return type.isUnion() ? type.types.every((part) => onlyNullish(ts, part))
    : !!(type.flags & (ts.TypeFlags.Void | ts.TypeFlags.Undefined | ts.TypeFlags.Null));
}

function validatorType(ts, checker, source, name) {
  let declaration;
  walk(ts, source, (node) => {
    if (ts.isVariableDeclaration(node) && ts.isIdentifier(node.name) && node.name.text === name) declaration = node;
  });
  if (!declaration) throw new Error(`Missing validator ${name} in ${source.fileName}`);
  const validator = checker.getTypeAtLocation(declaration.name);
  // Input types preserve defaults as optional and describe strings before parsing.
  const property = validator.getProperty('inferIn');
  if (!property) throw new Error(`Validator has no inferIn property: ${name}`);
  return checker.getTypeOfSymbol(property);
}

export function callbackResponseType(ts, checker, source) {
  const assignment = source.statements.find(ts.isExportAssignment);
  const call = assignment?.expression;
  if (!call || !ts.isCallExpression(call)) throw new Error(`Unsupported handler export: ${source.fileName}`);
  const callback = call.arguments[0];
  if (!callback || (!ts.isArrowFunction(callback) && !ts.isFunctionExpression(callback))) {
    throw new Error(`Unsupported handler callback: ${source.fileName}`);
  }
  const signature = checker.getSignatureFromDeclaration(callback);
  if (!signature) throw new Error(`Missing handler signature: ${source.fileName}`);
  // Inspect the callback itself: a request-only generic on an H3 wrapper can
  // otherwise erase the inferred response through its default type parameter.
  const type = checker.getAwaitedType(checker.getReturnTypeOfSignature(signature));
  if (!type) throw new Error(`Unresolved awaited response: ${source.fileName}`);
  return type;
}

export async function generate(root = rootDirectory) {
  const inventory = await readInventory(root);
  const probePath = join(root, 'server/web-api-type-probe.ts');
  const probe = [jsonTypes, 'type TopResponse<T> = T extends null | void ? undefined : T;'];
  for (const [index, route] of inventory.entries()) {
    const name = shortName(route.file);
    const module = `./${route.file.slice('server/'.length).replace(/\.ts$/, '')}`;
    const raw = ['binary', 'binary-or-empty', 'websocket', 'redirect'].includes(route.kind)
      ? 'never' : `Awaited<ReturnType<typeof import(${JSON.stringify(module)}).default>>`;
    probe.push(`export type RawResponse${index} = ${raw};`);
    probe.push(`export type Response${index} = TopResponse<${responses[name] ?? `RawResponse${index}`}>;`);
    for (const [field, specifications] of [['Body', bodies], ['Query', queries]]) {
      const expression = specifications[name]?.expression;
      if (expression) probe.push(`export type ${field}${index} = ${expression.replaceAll('RawResponse', `RawResponse${index}`)};`);
    }
  }
  const { ts, program, checker } = createProgram(root, new Map([[probePath, probe.join('\n')]]));
  const source = program.getSourceFile(probePath);
  const problems = problemCollector();
  const diagnostics = program.getSyntacticDiagnostics(source).concat(program.getSemanticDiagnostics(source));
  if (diagnostics.length) problems.add(new Error(formatDiagnostics(ts, diagnostics)));
  const aliases = new Map(source.statements.filter(ts.isTypeAliasDeclaration).map((node) => [node.name.text, checker.getTypeAtLocation(node)]));
  const dto = materializer(ts, checker);
  const qualify = (value) => value.replace(/\b(Dto\d+|JsonValue|JsonObject)\b/g, 'Dtos.$1');
  const entries = [];
  const metadata = [];
  for (const [index, route] of inventory.entries()) {
    const name = shortName(route.file);
    const handler = problems.capture(route.file, () => {
      const loaded = program.getSourceFile(join(root, route.file));
      if (!loaded) throw new Error('Handler was not loaded');
      return loaded;
    });
    if (!handler) continue;
    const calls = [];
    walk(ts, handler, (node) => {
      if (ts.isCallExpression(node) && ts.isIdentifier(node.expression)) calls.push(node);
    });
    function input(field, specification, fallback) {
      if (specification?.expression) return qualify(dto.render(aliases.get(`${field}${index}`), `${route.file} ${field}`, true));
      if (specification?.validator) return qualify(dto.render(validatorType(ts, checker, handler, specification.validator), `${route.file} ${field}`, true));
      if (fallback) return qualify(dto.render(validatorType(ts, checker, handler, fallback), `${route.file} ${field}`, true));
      return 'never';
    }
    const validatedBody = calls.find((node) => ['readDropValidatedBody', 'readValidatedBody'].includes(node.expression.text));
    const bodyValidator = validatedBody?.arguments[1];
    const hasRawBody = calls.some((node) => ['readBody', 'readRawBody', 'readFormData', 'readMultipartFormData', 'handleFileUpload'].includes(node.expression.text));
    const body = problems.capture(`${route.file} Body`, () => {
      if (bodyValidator && !ts.isIdentifier(bodyValidator)) throw new Error('Unsupported body validator');
      const value = formBodies.has(name) ? name === 'auth/oidc/logout.post' ? 'FormData | URLSearchParams' : 'FormData'
        : binaryBodies.has(name) ? 'Blob | ArrayBuffer | Uint8Array<ArrayBuffer>'
        : input('Body', bodies[name], bodyValidator?.text);
      if (hasRawBody && value === 'never') throw new Error('Unmapped request body');
      return value;
    });
    const hasQuery = calls.some((node) => node.expression.text === 'getQuery');
    const query = problems.capture(`${route.file} Query`, () => input('Query', queries[name], hasQuery && !queries[name] ? 'Query' : undefined));
    const parameters = [...route.path.matchAll(/:([A-Za-z0-9_]+)/g)].map((match) => `${JSON.stringify(match[1])}: string | number;`);
    const path = parameters.length ? `{ ${parameters.join(' ')} }` : 'never';
    const result = problems.capture(`${route.file} response`, () => {
      let kind = route.kind;
      let response;
      const responseType = ['binary', 'binary-or-empty', 'websocket', 'redirect'].includes(kind)
        ? undefined : responses[name] ? aliases.get(`Response${index}`) : callbackResponseType(ts, checker, handler);
      if (kind === 'empty') {
        if (!onlyNullish(ts, responseType)) throw new Error('Nonempty response marked empty');
        response = 'undefined';
      } else if (kind === 'binary' || kind === 'binary-or-empty') {
        response = kind === 'binary' ? 'Blob' : 'Blob | undefined';
      } else if (kind === 'websocket' || kind === 'redirect') {
        response = 'never';
      } else {
        if (onlyNullish(ts, responseType)) throw new Error('Empty response marked JSON');
        const parts = responseType.isUnion() ? responseType.types : [responseType];
        const rendered = parts.map((part) => problems.capture(`${route.file} response`, () => onlyNullish(ts, part) ? 'undefined'
          : qualify(dto.render(part, `${route.file} response`))));
        if (rendered.includes(undefined)) return;
        response = [...new Set(rendered)].sort().join(' | ');
        if (nullish(ts, responseType)) kind = kind === 'text' ? 'text-or-empty' : 'json-or-empty';
      }
      return { kind, response };
    });
    if (body === undefined || query === undefined || result === undefined) continue;
    const { kind, response } = result;
    metadata.push({ ...route, kind });
    const methods = route.method === '*' ? ['GET', 'HEAD', 'POST', 'PUT', 'PATCH', 'DELETE', 'OPTIONS', 'CONNECT', 'TRACE'] : [route.method];
    for (const method of methods) {
      const key = `${method} ${route.path}`;
      entries.push({ key, method, template: route.path, kind: method === 'HEAD' ? 'empty' : kind, path, query, body, response: method === 'HEAD' ? 'undefined' : response });
    }
  }
  problems.throwIfAny('HTTP contract generation failed');
  entries.sort((a, b) => a.key.localeCompare(b.key, 'en'));
  const header = '// Generated by tools/web-api/generate.mjs. Do not edit.\n// Transitional contracts for the existing HTTP handlers.\n\n';
  const routeTypes = entries.map((entry) => `${JSON.stringify(entry.key)}: {
    path: ${entry.path}; query: ${entry.query}; body: ${entry.body}; response: ${entry.response}; kind: ${JSON.stringify(entry.kind)};
  };`).join('\n');
  const routeValues = entries.map((entry) => `${JSON.stringify(entry.key)}: ${JSON.stringify({ method: entry.method, path: entry.template, kind: entry.kind })},`).join('\n');
  const require = createRequire(join(root, 'server/package.json'));
  const prettier = require('prettier');
  const format = (code) => prettier.format(code, { parser: 'typescript', endOfLine: 'lf' });
  return new Map([
    ['dtos.ts', await format(header + jsonTypes + dto.declarations())],
    ['routes.ts', await format(`${header}import type * as Dtos from "./dtos";\nexport interface ApiRouteMap {\n${routeTypes}\n}\nexport const apiRoutes = {\n${routeValues}\n} as const;\n`)],
    ['inventory.json', await prettier.format(JSON.stringify({
      handlers: metadata.filter((route) => route.file.startsWith('server/server/api/')),
      serverRoutes: metadata.filter((route) => route.file.startsWith('server/server/routes/')),
      redirectRules,
    }), {
      parser: 'json',
      endOfLine: 'lf',
      plugins: [require.resolve('prettier-plugin-sort-json')],
      jsonRecursiveSort: true,
      jsonSortOrder: '{"/.*/": "lexical"}',
    })],
  ]);
}

export async function compareOutputs(directory, expected) {
  const actual = (await filesBelow(directory)).map((file) => file.slice(directory.length + 1));
  const wanted = [...expected.keys()].sort();
  if (JSON.stringify(actual.sort()) !== JSON.stringify(wanted)) throw new Error(`Generated file set differs in ${directory}`);
  for (const [name, contents] of expected) {
    if (await readFile(join(directory, name), 'utf8') !== contents) throw new Error(`Generated file drift: ${name}`);
  }
}

async function main() {
  const args = process.argv.slice(2);
  if (args.length === 1 && args[0] === '--help') {
    console.log('Usage: node tools/web-api/generate.mjs [--check] [--out DIRECTORY]');
    console.log('Run after the frozen server dependency installation and Nuxt/Prisma preparation.');
    console.log('Only static TypeScript inspection is used; handlers are never imported at runtime.');
    console.log('Generation is offline and writes dtos.ts, routes.ts and inventory.json.');
    console.log('Commit those three files under server/shared/api/generated/.');
    return;
  }
  const check = args.includes('--check');
  const outIndex = args.indexOf('--out');
  const directory = outIndex < 0 ? join(rootDirectory, generatedDirectory) : resolve(args[outIndex + 1]);
  const allowed = outIndex < 0 ? ['--check'] : ['--check', '--out', args[outIndex + 1]];
  if (args.some((arg) => !allowed.includes(arg))) throw new Error('Usage: generate.mjs [--check] [--out DIRECTORY]');
  const outputs = await generate();
  if (check) {
    await compareOutputs(directory, outputs);
  } else {
    await mkdir(directory, { recursive: true });
    const extras = (await filesBelow(directory)).map((file) => file.slice(directory.length + 1)).filter((file) => !outputs.has(file));
    if (extras.length) throw new Error(`Unexpected generated files: ${extras.join(', ')}`);
    for (const [name, contents] of outputs) await writeFile(join(directory, name), contents);
  }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main().catch((error) => { console.error(error.message); process.exitCode = 1; });
}
