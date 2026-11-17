import path from 'node:path';
import { createRequire } from 'node:module';
import { readFileSync, readdirSync, realpathSync } from 'node:fs';
import { cli, stringOption } from './common.mjs';
import { admitReferenceImport, boundaryError as fail, digest, expectedPolicy, installedGraphs, within } from './reference-boundary.mjs';
import { importCounterexamples } from './reference-import-cases.mjs';

export function sourceFiles(ref) {
  const result = [];
  function visit(directory) {
    for (const entry of readdirSync(directory, { withFileTypes: true })) {
      if (entry.name === 'node_modules' && directory === ref) continue;
      const file = path.join(directory, entry.name);
      if (entry.isDirectory()) visit(file);
      else if (/\.(?:[cm]?js|[cm]?ts)$/.test(entry.name)) result.push(file);
    }
  }
  visit(ref);
  return result.sort();
}

// Analyze every own executable input, including tools and tests, and every
// resolved runtime and declaration edge. Computed module names fail closed.
export function scanSources(ts, context, entries) {
  const visited = new Set(), edges = [];
  const visit = file => {
    file = realpathSync(file);
    if (visited.has(file)) return;
    visited.add(file);
    const text = readFileSync(file, 'utf8'), own = within(context.ref, file);
    const tree = ts.createSourceFile(file, text, ts.ScriptTarget.Latest, true);
    if (tree.parseDiagnostics.length) fail(`unparseable module ${file}`);
    const modulePolicy = own ? expectedPolicy.runtime.modules[path.relative(context.ref, file).split(path.sep).join('/')] : null;
    const filesystemBindings = new Set();
    const openBindings = new Set(), flagBindings = new Map(), handles = new Set(), handleDeclarations = new Set(), openCalls = new Set(), handleAssignments = new Set();
    // Imports are hoisted. Collect capabilities before visiting any statements.
    for (const statement of tree.statements) {
      if (!own || !ts.isImportDeclaration(statement)) continue;
      const specifier = statement.moduleSpecifier.text, bindings = statement.importClause?.namedBindings;
      const allowed = specifier === 'node:fs/promises' ? modulePolicy?.filesystemImports :
        specifier === 'node:constants' ? modulePolicy?.constantImports : null;
      if (!allowed) continue; // The resolver rejects ungranted module imports.
      if (statement.importClause?.name || !bindings || !ts.isNamedImports(bindings) ||
          bindings.elements.some(item => !allowed.includes((item.propertyName ?? item.name).text))) fail(`unapproved filesystem capability in ${file}`);
      for (const item of bindings.elements) {
        const imported = (item.propertyName ?? item.name).text;
        if (specifier === 'node:constants') flagBindings.set(item.name.text, imported);
        else {
          filesystemBindings.add(item.name.text);
          if (imported === 'open') openBindings.add(item.name.text);
        }
      }
    }
    const flags = expression => {
      if (ts.isParenthesizedExpression(expression)) return flags(expression.expression);
      if (ts.isIdentifier(expression) && flagBindings.has(expression.text)) return [flagBindings.get(expression.text)];
      if (ts.isBinaryExpression(expression) && expression.operatorToken.kind === ts.SyntaxKind.BarToken) return [...flags(expression.left), ...flags(expression.right)];
      fail(`unanalyzable filesystem open flags in ${file}`);
    };
    let mutableHandle;
    const findMutableHandle = node => {
      if (modulePolicy?.mutableHandleName && ts.isVariableDeclaration(node) && ts.isIdentifier(node.name) &&
          node.name.text === modulePolicy.mutableHandleName && !node.initializer &&
          ts.isVariableDeclarationList(node.parent) && (node.parent.flags & ts.NodeFlags.Let)) {
        if (mutableHandle) fail(`duplicate mutable filesystem handle in ${file}`);
        mutableHandle = node; handles.add(node.name.text); handleDeclarations.add(node.name);
      }
      ts.forEachChild(node, findMutableHandle);
    };
    if (own) findMutableHandle(tree);
    const recordOpen = call => {
      if (call.arguments.length < 2 || call.arguments.length > 3) fail(`filesystem open requires an owned constant handle in ${file}`);
      const actual = [...new Set(flags(call.arguments[1]))].sort();
      if (JSON.stringify(actual) !== JSON.stringify([...(modulePolicy.openFlags ?? [])].sort())) fail(`unapproved filesystem open flags in ${file}`);
      openCalls.add(call);
    };
    const collectHandles = node => {
      if (own && ts.isVariableDeclaration(node) && node.initializer && ts.isAwaitExpression(node.initializer)) {
        const call = node.initializer.expression;
        if (ts.isCallExpression(call) && ts.isIdentifier(call.expression) && openBindings.has(call.expression.text)) {
          if (!ts.isIdentifier(node.name) || !ts.isVariableDeclarationList(node.parent) || !(node.parent.flags & ts.NodeFlags.Const) ||
              call.arguments.length < 2 || call.arguments.length > 3) fail(`filesystem open requires an owned constant handle in ${file}`);
          recordOpen(call);
          handles.add(node.name.text); handleDeclarations.add(node.name);
        }
      }
      if (own && mutableHandle && ts.isBinaryExpression(node) && node.operatorToken.kind === ts.SyntaxKind.EqualsToken &&
          ts.isIdentifier(node.left) && node.left.text === modulePolicy.mutableHandleName) {
        if (ts.isIdentifier(node.right) && node.right.text === 'undefined') handleAssignments.add(node.left);
        else if (ts.isAwaitExpression(node.right) && ts.isCallExpression(node.right.expression) &&
                 ts.isIdentifier(node.right.expression.expression) && openBindings.has(node.right.expression.expression.text)) {
          recordOpen(node.right.expression); handleAssignments.add(node.left);
        } else fail(`unapproved filesystem handle assignment in ${file}`);
      }
      ts.forEachChild(node, collectHandles);
    };
    collectHandles(tree);
    const edge = (specifier, type = false, requireMode = false, dynamic = false) => {
      type = type || tree.isDeclarationFile;
      if (!ts.isStringLiteral(specifier) && !ts.isNoSubstitutionTemplateLiteral(specifier)) fail(`unanalyzable import in ${file}`);
      if (own && ['node:crypto', 'node:test'].includes(specifier.text) && !ts.isImportDeclaration(specifier.parent)) fail(`capability imports must use named static bindings in ${file}`);
      for (const declaration of new Set([type, true])) {
        const target = admitReferenceImport(specifier.text, file, context, declaration, requireMode ? 'require' : dynamic ? 'dynamic' : 'static');
        if (own && target === 'node:fs/promises' && !ts.isImportDeclaration(specifier.parent)) fail(`filesystem capability must use named static imports in ${file}`);
        edges.push({ from: file, specifier: specifier.text, type: declaration, to: target });
        if (!target.startsWith('node:')) visit(target);
      }
    };
    for (const item of tree.referencedFiles) edge(ts.factory.createStringLiteral(item.fileName.startsWith('.') ? item.fileName : './' + item.fileName), true);
    for (const item of tree.typeReferenceDirectives) {
      if (item.fileName !== 'node') fail(`unapproved type reference ${item.fileName} in ${file}`);
      edges.push({ from: file, type: true, to: '@types/node (separate type-tools graph)' });
    }
    if (tree.libReferenceDirectives.length || tree.hasNoDefaultLib) fail(`unapproved compiler directive in ${file}`);
    const walkNode = node => {
      if (own && ts.isIdentifier(node) && flagBindings.has(node.text) &&
          ((ts.isVariableDeclaration(node.parent) || ts.isParameter(node.parent) || ts.isBindingElement(node.parent) ||
            ts.isFunctionDeclaration(node.parent) || ts.isClassDeclaration(node.parent)) && node.parent.name === node)) fail(`filesystem flag binding is shadowed in ${file}`);
      if (own && mutableHandle && ts.isIdentifier(node) && node.text === 'undefined' &&
          (ts.isVariableDeclaration(node.parent) || ts.isParameter(node.parent) || ts.isBindingElement(node.parent) ||
           ts.isFunctionDeclaration(node.parent) || ts.isClassDeclaration(node.parent)) && node.parent.name === node) fail(`filesystem reset binding is shadowed in ${file}`);
      if (own && ts.isCallExpression(node) && ts.isIdentifier(node.expression) && openBindings.has(node.expression.text) && !openCalls.has(node)) fail(`filesystem open requires an owned constant handle in ${file}`);
      if (own && ts.isIdentifier(node) && handles.has(node.text) && !handleDeclarations.has(node) && !handleAssignments.has(node)) {
        const access = node.parent;
        const guard = mutableHandle && node.text === modulePolicy.mutableHandleName &&
          (ts.isIfStatement(access) && access.expression === node ||
           ts.isPrefixUnaryExpression(access) && access.operator === ts.SyntaxKind.ExclamationToken &&
           ts.isIfStatement(access.parent) && access.parent.expression === access);
        if (!guard && (!ts.isPropertyAccessExpression(access) || access.expression !== node ||
            !(modulePolicy.handleMethods ?? []).includes(access.name.text) ||
            !ts.isCallExpression(access.parent) || access.parent.expression !== access)) fail(`filesystem handle escapes or uses an unapproved operation in ${file}`);
      }
      if (own && ts.isIdentifier(node) && filesystemBindings.has(node.text) && !ts.isImportSpecifier(node.parent) &&
          !(ts.isCallExpression(node.parent) && node.parent.expression === node)) fail(`filesystem capability escapes its module in ${file}`);
      if (own && ts.isImportDeclaration(node) && node.moduleSpecifier.text === 'node:crypto') {
        const bindings = node.importClause?.namedBindings;
        const allowed = modulePolicy?.cryptoImports ?? ['createHash', 'createHmac', 'hkdfSync', 'createPrivateKey', 'createPublicKey', 'sign', 'verify', 'timingSafeEqual', 'getHashes'];
        if (node.importClause?.name || !bindings || !ts.isNamedImports(bindings) ||
            bindings.elements.some(item => !node.importClause.isTypeOnly && !item.isTypeOnly && !allowed.includes((item.propertyName ?? item.name).text))) fail(`unapproved crypto capability in ${file}`);
      }
      if (own && ts.isImportDeclaration(node) && node.moduleSpecifier.text === 'node:test') {
        const bindings = node.importClause?.namedBindings;
        const allowed = ['test', 'describe', 'it', 'before', 'after', 'beforeEach', 'afterEach'];
        if (bindings && (!ts.isNamedImports(bindings) || bindings.elements.some(item => !node.importClause.isTypeOnly && !item.isTypeOnly && !allowed.includes((item.propertyName ?? item.name).text)))) fail(`unapproved test loader capability in ${file}`);
      }
      if (ts.isImportDeclaration(node) || ts.isExportDeclaration(node) && node.moduleSpecifier) edge(node.moduleSpecifier, !!node.isTypeOnly || !!node.importClause?.isTypeOnly);
      else if (ts.isImportEqualsDeclaration(node)) {
        if (own) fail(`forbidden require in ${file}`);
        if (!ts.isExternalModuleReference(node.moduleReference) || !node.moduleReference.expression) fail(`unanalyzable import-equals in ${file}`);
        edge(node.moduleReference.expression, !!node.isTypeOnly, true);
      } else if (ts.isImportTypeNode(node)) {
        if (!ts.isLiteralTypeNode(node.argument)) fail(`unanalyzable type import in ${file}`);
        edge(node.argument.literal, true);
      } else if (ts.isCallExpression(node) && node.expression.kind === ts.SyntaxKind.ImportKeyword) {
        if (own && (node.arguments.length !== 1 || !ts.isStringLiteralLike(node.arguments[0]))) fail(`forbidden dynamic import: unanalyzable target in ${file}`);
        if (node.arguments.length !== 1) fail(`unanalyzable dynamic import in ${file}`);
        edge(node.arguments[0], false, false, true);
      } else if (ts.isIdentifier(node) && node.text === 'require') {
        if (!ts.isCallExpression(node.parent) || node.parent.expression !== node || node.parent.arguments.length !== 1) fail(`${own ? 'forbidden' : 'escaped'} require in ${file}`);
        edge(node.parent.arguments[0], false, true);
      }
      if (own) {
        if (ts.isIdentifier(node) && ['eval', 'Function', 'AsyncFunction', 'WebAssembly', 'globalThis', 'global', 'Reflect', 'Proxy', 'fetch', 'Deno', 'Bun', 'XMLHttpRequest'].includes(node.text)) fail(`forbidden executable capability ${node.text} in ${file}`);
        if (ts.isIdentifier(node) && node.text === 'process' &&
            !(ts.isPropertyAccessExpression(node.parent) && node.parent.expression === node && ['argv', 'exit', 'exitCode', 'versions', 'version', 'platform'].includes(node.parent.name.text))) fail(`forbidden process capability in ${file}`);
        // A literal report key does not reference the CommonJS module binding.
        const moduleReportKey = ts.isIdentifier(node) && node.text === 'module' &&
          ts.isPropertyAssignment(node.parent) && node.parent.name === node;
        if (ts.isIdentifier(node) && !moduleReportKey && ['constructor', '__proto__', 'prototype', 'getBuiltinModule', 'getPrototypeOf', 'setPrototypeOf', 'getOwnPropertyDescriptor', 'getOwnPropertyDescriptors', 'module', 'setEngine', 'mock'].includes(node.text)) fail(`forbidden reflective capability in ${file}`);
        if (ts.isElementAccessExpression(node)) {
          const key = node.argumentExpression;
          if (ts.isStringLiteralLike(key) && ['constructor', '__proto__', 'prototype', 'require', 'getBuiltinModule', 'setEngine', 'mock', 'module'].includes(key.text)) fail(`forbidden computed capability in ${file}`);
        }
        if (ts.isPropertyAccessExpression(node) && ['resolve', 'register', 'registerHooks'].includes(node.name.text) && node.expression.kind === ts.SyntaxKind.MetaProperty) fail(`forbidden loader capability in ${file}`);
        if (ts.isMetaProperty(node) && node.keywordToken === ts.SyntaxKind.ImportKeyword &&
            !(ts.isPropertyAccessExpression(node.parent) && node.parent.name.text === 'url')) fail(`escaped import.meta in ${file}`);
      }
      ts.forEachChild(node, walk);
    };
    const walk = node => {
      try { walkNode(node); }
      catch (error) {
        if (!error.referenceLocation) {
          const line = tree.getLineAndCharacterOfPosition(node.getStart(tree)).line + 1;
          error.message = `${file}:${line}: ${error.message}`;
          error.referenceLocation = true;
        }
        throw error;
      }
    };
    walk(tree);
  };
  for (const file of entries) visit(file);
  return { files: [...visited].sort(), edges };
}

export async function inspectImports(root, selftest = true) {
  const context = { root, ...installedGraphs(root) };
  const compiler = context.typeTools.find(file => context.packages.get(file).name === 'typescript');
  const ts = createRequire(path.join(context.ref, 'package.json'))(path.join(compiler, 'lib/typescript.js'));
  const configFile = path.join(context.ref, 'tsconfig.json'), config = ts.readConfigFile(configFile, ts.sys.readFile);
  if (config.error || config.config.extends || config.config.references ||
      Object.keys(config.config.compilerOptions ?? {}).some(name => ['paths', 'baseUrl', 'rootDirs', 'plugins', 'typeRoots', 'customConditions', 'moduleSuffixes', 'noResolve'].includes(name))) fail('unanalyzable or redirected type configuration');
  const parsed = ts.parseJsonConfigFileContent(config.config, ts.sys, context.ref);
  if (parsed.errors.length || parsed.options.noEmit !== true || parsed.options.strict !== true ||
      parsed.options.module !== ts.ModuleKind.NodeNext || parsed.options.moduleResolution !== ts.ModuleResolutionKind.NodeNext ||
      JSON.stringify(parsed.options.types) !== '["node"]') fail('strict non-emitting NodeNext type configuration required');
  const program = ts.createProgram(parsed.fileNames, parsed.options), typeFiles = [];
  for (const file of program.getSourceFiles()) {
    const real = realpathSync(file.fileName);
    if (!within(context.ref, real) && ![...context.runtime, ...context.typeTools].some(dir => within(dir, real))) fail(`forbidden resolved compiler input ${real}`);
    typeFiles.push({ file: real, sha256: digest(readFileSync(real)) });
  }
  // Missing imports and declarations are failures even if they would be erased.
  const diagnostics = [...program.getOptionsDiagnostics(), ...program.getSyntacticDiagnostics(), ...program.getSemanticDiagnostics()];
  const issues = diagnostics.map(item => 'type graph is incomplete: ' + ts.flattenDiagnosticMessageText(item.messageText, '\n'));
  const graphs = {}, inventory = [];
  for (const [graph, entry] of Object.entries(expectedPolicy.runtime.entries)) {
    try { graphs[graph] = scanSources(ts, { ...context, graph }, [path.join(context.ref, entry)]); }
    catch (error) { graphs[graph] = { entry, diagnostic: error.message }; issues.push(error.message); }
  }
  for (const file of sourceFiles(context.ref)) {
    try { inventory.push({ entry: file, ...scanSources(ts, { ...context, graph: 'inventory' }, [file]) }); }
    catch (error) { inventory.push({ entry: file, diagnostic: error.message }); issues.push(error.message); }
  }
  const runtime = { graphs, inventory };
  const toolFiles = [];
  for (const directory of context.typeTools) {
    const visit = dir => {
      for (const item of readdirSync(dir, { withFileTypes: true })) {
        const file = path.join(dir, item.name);
        if (item.isSymbolicLink()) fail(`unapproved tool symlink ${file}`);
        if (item.isDirectory()) visit(file);
        else toolFiles.push({ file, sha256: digest(readFileSync(file)) });
      }
    };
    visit(directory);
  }
  return {
    policy: expectedPolicy, runtime: { ...runtime, packages: context.runtime, installedEdges: context.edges.filter(edge => Object.hasOwn(expectedPolicy.runtime.packages, edge.alias)) },
    issues: [...new Set(issues)],
    typeTools: { packages: context.typeTools, installedEdges: context.edges.filter(edge => Object.hasOwn(expectedPolicy.typeTools, edge.alias)), files: toolFiles, compilerInputs: typeFiles.sort((a, b) => a.file.localeCompare(b.file)) },
    counterexamples: selftest ? importCounterexamples(ts, root) : [],
  };
}
await cli(import.meta.url, 'Usage: node tools/ci/check-reference-imports.mjs --root DIR\nInspect resolved runtime and separate compiler/type graphs; reject unanalyzable edges.', { root: stringOption }, async options => {
  const result = await inspectImports(path.resolve(options.root ?? '.'));
  console.log(JSON.stringify(result, null, 2));
  return result.issues;
});
