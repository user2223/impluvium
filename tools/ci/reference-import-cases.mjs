import path from 'node:path';
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, writeFileSync, rmSync, symlinkSync } from 'node:fs';
import { scanSources } from './check-reference-imports.mjs';

// All probes inspect files in-process. No fixture module is ever executed.
export function importCounterexamples(ts, root, scratch = '/work/scratch/reference') {
  mkdirSync(scratch, { recursive: true });
  const directory = mkdtempSync(path.join(scratch, 'imports-')), cases = [];
  const ref = path.join(directory, 'reference'), product = path.join(directory, 'product/generated.ts');
  const noble = path.join(directory, 'store/noble');
  const put = (file, text) => { mkdirSync(path.dirname(file), { recursive: true }); writeFileSync(file, text); };
  const entry = path.join(ref, 'entry.ts'), helper = path.join(ref, 'helper.ts');
  const context = { root: directory, ref, runtime: [noble], packages: new Map([[noble, {
    name: '@noble/hashes', version: '1.8.0', exports: { './blake3': { types: './blake3.d.ts', import: './blake3.js' } },
  }]]) };
  try {
    put(product, 'export type Message = string;\n');
    mkdirSync(ref, { recursive: true });
    symlinkSync(product, path.join(ref, 'generated.ts'));
    put(path.join(noble, 'blake3.js'), 'export const blake3 = () => 0;\n');
    put(path.join(noble, 'blake3.d.ts'), 'export declare const blake3: () => number;\n');
    mkdirSync(path.join(ref, 'node_modules/@noble'), { recursive: true });
    symlinkSync(noble, path.join(ref, 'node_modules/@noble/hashes'));
    const reset = () => {
      put(path.join(ref, 'package.json'), JSON.stringify({ type: 'module', imports: { '#generated': './generated.ts', '#allowed': './helper.ts' } }));
      put(helper, 'export type Local = string;\n');
      put(path.join(ref, 'container/run.mjs'), 'import { blake3 } from "@noble/hashes/blake3";\n');
      put(entry, 'import type { Local } from "./helper.ts"; import "./container/run.mjs";\n');
    };
    reset();
    const baseline = scanSources(ts, context, [entry]);
    assert.ok(baseline.files.includes(helper) && baseline.files.includes(path.join(noble, 'blake3.d.ts')));
    cases.push({ id: 'baseline', result: 'passed', files: baseline.files });
    const mutations = [
      ['direct-type', 'import type { Message } from "../product/generated.ts";', /forbidden resolved path/],
      ['alias-type', 'import type { Message } from "#generated";', /forbidden resolved path/],
      ['reexport-type', 'export type { Message } from "./helper.ts";', /forbidden resolved path/, () => put(helper, 'export type { Message } from "../product/generated.ts";')],
      ['import-type-expression', 'type Message = import("../product/generated.ts").Message;', /forbidden resolved path/],
      ['alias-reexport-type', 'export type { Message } from "#allowed";', /forbidden resolved path/, () => put(helper, 'export type { Message } from "../product/generated.ts";')],
      ['dynamic-path', 'const name = "./helper.ts"; await import(name);', /forbidden dynamic import/],
      ['literal-dynamic-product', 'await import("../product/generated.ts");', /forbidden resolved path/],
      ['aliased-require', 'const loader = require; loader("../product/generated.ts");', /forbidden require/],
      ['commonjs-module', 'module.exports = {};', /forbidden reflective capability/],
      ['file-read', 'import { readFileSync as load } from "node:fs";', /forbidden builtin/],
      ['process-loader', 'process.getBuiltinModule("fs");', /forbidden process capability/],
      ['type-tool-in-runtime', 'import type { Node } from "typescript";', /forbidden resolved package/],
      ['native-code', 'import "./binding.wasm";', /executable extension/, () => put(path.join(ref, 'binding.wasm'), '')],
      ['symlink-product', 'import type { Message } from "./linked.ts";', /forbidden resolved path/, () => symlinkSync(product, path.join(ref, 'linked.ts'))],
      ['transitive-package-export', 'import "./container/run.mjs";', /forbidden resolved path/, () => put(path.join(noble, 'blake3.js'), 'export * from "../../product/generated.ts";')],
      ['poisoned-type-export', 'import "./container/run.mjs";', /forbidden resolved path/, () => put(path.join(noble, 'blake3.d.ts'), 'export type { Message } from "../../product/generated.ts";')],
    ];
    for (const [id, text, expected, prepare] of mutations) {
      reset();
      put(path.join(noble, 'blake3.js'), 'export const blake3 = () => 0;\n');
      put(path.join(noble, 'blake3.d.ts'), 'export declare const blake3: () => number;\n');
      prepare?.(); put(entry, text);
      let diagnostic;
      assert.throws(() => scanSources(ts, context, [entry]), error => {
        diagnostic = error.message;
        return expected.test(diagnostic);
      }, id);
      cases.push({ id, result: 'rejected', diagnostic, witness: 'entry.ts', path: text });
    }
    reset();
    put(path.join(noble, 'blake3.js'), 'export const blake3 = () => 0;\n');
    put(path.join(noble, 'blake3.d.ts'), 'export declare const blake3: () => number;\n');
    const original = scanSources(ts, context, [entry]);
    put(entry, '// An irrelevant comment is not a successful mutation.\n' + 'import type { Local } from "./helper.ts"; import "./container/run.mjs";\n');
    assert.deepEqual(scanSources(ts, context, [entry]), original);
    cases.push({ id: 'comment-control', result: 'survived' });
    put(entry, 'export const dependency = { module: "@noble/hashes/blake3" };\n');
    scanSources(ts, context, [entry]);
    cases.push({ id: 'literal-report-key', result: 'passed' });
    assert.ok(root); // caller records the attested source root separately
    const adapter = path.join(ref, 'container/run.mjs');
    put(adapter, 'import { blake3 } from "@noble/hashes/blake3";\n');
    assert.ok(scanSources(ts, { ...context, graph: 'container' }, [adapter]).files.includes(path.join(noble, 'blake3.js')));
    cases.push({ id: 'container-adapter-baseline', result: 'passed' });
    assert.throws(() => scanSources(ts, { ...context, graph: 'local' }, [adapter]), /forbidden container dependency/);
    cases.push({ id: 'local-container-dependency', result: 'rejected' });
    put(entry, 'import "./container/run.mjs";\n');
    assert.throws(() => scanSources(ts, { ...context, graph: 'contract' }, [entry]), /forbidden container module/);
    cases.push({ id: 'contract-container-transitive', result: 'rejected' });
    put(entry, 'import { readFile } from "node:fs/promises";\n');
    assert.throws(() => scanSources(ts, context, [entry]), /forbidden builtin node:fs\/promises/);
    cases.push({ id: 'unbounded-async-file-read', result: 'rejected' });
    put(entry, 'import.meta.resolve("@noble/hashes/blake3");\n');
    assert.throws(() => scanSources(ts, context, [entry]), /forbidden loader capability/);
    cases.push({ id: 'runtime-module-resolution', result: 'rejected' });
    const loader = path.join(ref, 'src/loader.ts'), output = path.join(ref, 'tooling/output.mjs');
    put(loader, 'import { open } from "node:fs/promises"; import { O_RDONLY, O_NOFOLLOW } from "node:constants"; export async function load(path: string) { const handle = await open(path, O_RDONLY | O_NOFOLLOW); try { return await handle.readFile(); } finally { await handle.close(); } }\n');
    const writerPrefix = 'import { open } from "node:fs/promises"; import { O_CREAT, O_EXCL, O_WRONLY, O_NOFOLLOW } from "node:constants"; ';
    put(output, writerPrefix + 'import { randomBytes } from "node:crypto"; export async function output(path, bytes) { let handle; try { handle = await open(path, O_CREAT | O_EXCL | O_WRONLY | O_NOFOLLOW, 384); if (!handle) throw Error("Missing handle"); await handle.writeFile(bytes); await handle.sync(); await handle.close(); handle = undefined; } finally { if (handle) await handle.close(); } }\n');
    scanSources(ts, context, [loader, output]);
    cases.push({ id: 'separate-reader-writer-baseline', result: 'passed' });
    for (const [id, file, code, expected] of [
      ['reader-cannot-write', loader, 'import { writeFile } from "node:fs/promises";', /unapproved filesystem capability/],
      ['writer-cannot-read', output, 'import { readFile } from "node:fs/promises";', /unapproved filesystem capability/],
      ['filesystem-capability-reexport', loader, 'import { lstat as info } from "node:fs/promises"; export { info };', /filesystem capability escapes/],
      ['filesystem-namespace', loader, 'import * as fs from "node:fs/promises";', /unapproved filesystem capability/],
      ['literal-dynamic-own-module', entry, 'await import("./helper.ts");', /forbidden dynamic import/],
      ['literal-require-own-module', entry, 'require("./helper.ts");', /forbidden require/],
    ]) {
      put(file, code);
      assert.throws(() => scanSources(ts, context, [file]), expected);
      cases.push({ id, result: 'rejected' });
    }
    for (const [id, code, expected] of [
      ['writer-open-not-exclusive', writerPrefix + 'async function output(path) { let handle; handle = await open(path, O_CREAT | O_WRONLY | O_NOFOLLOW); }', /unapproved filesystem open flags/],
      ['writer-handle-read', writerPrefix + 'async function output(path) { let handle; handle = await open(path, O_CREAT | O_EXCL | O_WRONLY | O_NOFOLLOW); await handle.readFile(); }', /filesystem handle escapes or uses an unapproved operation/],
      ['writer-handle-export', writerPrefix + 'async function output(path) { let handle; handle = await open(path, O_CREAT | O_EXCL | O_WRONLY | O_NOFOLLOW); return handle; }', /filesystem handle escapes or uses an unapproved operation/],
      ['writer-handle-assignment', writerPrefix + 'async function output(path, other) { let handle; handle = other; }', /unapproved filesystem handle assignment/],
      ['writer-reset-shadowed', writerPrefix + 'async function output(path, undefined) { let handle; handle = undefined; }', /filesystem reset binding is shadowed/],
      ['reader-no-randomness-grant', 'import { randomBytes } from "node:crypto";', /unapproved crypto capability/],
    ]) {
      const file = id === 'reader-no-randomness-grant' ? loader : output;
      put(file, code); assert.throws(() => scanSources(ts, context, [file]), expected);
      cases.push({ id, result: 'rejected' });
    }
    const prefix = 'import { open } from "node:fs/promises"; import { O_RDONLY, O_NOFOLLOW } from "node:constants"; ';
    for (const [id, code, expected] of [
      ['reader-open-flags', prefix + 'async function load(path: string) { const handle = await open(path, O_RDONLY); await handle.close(); }', /unapproved filesystem open flags/],
      ['reader-handle-write', prefix + 'async function load(path: string) { const handle = await open(path, O_RDONLY | O_NOFOLLOW); await handle.writeFile("x"); }', /filesystem handle escapes or uses an unapproved operation/],
      ['reader-handle-export', prefix + 'async function load(path: string) { const handle = await open(path, O_RDONLY | O_NOFOLLOW); return handle; }', /filesystem handle escapes or uses an unapproved operation/],
      ['reader-flags-shadowed', prefix + 'async function load(path: string, O_RDONLY: number) { const handle = await open(path, O_RDONLY | O_NOFOLLOW); await handle.close(); }', /filesystem flag binding is shadowed/],
      ['reader-open-unowned', prefix + 'async function load(path: string) { await open(path, O_RDONLY | O_NOFOLLOW); }', /owned constant handle/],
    ]) {
      put(loader, code); assert.throws(() => scanSources(ts, context, [loader]), expected);
      cases.push({ id, result: 'rejected' });
    }
    reset();
    put(path.join(noble, 'blake3.d.ts'), 'export declare const blake3: () => number; export type { Hash } from "./_md.d.ts";\n');
    put(path.join(noble, '_md.d.ts'), 'export type { Hash } from "./utils.ts";\n');
    put(path.join(noble, 'utils.d.ts'), 'export type Hash = string;\n');
    assert.ok(scanSources(ts, context, [entry]).files.includes(path.join(noble, 'utils.d.ts')));
    cases.push({ id: 'declaration-extension-baseline', result: 'passed' });
    put(path.join(noble, '_md.d.ts'), 'export type { Hash } from "./missing.ts";\n');
    assert.throws(() => scanSources(ts, context, [entry]), /unresolved file import/);
    cases.push({ id: 'missing-declaration-import', result: 'rejected' });
    return cases;
  } finally { rmSync(directory, { recursive: true, force: true }); }
}
