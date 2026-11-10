import { readFile } from 'node:fs/promises';
import { join } from 'node:path';
import { compiler, createProgram, formatDiagnostics } from './compiler.mjs';
import { rootDirectory as root, generatedDirectory } from './generate.mjs';
import { comparisonResponse, comparisonTypes, responseComparator, responseMismatch } from './nitro-comparison.mjs';
import { comparisonPolicy, nonComparableFile } from './non-comparable.mjs';

function assertClean(ts, program, diagnosticContext) {
  const diagnostics = ts.getPreEmitDiagnostics(program);
  if (diagnostics.length) {
    const message = diagnosticContext ? diagnostics.map((diagnostic) => {
      const context = diagnosticContext(diagnostic);
      return `${context ? `${context}\n` : ''}${formatDiagnostics(ts, [diagnostic])}`;
    }).join('\n') : formatDiagnostics(ts, diagnostics);
    throw new Error(message);
  }
}

async function checkIndependentClient() {
  const ts = compiler(root);
  const contextPath = join(root, 'server/web-api-client-context.d.ts');
  const componentsPath = join(root, 'server/web-api-client-components.ts');
  const virtual = new Map([
    [contextPath, `
      import type { ApiFetch } from "./shared/api/client";
      declare global {
        const $apiFetch: ApiFetch;
        interface ImportMeta { readonly client: boolean; readonly server: boolean }
        function updateUser(): Promise<void>;
        function useUser(): { value: { admin: boolean } | null };
        function isClientRequest(): boolean;
        function ref<T>(value: T): { value: T };
        function computed<T>(getter: () => T): { readonly value: T };
        function onMounted(callback: () => void): void;
        function useI18n(): { t: (key: string, values?: unknown[]) => string };
        function useRoute(): { query: Record<string, string | string[] | undefined> };
        function useHead(head: { title?: string; titleTemplate?: (title?: string) => string }): void;
        function useObject(id: string): string;
        function useFavicon(url: string, options: { rel: string }): void;
        function definePageMeta(meta: { layout: false }): void;
      }
    `],
    [componentsPath, 'export declare const IconsDiscordLogo: unknown; export declare const IconsGithubLogo: unknown;'],
  ]);
  // Typecheck the actual consumer scripts with small UI context declarations,
  // instead of Nuxt's generated globals, which also expose the old fetch interface.
  for (const file of [
    'app.vue', 'components/ApplicationLogo.vue', 'components/DropWordmark.vue',
    'components/UserFooter.vue', 'layouts/default.vue', 'pages/auth/signin.vue',
  ]) {
    const contents = await readFile(join(root, 'server', file), 'utf8');
    const script = contents.match(/<script setup lang="ts">([\s\S]*?)<\/script>/)?.[1];
    if (!script) throw new Error(`Missing TypeScript setup script: ${file}`);
    virtual.set(join(root, 'server', file.replace(/\.vue$/, '.api-contract.ts')), `export {};\n${script}`);
  }
  const options = {
    noEmit: true,
    strict: true,
    target: ts.ScriptTarget.ESNext,
    module: ts.ModuleKind.ESNext,
    moduleResolution: ts.ModuleResolutionKind.Bundler,
    lib: ['lib.esnext.d.ts', 'lib.dom.d.ts', 'lib.dom.iterable.d.ts'],
    types: [],
    skipLibCheck: true,
    paths: { '#components': [componentsPath] },
  };
  const host = ts.createCompilerHost(options);
  const read = host.readFile.bind(host);
  const exists = host.fileExists.bind(host);
  host.readFile = (file) => virtual.get(file) ?? read(file);
  host.fileExists = (file) => virtual.has(file) || exists(file);
  const program = ts.createProgram({
    rootNames: [join(root, 'tools/ci/fixtures/web-api-types.ts'), ...virtual.keys()],
    options,
    host,
  });
  for (const source of program.getSourceFiles()) {
    if (/\/server\/server\/|\/\.nuxt\/|\/nitropack\/|\/prisma\//.test(source.fileName)) {
      throw new Error(`Client contracts load a backend type: ${source.fileName}`);
    }
  }
  assertClean(ts, program);
  // Recheck the same calls without suppression directives. This proves that
  // each negative case is rejected by the generated contracts themselves.
  const fixture = join(root, 'tools/ci/fixtures/web-api-types.ts');
  const contents = await readFile(fixture, 'utf8');
  const lines = contents.split('\n');
  const expected = lines.flatMap((line, index) => line.includes('// @ts-expect-error') ? [index + 1] : []);
  if (expected.length < 13) throw new Error('Negative API type cases are missing.');
  virtual.set(fixture, contents.replaceAll('@ts-expect-error', 'negative-type-case'));
  const unsuppressed = ts.createProgram({ rootNames: program.getRootFileNames(), options, host });
  const errors = ts.getPreEmitDiagnostics(unsuppressed);
  const observed = new Set();
  for (const diagnostic of errors) {
    if (diagnostic.file?.fileName !== fixture || diagnostic.start === undefined) {
      throw new Error(formatDiagnostics(ts, [diagnostic]));
    }
    const line = diagnostic.file.getLineAndCharacterOfPosition(diagnostic.start).line;
    if (!expected.includes(line)) throw new Error(formatDiagnostics(ts, [diagnostic]));
    observed.add(line);
  }
  for (const line of expected) {
    if (!observed.has(line)) throw new Error(`Negative API type case was accepted at line ${line + 1}.`);
  }
}

async function checkNitro() {
  const inventory = JSON.parse(await readFile(join(root, generatedDirectory, 'inventory.json'), 'utf8'));
  const policy = comparisonPolicy(JSON.parse(await readFile(join(root, nonComparableFile), 'utf8')));
  let source = [
    'import type { InternalApi } from "nitropack/types";',
    'import type { ApiRouteMap } from "./shared/api/generated/routes";',
    comparisonTypes,
  ].join('\n') + '\n';
  let line = source.split('\n').length - 1;
  const routeLines = new Map();
  const comparisons = [];
  function append(statement, key) {
    const count = statement.split('\n').length;
    for (let offset = 0; offset < count; offset++) routeLines.set(line + offset, key);
    source += `${statement}\n`;
    line += count;
  }
  for (const [index, route] of [...inventory.handlers, ...inventory.serverRoutes].entries()) {
    const method = route.method === '*' ? 'default' : route.method.toLowerCase();
    const key = `${route.method === '*' ? 'GET' : route.method} ${route.path}`;
    append(`export type Observed${index} = InternalApi[${JSON.stringify(route.path)}][${JSON.stringify(method)}];`, key);
    if (['json', 'json-or-empty', 'empty', 'text', 'text-or-empty'].includes(route.kind)) {
      const response = comparisonResponse(route, index);
      for (const declaration of response.declarations) append(declaration, key);
      append(`type Contract${index} = ApiRouteMap[${JSON.stringify(key)}]["response"];`, key);
      comparisons.push({ index, key, kind: route.kind });
    }
  }
  const probe = join(root, 'server/web-api-nitro-comparison.ts');
  const { ts, program, checker } = createProgram(root, new Map([[probe, source]]));
  assertClean(ts, program, (diagnostic) => {
    if (diagnostic.file?.fileName !== probe || diagnostic.start === undefined) return;
    const line = diagnostic.file.getLineAndCharacterOfPosition(diagnostic.start).line;
    const key = routeLines.get(line);
    return key && `HTTP response contract: ${key}`;
  });
  const aliases = new Map(program.getSourceFile(probe).statements.filter(ts.isTypeAliasDeclaration)
    .map((node) => [node.name.text, checker.getTypeAtLocation(node)]));
  const compare = responseComparator(ts, checker, aliases);
  let excluded = 0;
  const problems = comparisons.flatMap(({ index, key, kind }) => {
    const contract = aliases.get(`Contract${index}`);
    const observed = aliases.get(`Observed${index}`);
    const decision = policy.check(key, !!(observed.flags & ts.TypeFlags.Any));
    // Even a documented Nitro any cannot excuse an unresolved transition
    // contract. Inspect that complete contract independently before skipping
    // only the comparison that Nitro's erased type makes impossible.
    const differences = decision.skip ? compare(contract, contract)
      : compare(contract, observed, { raw: aliases.get(`Raw${index}`), kind });
    differences.unshift(...decision.problems);
    if (decision.skip) excluded++;
    return differences.length ? [responseMismatch(key, differences)] : [];
  });
  problems.push(...policy.finish());
  if (problems.length) throw new Error(problems.join('\n\n'));
  return excluded;
}

try {
  // Do not turn a missing map into misleading unused-directive diagnostics.
  for (const file of ['dtos.ts', 'routes.ts', 'inventory.json']) {
    await readFile(join(root, generatedDirectory, file), 'utf8');
  }
  await checkIndependentClient();
  const excluded = await checkNitro();
  console.log(`Independent client contracts, negative type cases and Nitro responses checked; ${excluded} documented Nitro any responses are not comparable (${nonComparableFile}).`);
} catch (error) {
  console.error(error.message);
  process.exitCode = 1;
}
