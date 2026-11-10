import assert from 'node:assert/strict';
import { join } from 'node:path';
import test from 'node:test';
import { compiler, materializer } from '../compiler.mjs';
import { callbackResponseType, rootDirectory as root } from '../generate.mjs';

function inspect(source, name = 'wire-type-fixture') {
  const ts = compiler(root);
  const file = join(root, 'server', `${name}.ts`);
  const options = {
    noEmit: true, strict: true, types: [], skipLibCheck: true,
    target: ts.ScriptTarget.ESNext, module: ts.ModuleKind.ESNext,
    moduleResolution: ts.ModuleResolutionKind.Bundler,
  };
  const host = ts.createCompilerHost(options);
  const read = host.readFile.bind(host);
  const exists = host.fileExists.bind(host);
  host.readFile = (path) => path === file ? source : read(path);
  host.fileExists = (path) => path === file || exists(path);
  const program = ts.createProgram({ rootNames: [file], options, host });
  const diagnostics = ts.getPreEmitDiagnostics(program);
  assert.deepEqual(diagnostics.map((diagnostic) => ts.flattenDiagnosticMessageText(diagnostic.messageText, '\n')), []);
  const checker = program.getTypeChecker();
  const aliases = new Map(program.getSourceFile(file).statements.filter(ts.isTypeAliasDeclaration).map((node) => [node.name.text, checker.getTypeAtLocation(node)]));
  return { ts, checker, aliases };
}

test('materialized wire types preserve dates, null and omitted fields and reject unresolved types', () => {
  const ts = compiler(root);
  const file = join(root, 'tools/ci/fixtures/wire-type-probe.ts');
  const source = `
    type Sample = { created: Date; nullable: string | null; missing: string | undefined; optional?: number; flags: boolean[]; values: (number | undefined)[] };
    type Unsafe = { result: any };
    type Unresolved = { result: unknown };
    type HugeInteger = { result: bigint };
    declare function defineHandler<Request = object, Response = any>(handler: (request: Request) => Response): () => Response;
    export default defineHandler<{ sample: string }>(async (_request) => ({ valid: true }));
  `;
  const options = { noEmit: true, strict: true, types: [], target: ts.ScriptTarget.ESNext };
  const host = ts.createCompilerHost(options);
  const original = host.readFile.bind(host);
  const exists = host.fileExists.bind(host);
  host.readFile = (path) => path === file ? source : original(path);
  host.fileExists = (path) => path === file || exists(path);
  const program = ts.createProgram({ rootNames: [file], options, host });
  const checker = program.getTypeChecker();
  const aliases = new Map(program.getSourceFile(file).statements.filter(ts.isTypeAliasDeclaration).map((node) => [node.name.text, checker.getTypeAtLocation(node)]));
  const dto = materializer(ts, checker);
  dto.render(aliases.get('Sample'), 'sample');
  const output = dto.declarations();
  assert.match(output, /"created": string/);
  assert.match(output, /"missing"\?: string/);
  assert.match(output, /"optional"\?: number/);
  assert.match(output, /null \| string/);
  assert.match(output, /Array<number \| null>|Array<null \| number>/);
  assert.doesNotMatch(output, /Date|undefined|\bany\b|\bunknown\b/);
  for (const name of ['Unsafe', 'Unresolved']) {
    assert.throws(() => dto.render(aliases.get(name), name), /Unresolved contract/);
  }
  assert.throws(() => dto.render(aliases.get('HugeInteger'), 'integer'), /BigInt/);
  const callback = materializer(ts, checker);
  const callbackType = callback.render(callbackResponseType(ts, checker, program.getSourceFile(file)), 'callback');
  const callbackDeclarations = callback.declarations();
  assert.doesNotMatch(callbackDeclarations, /\bany\b/);
  // boolean and false | true have the same values; verify the type domain,
  // rather than the compiler's choice of textual spelling. Both directions
  // reject a narrowed literal, and the explicit guard rejects an untyped result.
  inspect(`${callbackDeclarations}
    type IsAny<T> = 0 extends (1 & T) ? true : false;
    type Assert<T extends true> = T;
    type Compatible<A, B> = [A] extends [B] ? [B] extends [A] ? true : false : false;
    type CallbackBoolean = Assert<Compatible<${callbackType}["valid"], boolean>>;
    type CallbackTyped = Assert<IsAny<${callbackType}["valid"]> extends false ? true : false>;
  `, 'materialized-callback-fixture');
});

test('mapped and composed schema inputs retain defaults, optionality, enums and morph inputs', () => {
  // The compiler reads arktype declarations; no schema is imported or run here.
  const { ts, checker, aliases } = inspect(`
    import { type } from "arktype";
    import { configure } from "arktype/config";
    const throwing = configure({ onFail: errors => errors.throw(), actual: () => "" });
    declare global { interface ArkEnv { onFail: typeof throwing.onFail } }
    const shared = type({ username: "string >= 5", email: "string.email" });
    const invite = shared.partial().and({ expires: "string", isAdmin: "boolean = false" })
      .configure(throwing);
    const platforms = { Linux: "Linux", Windows: "Windows" } as const;
    const version = type({
      launches: type({ platform: type.valueOf(platforms), suggestions: "string[]?" }).array(),
      onlySetup: "boolean = false",
      requiredContent: type("string").array().default(() => []),
      expiry: "string.date.iso.parse?",
    });
    const store = type({ take: type("string").pipe(s => Number.parseInt(s)).default("10") });
    const manifest = type({
      key: "16 <= number[] <= 16",
      chunks: type({ ["string"]: { files: type({ filename: "string", length: "number" }).array() } }),
    });
    const dictionary = type({ "[string]": { filename: "string", length: "number" } });
    type InviteInput = typeof invite.inferIn;
    type InviteOutput = typeof invite.infer;
    type VersionInput = typeof version.inferIn;
    type StoreInput = typeof store.inferIn;
    type ManifestInput = typeof manifest.inferIn;
    type DictionaryInput = typeof dictionary.inferIn;
    type PatchInput = Partial<Omit<InviteOutput, "email">> & { audit?: string };
    type MappedInput = { [Key in "flag" | "label"]?: Key extends "flag" ? boolean : string };
    type TupleInput = [string, number?];
    type RestInput = [string, ...number[]];
  `);
  const dto = materializer(ts, checker);
  const names = new Map([...aliases].map(([name, type]) => [name, dto.render(type, name, true)]));
  const output = dto.declarations();
  assert.doesNotMatch(output, /\bany\b|\bunknown\b|\bDate\b/);
  inspect(`${output}
    type Compatible<A, B> = [A] extends [B] ? [B] extends [A] ? true : false : false;
    type Assert<T extends true> = T;
    type InviteInputCheck = Assert<Compatible<${names.get('InviteInput')}, { username?: string; email?: string; expires: string; isAdmin?: boolean }>>;
    type InviteOutputCheck = Assert<Compatible<${names.get('InviteOutput')}, { username?: string; email?: string; expires: string; isAdmin: boolean }>>;
    type VersionCheck = Assert<Compatible<${names.get('VersionInput')}, { launches: { platform: "Linux" | "Windows"; suggestions?: string[] }[]; onlySetup?: boolean; requiredContent?: string[]; expiry?: string }>>;
    type StoreCheck = Assert<Compatible<${names.get('StoreInput')}, { take?: string }>>;
    type PatchCheck = Assert<Compatible<${names.get('PatchInput')}, { username?: string; expires?: string; isAdmin?: boolean; audit?: string }>>;
    type MappedCheck = Assert<Compatible<${names.get('MappedInput')}, { flag?: boolean; label?: string }>>;
    type TupleCheck = Assert<Compatible<${names.get('TupleInput')}, [string, number?]>>;
    type RestCheck = Assert<Compatible<${names.get('RestInput')}, [string, ...number[]]>>;
    const manifest: ${names.get('ManifestInput')} = { key: [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0], chunks: { string: { files: [{ filename: "sample", length: 1 }] } } };
    const dictionary: ${names.get('DictionaryInput')} = { sample: { filename: "sample", length: 1 } };
    void manifest;
    void dictionary;
  `, 'materialized-schema-fixture');
});

test('materialization reports every invalid field and refuses to emit partial declarations', () => {
  const { ts, checker, aliases } = inspect('type Invalid = { first: any; nested: { second: unknown; huge: bigint } };');
  const dto = materializer(ts, checker);
  assert.throws(() => dto.render(aliases.get('Invalid'), 'sample Body', true), (error) => {
    assert.ok(error instanceof AggregateError);
    assert.equal(error.errors.length, 3);
    for (const field of ['first', 'nested.second', 'nested.huge']) assert.ok(error.message.includes(`sample Body.${field}`));
    return true;
  });
  assert.throws(() => dto.declarations(), AggregateError);
});
