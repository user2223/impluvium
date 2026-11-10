import assert from 'node:assert/strict';
import { join } from 'node:path';
import test from 'node:test';
import { compiler } from '../compiler.mjs';
import { rootDirectory as root } from '../generate.mjs';
import { comparisonResponse, comparisonTypes, responseComparator, responseMismatch } from '../nitro-comparison.mjs';

function fixture(source) {
  const ts = compiler(root);
  const file = join(root, 'server/nitro-comparison-type-fixture.ts');
  const options = {
    noEmit: true, strict: true, types: [], skipLibCheck: true,
    target: ts.ScriptTarget.ESNext, module: ts.ModuleKind.ESNext,
    moduleResolution: ts.ModuleResolutionKind.Bundler,
  };
  const host = ts.createCompilerHost(options);
  const read = host.readFile.bind(host);
  const exists = host.fileExists.bind(host);
  host.readFile = (path) => path === file ? `export {};\n${comparisonTypes}\n${source}` : read(path);
  host.fileExists = (path) => path === file || exists(path);
  const program = ts.createProgram({ rootNames: [file], options, host });
  assert.deepEqual(ts.getPreEmitDiagnostics(program).map((diagnostic) =>
    ts.flattenDiagnosticMessageText(diagnostic.messageText, '\n')), []);
  const checker = program.getTypeChecker();
  const aliases = new Map(program.getSourceFile(file).statements.filter(ts.isTypeAliasDeclaration)
    .map((node) => [node.name.text, checker.getTypeAtLocation(node)]));
  const compare = responseComparator(ts, checker, aliases);
  return (contract, observed, raw, kind = 'json') => {
    assert.ok(aliases.has(contract), `Missing fixture contract: ${contract}`);
    assert.ok(aliases.has(observed), `Missing fixture response: ${observed}`);
    if (raw) assert.ok(aliases.has(raw), `Missing fixture handler: ${raw}`);
    return compare(aliases.get(contract), aliases.get(observed), { raw: aliases.get(raw), kind });
  };
}

test('only a proven void result can turn Nitro never into an empty response', () => {
  const compare = fixture(`
    import type { Serialize } from "nitropack/types";
    type Void = void;
    type NitroVoid = Serialize<Void>;
    type Empty = undefined;
    type Throwing = never;
    type Untyped = any;
    type Json = { value: string };
  `);
  assert.deepEqual(compare('Empty', 'NitroVoid', 'Void', 'empty'), []);
  assert.deepEqual(compare('Empty', 'Empty', 'Empty', 'empty'), []);
  for (const raw of ['Throwing', 'Untyped', 'Json']) {
    assert.notDeepEqual(compare('Empty', 'NitroVoid', raw, 'empty'), [], raw);
  }
  assert.notDeepEqual(compare('Empty', 'NitroVoid', 'Void', 'json'), []);
  assert.deepEqual(compare('Json', 'Json', 'Json'), []);
});

const emulatorFixture = `
  type NitroVersion = {
    versionId: string;
    launches: { launchId: string; emulator: { launchId: string; gameId: string } | undefined }[];
  };
  type VersionContract = {
    versionId: string;
    launches: { launchId: string; emulator?: { launchId: string; gameId: string; gameVersion?: never } }[];
  };
`;

test('undefined fields normalize to omission at any depth without changing other values', () => {
  const compare = fixture(`${emulatorFixture}
    type Required = { versionId: string; launches: { launchId: string; emulator: { launchId: string; gameId: string } }[] };
    type Nullable = { versionId: string; launches: { launchId: string; emulator: { launchId: string; gameId: string } | null | undefined }[] };
    type Untyped = { versionId: string; launches: { launchId: string; emulator: any }[] };
    type Missing = { versionId: string; launches: { launchId: string; emulator: undefined }[] };
  `);
  assert.deepEqual(compare('VersionContract', 'NitroVersion'), []);
  for (const observed of ['Required', 'Nullable', 'Untyped', 'Missing']) {
    assert.notDeepEqual(compare('VersionContract', observed), [], observed);
  }
  // Only static handler type evidence is requested; no route-specific bridge is selected.
  const route = { method: 'GET', path: '/arbitrary', kind: 'json', file: 'server/server/api/v1/arbitrary.get.ts' };
  assert.deepEqual(comparisonResponse(route, 0), {
    declarations: ['type Raw0 = Awaited<ReturnType<typeof import("./server/api/v1/arbitrary.get").default>>;'],
  });
});

test('response errors retain route names and reject changes outside omission rules', () => {
  const compare = fixture(`${emulatorFixture}
    type ChangedRoot = Omit<VersionContract, "versionId"> & { versionId: number };
    type ChangedNested = { versionId: string; launches: { launchId: string; emulator?: { launchId: string; gameId: number } }[] };
    type MissingField = { launches: VersionContract["launches"] };
    type Empty = undefined;
    type Json = { value: string };
  `);
  for (const contract of ['ChangedRoot', 'ChangedNested', 'MissingField']) {
    const errors = compare(contract, 'NitroVersion');
    assert.notDeepEqual(errors, []);
    assert.match(responseMismatch('GET /sample/:id', errors), /Response mismatch/);
    assert.match(responseMismatch('GET /sample/:id', errors), /GET \/sample\/:id/);
  }
  assert.notDeepEqual(compare('Empty', 'Json', 'Json', 'empty'), []);
});

test('real Nitro serialization preserves the five documented representation classes', () => {
  const compare = fixture(`
    import type { Serialize, Simplify } from "nitropack/types";
    import type { JsonValue } from "@prisma/client/runtime/client";
    type Source = { id: string; working: boolean; options: JsonValue; fsStats?: { freeSpace: number; totalSpace: number } | undefined };
    type Sources = Simplify<Serialize<Source[]>>;
    type SourcesRaw = Source[];
    type SourcesContract = { id: string; working: false | true; options: WireJsonValue; fsStats?: { freeSpace: number; totalSpace: number } }[];

    type Task = { id: string; actions: string[]; error: JsonValue | null; success: boolean };
    type TasksRaw = { historicalTasks: Task[]; runningTasks: string[]; dailyTasks: ("daily" | "weekly")[] };
    type Tasks = Simplify<Serialize<TasksRaw>>;
    type TasksContract = { historicalTasks: { id: string; actions: string[]; error: WireJsonValue; success: boolean }[]; runningTasks: string[]; dailyTasks: ("daily" | "weekly")[] };

    type AuthRaw = { enabledAuthProviders: string[]; oidcProviderName: string | undefined };
    type Auth = Simplify<Serialize<AuthRaw>>;
    type AuthContract = { enabledAuthProviders: string[]; oidcProviderName?: string };

    declare enum Mode { Callback = "callback", Code = "code" }
    declare enum Capability { Peer = "peerAPI", User = "userStatus", Saves = "cloudSaves" }
    type ClientRaw = { name: string; mode: Mode; capabilities: Partial<{ [Capability.Peer]: object; [Capability.User]: object; [Capability.Saves]: object }> };
    type Client = Simplify<Serialize<ClientRaw>>;
    type ClientContract = { name: string; mode: "callback" | "code"; capabilities: { peerAPI?: WireJsonObject; userStatus?: WireJsonObject; cloudSaves?: WireJsonObject } };

    type VersionRaw = { created: Date; delta: boolean; dropletManifest: JsonValue; launches: { launchId: string; emulator: { launchId: string; gameId: string; gameVersion: undefined } | undefined }[] };
    type Version = Simplify<Serialize<VersionRaw>>;
    type VersionContract = { created: string; delta: boolean; dropletManifest: WireJsonValue; launches: { launchId: string; emulator?: { launchId: string; gameId: string; gameVersion?: never } }[] };
  `);
  for (const name of ['Sources', 'Tasks', 'Auth', 'Client', 'Version']) {
    assert.deepEqual(compare(`${name}Contract`, name, `${name}Raw`), [], name);
  }
});

test('normalization rejects changed fields, missing fields and changed scalar domains', () => {
  const compare = fixture(`
    type StoredJsonResponse = import("nitropack/types").Serialize<import("@prisma/client/runtime/client").JsonValue>;
    type Source = { id: string; options: StoredJsonResponse }[];
    type WrongSource = { id: number; options: WireJsonValue }[];
    type Task = { historicalTasks: { error: StoredJsonResponse; success: boolean }[] };
    type WrongTask = { historicalTasks: { error: WireJsonValue; success: string }[] };
    type Auth = { enabledAuthProviders: string[]; oidcProviderName: string | undefined };
    type WrongName = { enabledAuthProviders: number[]; oidcProviderName?: string };
    type Mode = { name: string; mode: "callback" | "code" };
    type WrongMode = { name: string; mode: "other" };
    type Renamed = { label: string; mode: "callback" | "code" };
    type Missing = { mode: "callback" | "code" };
    type ExtraOptional = { name: string; mode: "callback" | "code"; extra?: number };
    type NarrowJson = { id: string; options: { endpoint: string } }[];
    type RequiredName = { enabledAuthProviders: string[]; oidcProviderName: string };
    type NarrowName = { enabledAuthProviders: string[]; oidcProviderName?: "specific" };
  `);
  for (const [contract, observed] of [
    ['WrongSource', 'Source'], ['WrongTask', 'Task'], ['WrongName', 'Auth'], ['WrongMode', 'Mode'],
    ['Renamed', 'Mode'], ['Missing', 'Mode'], ['ExtraOptional', 'Mode'], ['NarrowJson', 'Source'],
    ['RequiredName', 'Auth'], ['NarrowName', 'Auth'],
  ]) {
    assert.notDeepEqual(compare(contract, observed), [], `${contract} / ${observed}`);
    assert.notDeepEqual(compare(observed, contract), [], `${observed} / ${contract}`);
  }
});

test('open objects require raw type evidence and any is rejected even inside JSON domains', () => {
  const compare = fixture(`
    import type { Serialize } from "nitropack/types";
    type OpenRaw = { settings?: object };
    type Open = Serialize<OpenRaw>;
    type OpenContract = { settings?: WireJsonObject };
    type ClosedRaw = { settings?: {} };
    type AnyRaw = { settings?: any };
    type NarrowRaw = { settings?: { endpoint: string } };
    type Narrow = Serialize<NarrowRaw>;
    type RequiredRaw = { settings: object };
    type Required = Serialize<RequiredRaw>;
    type WrongKeyRaw = { other?: object };
    type WrongKey = Serialize<WrongKeyRaw>;
    type Unsafe = any;
    type UnsafeField = { settings: any };
    type UnsafeArray = { settings?: any[] };
    type UnsafeJson = { settings?: string | number | boolean | null | any[] | Record<string, any> };
    type Unknown = { settings?: unknown };
    type Safe = { settings?: WireJsonValue };
    type TupleRaw = { settings: [{}, object] };
    type Tuple = Serialize<TupleRaw>;
    type TupleContract = { settings: [Record<string, never>, WireJsonObject] };
    type WrongTupleContract = { settings: [WireJsonObject, WireJsonObject] };
    type UnionRaw = { settings: string | object };
    type Union = Serialize<UnionRaw>;
    type UnionContract = { settings: string | WireJsonObject };
  `);
  assert.deepEqual(compare('OpenContract', 'Open', 'OpenRaw'), []);
  for (const raw of [undefined, 'ClosedRaw', 'AnyRaw']) {
    assert.notDeepEqual(compare('OpenContract', 'Open', raw), [], raw ?? 'No evidence');
  }
  for (const name of ['Narrow', 'Required', 'WrongKey']) {
    assert.notDeepEqual(compare('OpenContract', name, `${name}Raw`), [], name);
  }
  for (const name of ['Unsafe', 'UnsafeField', 'UnsafeArray', 'UnsafeJson', 'Unknown']) {
    assert.match(compare('Safe', name).join('\n'), /unresolved (any|unknown)/, name);
    assert.match(compare(name, 'Safe').join('\n'), /unresolved (any|unknown)/, name);
    assert.match(compare(name, name).join('\n'), /unresolved (any|unknown)/, `Both sides: ${name}`);
  }
  assert.deepEqual(compare('TupleContract', 'Tuple', 'TupleRaw'), []);
  assert.notDeepEqual(compare('WrongTupleContract', 'Tuple', 'TupleRaw'), []);
  assert.deepEqual(compare('UnionContract', 'Union', 'UnionRaw'), []);
});

test('nested library JSON and omitted authentication and version fields use the same rules', () => {
  const compare = fixture(`
    import type { Serialize, Simplify } from "nitropack/types";
    import type { JsonValue } from "@prisma/client/runtime/client";
    type Source = { id: string; options: JsonValue };
    type SourceWire = { id: string; options: WireJsonValue };
    type HomeRaw = { gameCount: number; sources: Source[] };
    type Home = Simplify<Serialize<HomeRaw>>;
    type HomeContract = { gameCount: number; sources: SourceWire[] };
    type ImportRaw = { unimportedGames: { game: string; library: Source }[] };
    type Import = Simplify<Serialize<ImportRaw>>;
    type ImportContract = { unimportedGames: { game: string; library: SourceWire }[] };
    type LibraryRaw = { results: { game: { id: string; library: Source }; status: "offline" | { noVersions: boolean } }[]; count: number };
    type Library = Simplify<Serialize<LibraryRaw>>;
    type LibraryContract = { results: { game: { id: string; library: SourceWire }; status: "offline" | { noVersions: boolean } }[]; count: number };
    type AuthRaw = { Simple: boolean; OpenID: { scopes: string; adminGroup: string | undefined } | undefined };
    type Auth = Simplify<Serialize<AuthRaw>>;
    type AuthContract = { Simple: boolean; OpenID?: { scopes: string; adminGroup?: string } };
    type MassRaw = { id: string; versions: { name: string }[] | undefined }[];
    type Mass = Simplify<Serialize<MassRaw>>;
    type MassContract = { id: string; versions?: { name: string }[] }[];
  `);
  for (const name of ['Home', 'Import', 'Library', 'Auth', 'Mass']) {
    assert.deepEqual(compare(`${name}Contract`, name, `${name}Raw`), [], name);
  }
});

test('stale handler annotations cannot hide additional callback response fields', () => {
  const compare = fixture(`
    import type { Serialize } from "nitropack/types";
    type AnnotatedRaw = { game: { versions: { versionId: string }[] }; unimportedVersions: string[] | undefined };
    type Observed = Serialize<AnnotatedRaw>;
    type CallbackContract = { game: { versions: { versionId: string; clientSize: { downloadSize: number } | null; diskSize: number | null }[] }; unimportedVersions?: string[] };
    type CorrectedRaw = { game: CallbackContract["game"]; unimportedVersions: string[] | undefined };
    type Corrected = Serialize<CorrectedRaw>;
  `);
  const errors = compare('CallbackContract', 'Observed', 'AnnotatedRaw');
  assert.ok(errors.some((error) => error.includes('clientSize') && error.includes('missing')));
  assert.ok(errors.some((error) => error.includes('diskSize') && error.includes('missing')));
  assert.deepEqual(compare('CallbackContract', 'Corrected', 'CorrectedRaw'), []);
});

test('only numerically redundant string-dictionary indices share a wire representation', () => {
  const compare = fixture(`
    import type { Serialize, Simplify } from "nitropack/types";
    type Dictionary = { values: { [key: string]: { filename: string; chunks: { [key: string]: number[] } } } };
    type NitroDictionary = Simplify<Serialize<Dictionary>>;
    type ExplicitNumeric = { values: { [key: string]: string; [key: number]: string } };
    type StringOnly = { values: { [key: string]: string } };
    type NarrowNumeric = { values: { [key: string]: string; [key: number]: "limited" } };
    type NumberOnly = { values: { [key: number]: string } };
    type WrongValue = { values: { [key: string]: number } };
    type UnsafeNumeric = { values: { [key: string]: string; [key: number]: any } };
    type ExtraField = { values: { [key: string]: string; label: string } };
  `);
  assert.deepEqual(compare('Dictionary', 'NitroDictionary', 'Dictionary'), []);
  assert.deepEqual(compare('StringOnly', 'ExplicitNumeric'), []);
  assert.deepEqual(compare('ExplicitNumeric', 'StringOnly'), []);
  for (const name of ['NarrowNumeric', 'NumberOnly', 'WrongValue', 'UnsafeNumeric', 'ExtraField']) {
    assert.notDeepEqual(compare('StringOnly', name), [], name);
    assert.notDeepEqual(compare(name, 'StringOnly'), [], name);
  }
});

test('in-place MFA sanitization preserves the model dictionary type rather than a keyed union', () => {
  const compare = fixture(`
    import type { Serialize, Simplify } from "nitropack/types";
    import type { JsonValue } from "@prisma/client/runtime/client";
    type MfaRow = { userId: string; mec: "TOTP" | "WebAuthn"; enabled: boolean; version: number; credentials: JsonValue };
    declare const rows: MfaRow[];
    const mapped = Object.fromEntries(rows.map(row => {
      row.credentials = row.mec === "TOTP" ? {} : [{ name: "sample", id: "sample", created: 1 }];
      return [row.mec, row];
    }));
    type Raw = { mecs: typeof mapped; available: string[] };
    type Nitro = Simplify<Serialize<Raw>>;
    type WireRow = { userId: string; mec: "TOTP" | "WebAuthn"; enabled: boolean; version: number; credentials: WireJsonValue };
    type Contract = { mecs: Record<string, WireRow>; available: string[] };
    type OldContract = { mecs: Partial<{
      TOTP: { userId: string; mec: "TOTP"; enabled: boolean; version: number; credentials: Record<string, never> };
      WebAuthn: { userId: string; mec: "WebAuthn"; enabled: boolean; version: number; credentials: { name: string; id: string; created: number }[] };
    }>; available: string[] };
  `);
  assert.deepEqual(compare('Contract', 'Nitro', 'Raw'), []);
  assert.notDeepEqual(compare('OldContract', 'Nitro', 'Raw'), []);
});
