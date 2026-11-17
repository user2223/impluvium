// SPDX-License-Identifier: 0BSD
import { lstat, readdir, open } from 'node:fs/promises';
import { O_RDONLY, O_NOFOLLOW } from 'node:constants';
import { fileURLToPath } from 'node:url';
import { join } from 'node:path';
import { createHash } from 'node:crypto';
import { fromHex } from './primitives.ts';
import { RecipeBook, materialize } from './recipes.ts';
import { testdataDirectory, packageDirectory } from './locations.ts';


export type DeliveredFile = { root: 'package' | 'testdata'; file: string; bytes: number };
type ReadInfo = { size: number; dev: number; ino: number; isFile(): boolean; isDirectory(): boolean; isSymbolicLink(): boolean };
function pathParts(file: string): string[] {
  if (typeof file !== 'string' || !file || /[:\\\0]/.test(file) || file.split('/').some(part => !part || part === '.' || part === '..' || part === 'node_modules')) throw Error('invalid_read_path');
  return file.split('/');
}
async function checkedPath(root: URL, file: string): Promise<{ path: string; info: ReadInfo }> {
  const parts = pathParts(file); let path = fileURLToPath(root), info = await lstat(path);
  if (info.isSymbolicLink()) throw Error('read_symlink');
  if (!info.isDirectory()) throw Error('read_root_not_directory');
  for (let i=0;i<parts.length;i++) { path=join(path,parts[i]); info=await lstat(path); if(info.isSymbolicLink())throw Error('read_symlink'); if(i<parts.length-1&&!info.isDirectory())throw Error('read_parent_not_directory'); }
  return {path,info};
}
async function checkedBytes(root: URL, file: string): Promise<Buffer> {
  const before=await checkedPath(root,file);if(!before.info.isFile())throw Error('read_not_file');
  if(before.info.size>1048576)throw Error('delivery_file_limit');
  const handle=await open(before.path,O_RDONLY|O_NOFOLLOW);
  try { const opened=await handle.stat(),after=await checkedPath(root,file); if(opened.dev!==before.info.dev||opened.ino!==before.info.ino||after.info.dev!==opened.dev||after.info.ino!==opened.ino)throw Error('read_path_changed');const bytes=await handle.readFile();if(bytes.length>1048576)throw Error('delivery_file_limit');return bytes; } finally {await handle.close();}
}
export async function readDataBytes(file: string): Promise<Buffer> { return checkedBytes(testdataDirectory,file); }
const packageDocumentation = new Set(['README.md','FORMAT.md','MODULES.md']);
// Source text is required only by the static import and delivered-source path checks.
export const selfCheckSourceFiles = Object.freeze([
 'container/blake3.ts','container/run.mjs',
 'src/api.ts','src/auth-rpc.ts','src/counterexamples.ts','src/coverage.ts','src/documents.ts','src/ed25519.ts','src/events.ts',
 'src/fastcdc.ts','src/flows.ts','src/format-gates.ts','src/gates.ts','src/get-routes.ts',
 'src/json-profile.ts','src/loader.ts','src/locations.ts','src/models.ts','src/paths.ts',
 'src/primitives.ts','src/profile.ts','src/provenance.ts','src/publication.ts','src/recipes.ts',
 'src/rpc.ts','src/state-contracts.ts','src/transport.ts',
 'tooling/check.mjs','tooling/imports.mjs','tooling/options.mjs','tooling/output.mjs','tooling/stage.mjs'
]);
const selfCheckSources = new Set<string>(selfCheckSourceFiles);
export async function readPackageText(file: string): Promise<string> {
 pathParts(file);if(!packageDocumentation.has(file))throw Error('invalid_package_read');
 return (await checkedBytes(packageDirectory,file)).toString('utf8');
}
export async function readSelfCheckText(file: string): Promise<string> {
 pathParts(file);if(!selfCheckSources.has(file))throw Error('invalid_self_check_read');
 return (await checkedBytes(packageDirectory,file)).toString('utf8');
}
export async function listDeliveredFiles(): Promise<DeliveredFile[]> {
 const files:DeliveredFile[]=[],packageFiles=new Set(['package.json','LICENSE',...packageDocumentation]),packageDirs=new Set(['src','tooling','container']);
 async function walk(root:URL,kind:'package'|'testdata',prefix=''):Promise<void>{
  const path=prefix?(await checkedPath(root,prefix)).path:fileURLToPath(root),rootInfo=await lstat(path);if(rootInfo.isSymbolicLink())throw Error('read_symlink');if(!rootInfo.isDirectory())throw Error('read_root_not_directory');
  for(const entry of(await readdir(path,{withFileTypes:true})).sort((a,b)=>a.name<b.name?-1:a.name>b.name?1:0)){
   if(entry.name==='node_modules')continue;
   if(!prefix&&kind==='package'&&!packageFiles.has(entry.name)&&!packageDirs.has(entry.name))continue;
   const file=prefix?prefix+'/'+entry.name:entry.name,info=(await checkedPath(root,file)).info;
   if(info.isDirectory()){if(!prefix&&(kind==='package'?packageDirs.has(entry.name):entry.name==='foreign'))await walk(root,kind,file);continue;}
   if(!info.isFile())throw Error('delivery_special_file');
   if(kind==='package'&&prefix&&!selfCheckSources.has(file))throw Error('delivery_unlisted_package_file');
   files.push({root:kind,file,bytes:info.size});
  }
 }
 await walk(packageDirectory,'package');await walk(testdataDirectory,'testdata');return files;
}

export type Json = null | boolean | number | string | Json[] | { [key: string]: Json };
export type Case = { id: string; operation: string; input: Record<string, any>; expected: Record<string, any>;
  rule: string; provenance: Record<string, any>; classification?: string };
export type Corpus = { format: string; group: string; cases: Case[] };
export type Index = { format: string; groups: { file: string; name?: string; sha256: string; count: number }[];
  assets: { file: string; sha256: string }[]; blocked: { group: string; blocked: string; state:string; condition:string; references:any[];caseIds?:string[] }[] };

function leaves(value: unknown, path = ''): string[] {
  if (Array.isArray(value)) return value.length ? value.flatMap((element, index) => leaves(element, path + '/' + index)) : [path];
  if (value !== null && typeof value === 'object') {
    if (Object.keys(value).length === 1 && ('$bytes' in value || '$utf8' in value)) return [path];
    return Object.entries(value).flatMap(([key, element]) => leaves(element, path + '/' + key));
  }
  return [path];
}
export function validateCorpus(data: unknown): asserts data is Corpus {
  const corpus = data as Corpus;
  if (!corpus || corpus.format !== 'reference-corpus-v1' || typeof corpus.group !== 'string' || !Array.isArray(corpus.cases) || !corpus.cases.length) throw Error('invalid_corpus_format');
  const ids = new Set<string>();
  for (const entry of corpus.cases) {
    if (!entry || typeof entry.id !== 'string' || !/^[a-z0-9][a-z0-9.-]*$/.test(entry.id) || ids.has(entry.id) ||
      typeof entry.operation !== 'string' || typeof entry.rule !== 'string' || !entry.rule.length ||
      !entry.input || !entry.expected || !Object.keys(entry.expected).length || !entry.provenance) throw Error('invalid_case_format:' + entry?.id);
    ids.add(entry.id);
    for (const path of [...leaves(entry.input, '/input'), ...leaves(entry.expected, '/expected')]) {
      const origin = entry.provenance[path];
      if (!origin || !['foreign', 'synthetic', 'calculated', 'normative'].includes(origin.kind) || typeof origin.source !== 'string' || !origin.source.length ||
        !/^[0-9a-f]{64}$/.test(origin.sha256) || typeof origin.selector !== 'string' || !origin.selector.length) throw Error('missing_provenance:' + entry.id + path);
      if (origin.kind === 'calculated' && typeof origin.fieldId !== 'string') throw Error('missing_derivation:' + entry.id + path);
    }
    function validate(value: unknown, key = ''): void {
      if (Array.isArray(value)) { for (const item of value) validate(item, key); return; }
      if (value !== null && typeof value === 'object') { for (const [name, item] of Object.entries(value)) validate(item, name); return; }
      if (key.endsWith('Hex')) fromHex(value as string);
      if (typeof value === 'number' && (!Number.isSafeInteger(value) || value < -2147483648)) throw Error('unsafe_number');
      if (key.endsWith('U64') && (typeof value !== 'string' || value.length > 20 || !/^(0|[1-9][0-9]*)$/.test(value) || BigInt(value) > 18446744073709551615n)) throw Error('invalid_u64');
    }
    validate(entry.input); validate(entry.expected);
  }
}
export async function readIndex(): Promise<Index> {
  const bytes = await readDataBytes('INDEX.json');
  const index: Index = JSON.parse(bytes.toString('utf8'));
  if (!['corpus-index-v1', 'corpus-index-v2'].includes(index.format) || !Array.isArray(index.groups) || !index.groups.length || !Array.isArray(index.assets) || !Array.isArray(index.blocked)) throw Error('invalid_index');
  for (const entry of [...index.groups, ...index.assets]) if (!/^[a-z][a-z0-9.-]*\.json$/.test(entry.file) || !/^[a-f0-9]{64}$/.test(entry.sha256)) throw Error('invalid_index_entry');
  for (const entry of index.groups) if (!Number.isSafeInteger(entry.count) || entry.count < 1) throw Error('invalid_index_count');
  for (const entry of index.blocked) if (!entry.group || !entry.blocked || entry.state!=='blocked'||!entry.condition||!Array.isArray(entry.references)||!entry.references.length||Object.hasOwn(entry, 'expected')) throw Error('invalid_blocked_group');
  return index;
}
export async function loadCorpus(file: string, expectedHash: string): Promise<Corpus> {
  if (!/^[a-z][a-z0-9.-]*\.json$/.test(file)) throw Error('invalid_corpus_path');
  const bytes = await readDataBytes(file);
  return loadCorpusBytes(bytes, expectedHash);
}
export function loadCorpusBytes(bytes: Uint8Array, expectedHash: string): Corpus {
  const actual = createHash('sha256').update(bytes).digest('hex');
  if (actual !== expectedHash) throw Error('corpus_integrity_mismatch');
  let data: any = JSON.parse(Buffer.from(bytes).toString('utf8'));
  if (data?.format === 'reference-corpus-v2') {
    if (!data.rules || !data.origins || !data.values || !Array.isArray(data.cases)) throw Error('invalid_compact_corpus');
    const lookup = (table: any, id: unknown, prefix: string) => {
      if (typeof id !== 'string' || !new RegExp('^' + prefix + '-[0-9]{4}$').test(id) || !Object.hasOwn(table, id)) throw Error('invalid_compact_reference');
      return structuredClone(table[id]);
    };
    const cases = data.cases.map((stored: any) => {
      const expected = lookup(data.values, stored.expected?.$value, 'value'), provenance: Record<string, any> = {};
      for (const path of [...leaves(stored.input, '/input'), ...leaves(expected, '/expected')]) {
        const ref = stored.provenance?.overrides?.[path] ?? stored.provenance?.[path.startsWith('/input/') ? 'input' : 'expected'];
        const origin = lookup(data.origins, ref, 'origin');
        if (origin.fieldId === '$case$path') origin.fieldId = stored.id + path;
        provenance[path] = origin;
      }
      return { ...stored, expected, rule: lookup(data.rules, stored.rule, 'rule'), provenance };
    });
    data = { format: 'reference-corpus-v1', group: data.group, cases };
  }
  validateCorpus(data);
  return data;
}
export async function loadAsset(file: string, index: Index): Promise<any> {
  if (!/^[a-z][a-z0-9.-]*\.json$/.test(file)) throw Error('invalid_asset_path');
  const entry = index.assets.find(item => item.file === file);
  if (!entry) throw Error('asset_not_indexed:' + file);
  const bytes = await readDataBytes(file);
  if (createHash('sha256').update(bytes).digest('hex') !== entry.sha256) throw Error('asset_integrity_mismatch:' + file);
  const data=JSON.parse(bytes.toString('utf8'));
  if(data.format==='field-calculations-v2'){
    if(!Array.isArray(data.fields))throw Error('invalid_compact_calculations');
    if(data.fieldPrefixes!==undefined&&(!Array.isArray(data.fieldPrefixes)||data.fieldPrefixes.some((prefix:any)=>typeof prefix!=='string')))throw Error('invalid_compact_field_prefixes');
    if(data.fieldSuffixes!==undefined&&(!Array.isArray(data.fieldSuffixes)||data.fieldSuffixes.some((suffix:any)=>typeof suffix!=='string')))throw Error('invalid_compact_field_suffixes');
    return {...data,format:'field-calculations-v1',fields:data.fields.map((row:any)=>{
      if(!Array.isArray(row)||row.length!==5||!Array.isArray(row[4]))throw Error('invalid_compact_calculation');
      const second=row[4];if(second.length!==0&&second.length!==2)throw Error('invalid_compact_confirmation');
      let fieldId=row[0];if(Array.isArray(fieldId)){const suffix=typeof fieldId[1]==='string'?fieldId[1]:Number.isSafeInteger(fieldId[1])&&fieldId[1]>=0?data.fieldSuffixes?.[fieldId[1]]:undefined;if(fieldId.length!==2||!Number.isSafeInteger(fieldId[0])||fieldId[0]<0||typeof data.fieldPrefixes?.[fieldId[0]]!=='string'||typeof suffix!=='string')throw Error('invalid_compact_field_id');fieldId=data.fieldPrefixes[fieldId[0]]+suffix;}
      return{fieldId,rule:row[1],method:row[2],sources:row[3],secondCalculation:second.length?{method:'independent',result:'confirmed',checks:second[0],valueSha256:second[1]}:{method:'',result:'pending'}};
    })};
  }
  return data;
}
const books = new Map<string, RecipeBook>();
export async function recipeBook(index: Index): Promise<RecipeBook> {
  const assets = index.assets.filter(item => /^recipes-[0-9]{3}\.json$/.test(item.file));
  const key = assets.map(item => item.sha256).join(':');
  if (books.has(key)) return books.get(key)!;
  const entries = [];
  for (const entry of assets) { const data = await loadAsset(entry.file, index); if (data.format !== 'byte-recipes-v1' || !Array.isArray(data.recipes)) throw Error('invalid_recipe_file'); entries.push(...data.recipes); }
  const book = new RecipeBook(entries); books.set(key, book); return book;
}
export async function* groupCases(name: string, index: Index): AsyncGenerator<Case> {
  const groups = index.groups.filter(item => item.name === name || item.file === name + '.json');
  if (!groups.length) throw Error('missing_group:' + name);
  const book = await recipeBook(index);
  for (const group of groups) for (const stored of (await loadCorpus(group.file, group.sha256)).cases) {
    yield { ...stored, input: materialize(stored.input, book), expected: materialize(stored.expected, book) };
  }
}
