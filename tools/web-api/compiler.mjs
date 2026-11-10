import { createRequire } from 'node:module';
import { join } from 'node:path';

export function compiler(root) {
  const require = createRequire(join(root, 'server/package.json'));
  return require('typescript');
}

export function createProgram(root, virtualFiles = new Map(), extraRoots = []) {
  const ts = compiler(root);
  const configPath = join(root, 'server/server/tsconfig.json');
  const config = ts.readConfigFile(configPath, ts.sys.readFile);
  if (config.error) throw new Error(ts.flattenDiagnosticMessageText(config.error.messageText, '\n'));
  const parsed = ts.parseJsonConfigFileContent(config.config, ts.sys, join(root, 'server/server'));
  if (parsed.errors.length) throw new Error(formatDiagnostics(ts, parsed.errors));
  // The project type-checks with exactOptionalPropertyTypes off (server/tsconfig.json). With it on,
  // Prisma drops conditional includes from its result types and responses resolve to the wrong shape.
  const options = { ...parsed.options, exactOptionalPropertyTypes: false, noEmit: true, incremental: false, composite: false };
  const host = ts.createCompilerHost(options);
  const readFile = host.readFile.bind(host);
  const fileExists = host.fileExists.bind(host);
  host.readFile = (file) => virtualFiles.get(file) ?? readFile(file);
  host.fileExists = (file) => virtualFiles.has(file) || fileExists(file);
  const program = ts.createProgram({
    rootNames: [...parsed.fileNames, ...virtualFiles.keys(), ...extraRoots],
    options,
    host,
  });
  return { ts, program, checker: program.getTypeChecker() };
}

export function formatDiagnostics(ts, diagnostics) {
  return ts.formatDiagnostics(diagnostics, {
    getCanonicalFileName: (file) => file,
    getCurrentDirectory: () => '.',
    getNewLine: () => '\n',
  });
}

export function walk(ts, node, visit) {
  visit(node);
  ts.forEachChild(node, (child) => walk(ts, child, visit));
}

export function problemCollector() {
  const problems = [];
  function add(error, context) {
    if (error instanceof AggregateError) {
      for (const nested of error.errors) add(nested, context);
      return;
    }
    const message = error instanceof Error ? error.message : String(error);
    problems.push(new Error(context && !message.includes(context) ? `${context}: ${message}` : message));
  }
  return {
    add,
    capture(context, operation) {
      try { return operation(); } catch (error) { add(error, context); }
    },
    throwIfAny(message) {
      if (problems.length) {
        throw new AggregateError(problems, `${message} (${problems.length}):\n${problems.map((error) => `- ${error.message}`).join('\n')}`);
      }
    },
  };
}

/** Materialize types, never printing a reference to a handler, Prisma or Nitro. */
export function materializer(ts, checker) {
  const definitions = new Map();
  const names = new Map();
  const inputNames = new Map();
  const problems = [];
  const undefinedType = (type) => !!(type.flags & (ts.TypeFlags.Undefined | ts.TypeFlags.Void));
  const hasUndefined = (type) => type.isUnion() ? type.types.some(undefinedType) : undefinedType(type);
  function withoutUndefined(type, context, input) {
    if (!type.isUnion()) return undefinedType(type) ? 'never' : visit(type, context, input);
    return [...new Set(type.types.filter((part) => !undefinedType(part)).map((part) => visit(part, context, input)))].sort().join(' | ') || 'never';
  }
  function arrayElement(type, context, input) {
    if (input) return visit(type, context, true);
    if (type.isUnion()) return [...new Set(type.types.map((part) => arrayElement(part, context, false)))].join(' | ');
    if (undefinedType(type) || type.getCallSignatures().length) return 'null';
    return visit(type, context, false);
  }
  function visit(type, context, input = false) {
    try { return renderType(type, context, input); } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      problems.push(new Error(message.includes(context) ? message : `${context}: ${message}`));
      // This marker is never emitted: render and declarations reject the problems.
      return 'never';
    }
  }
  function rejectProblems(errors) {
    if (errors.length) throw new AggregateError(errors, errors.map((error) => error.message).join('\n'));
  }
  function renderType(type, context, input) {
    const flags = type.flags;
    if (flags & (ts.TypeFlags.Any | ts.TypeFlags.Unknown)) {
      throw new Error(`Unresolved contract at ${context}: ${checker.typeToString(type)}`);
    }
    if (flags & ts.TypeFlags.Never) return 'never';
    if (flags & ts.TypeFlags.StringLiteral) return JSON.stringify(type.value);
    if (flags & ts.TypeFlags.NumberLiteral) return String(type.value);
    if (flags & ts.TypeFlags.BooleanLiteral) return type.intrinsicName;
    if (flags & ts.TypeFlags.String) return 'string';
    if (flags & ts.TypeFlags.Number) return 'number';
    if (flags & ts.TypeFlags.Boolean) return 'boolean';
    if (flags & ts.TypeFlags.Null) return 'null';
    if (flags & (ts.TypeFlags.Undefined | ts.TypeFlags.Void)) return 'undefined';
    if (flags & ts.TypeFlags.BigIntLike) throw new Error(`BigInt has no existing JSON wire form: ${context}`);
    if (flags & ts.TypeFlags.NonPrimitive) return 'Record<string, JsonValue>';
    const symbolName = type.aliasSymbol?.name ?? type.symbol?.name;
    if (['JsonValue', 'InputJsonValue'].includes(symbolName)) return 'JsonValue';
    if (['JsonObject', 'InputJsonObject'].includes(symbolName)) return 'JsonObject';
    if (['JsonArray', 'InputJsonArray'].includes(symbolName)) return 'JsonValue[]';
    if (symbolName === 'Date') return 'string';
    const seen = input ? inputNames : names;
    if (seen.has(type)) return seen.get(type);
    const name = `Dto${definitions.size}`;
    seen.set(type, name);
    definitions.set(name, '');
    let value;
    if (type.isUnion()) {
      value = [...new Set(type.types.map((part) => visit(part, context, input)))].sort().join(' | ');
    } else if (checker.isTupleType(type)) {
      const args = checker.getTypeArguments(type);
      const flags = type.target.elementFlags;
      value = `[${args.map((part, index) => {
        const element = arrayElement(part, `${context}[${index}]`, input);
        if (flags[index] & ts.ElementFlags.Rest) return `...Array<${element}>`;
        if (flags[index] & ts.ElementFlags.Variadic) throw new Error(`Unresolved variadic tuple at ${context}[${index}]`);
        return flags[index] & ts.ElementFlags.Optional ? `(${element})?` : element;
      }).join(', ')}]`;
    } else if (checker.isArrayType(type)) {
      value = `Array<${arrayElement(checker.getTypeArguments(type)[0], `${context}[]`, input)}>`;
    } else {
      const toJSON = type.getProperty('toJSON');
      const toJSONType = toJSON && checker.getTypeOfSymbol(toJSON);
      const toJSONSignature = toJSONType?.getCallSignatures()[0];
      if (toJSONSignature) {
        value = visit(checker.getReturnTypeOfSignature(toJSONSignature), `${context}.toJSON()`, input);
      } else if (flags & (ts.TypeFlags.Object | ts.TypeFlags.Intersection)) {
        const fields = [];
        for (const property of checker.getPropertiesOfType(type).sort((a, b) => a.name.localeCompare(b.name, 'en'))) {
          if (property.name.startsWith('__@')) continue;
          // Mapped, inferred and intersected properties may have no AST declaration.
          // The checker resolves instantiated symbols using their retained mapper.
          const fieldType = checker.getTypeOfSymbol(property);
          if (fieldType.getCallSignatures().length) continue;
          const optional = (property.flags & ts.SymbolFlags.Optional) || (!input && hasUndefined(fieldType)) ? '?' : '';
          const field = input ? visit(fieldType, `${context}.${property.name}`, true)
            : withoutUndefined(fieldType, `${context}.${property.name}`, false);
          fields.push(`${JSON.stringify(property.name)}${optional}: ${field};`);
        }
        for (const index of checker.getIndexInfosOfType(type)) {
          const key = index.keyType.flags & ts.TypeFlags.Number ? 'number' : 'string';
          fields.push(`[key: ${key}]: ${visit(index.type, `${context}[${key}]`, input)};`);
        }
        value = fields.length ? `{\n${fields.join('\n')}\n}` : input ? 'Record<string, JsonValue>' : 'Record<string, never>';
      } else {
        throw new Error(`Unsupported contract at ${context}: ${checker.typeToString(type)}`);
      }
    }
    definitions.set(name, value);
    return name;
  }
  return {
    render(type, context, input = false) {
      const start = problems.length;
      const result = visit(type, context, input);
      rejectProblems(problems.slice(start));
      return result;
    },
    declarations() {
      rejectProblems(problems);
      return [...definitions].map(([name, value]) => `export type ${name} = ${value};`).join('\n\n');
    },
  };
}
