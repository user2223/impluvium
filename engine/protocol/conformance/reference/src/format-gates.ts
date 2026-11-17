// SPDX-License-Identifier: 0BSD
import { deepStrictEqual, throws } from 'node:assert';
import { RecipeBook, canonicalSpans } from './recipes.ts';
import type { Recipe } from './recipes.ts';
import {listDeliveredFiles}from'./loader.ts';
const emptyHash = 'e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855';
const empty = (): Recipe => ({ id: 'bytes-000000', op: 'literal', hex: '', byteLengthU64: '0', sha256Hex: emptyHash });
export function recipeFormatGates(): { id: string; result: 'pass'; path: string }[] {
  const results: { id: string; result: 'pass'; path: string }[] = [];
  const probe = (id: string, nodes: Recipe[], error: RegExp, action = (book: RecipeBook) => book.verify(nodes.at(-1)!.id)) => {
    throws(() => action(new RecipeBook(nodes)), error); results.push({ id: 'recipe.reject.' + id, result: 'pass', path: 'src/recipes.ts' });
  };
  deepStrictEqual(new RecipeBook([empty()]).buffer('bytes-000000'), Buffer.alloc(0));
  results.push({ id: 'recipe.empty-known-answer', result: 'pass', path: 'src/recipes.ts/literal' });
  probe('unknown-operation', [{ ...empty(), op: 'javascript' }], /recipe_unknown_operation/);
  probe('extra-field', [{ ...empty(), program: 'ignored' }], /recipe_invalid_fields/);
  probe('wrong-length', [{ ...empty(), byteLengthU64: '1' }], /recipe_length_mismatch/);
  probe('wrong-digest', [{ ...empty(), sha256Hex: '00'.repeat(32) }], /recipe_digest_mismatch/);
  probe('uppercase-hex', [{ ...empty(), hex: 'AA', byteLengthU64: '1' }], /recipe_invalid_hex/);
  probe('literal-limit', [{ ...empty(), hex: '00'.repeat(65537), byteLengthU64: '65537' }], /recipe_literal_limit/);
  probe('missing-reference', [{ ...empty(), op: 'repeat', source: 'bytes-000099', countU64: '1', hex: undefined }].map(({hex,...node}) => node), /recipe_missing_reference/);
  probe('cycle', [{ id: 'bytes-000000', op: 'repeat', source: 'bytes-000000', countU64: '1', byteLengthU64: '0', sha256Hex: emptyHash }], /recipe_cycle/);
  const zero = empty();
  const depth: Recipe[] = [zero];
  for (let i = 1; i <= 32; i++) depth.push({ id: 'bytes-' + String(i).padStart(6, '0'), op: 'repeat', source: depth.at(-1)!.id, countU64: '1', byteLengthU64: '0', sha256Hex: emptyHash });
  probe('depth', depth, /recipe_depth_limit/);
  const count = Array.from({ length: 8193 }, (_, i) => ({ ...empty(), id: 'bytes-' + String(i).padStart(6, '0') }));
  probe('stored-nodes', count, /recipe_node_limit/);
  probe('child-count', [zero, { id: 'bytes-000001', op: 'concat', parts: Array(129).fill(zero.id), byteLengthU64: '0', sha256Hex: emptyHash }], /recipe_child_limit/);
  const visits = [zero];
  for (let i = 1; i < 4; i++) visits.push({ id: 'bytes-' + String(i).padStart(6, '0'), op: 'concat', parts: Array(64).fill(visits.at(-1)!.id), byteLengthU64: '0', sha256Hex: emptyHash });
  probe('bounded-traversal', visits, /recipe_traversal_limit/);
  const reachable: Recipe[] = Array.from({length:255},(_,i)=>({...empty(),id:'bytes-'+String(i).padStart(6,'0')}));
  reachable.push({id:'bytes-000255',op:'concat',parts:reachable.slice(0,128).map(row=>row.id),byteLengthU64:'0',sha256Hex:emptyHash});
  reachable.push({id:'bytes-000256',op:'concat',parts:reachable.slice(128,255).map(row=>row.id),byteLengthU64:'0',sha256Hex:emptyHash});
  reachable.push({id:'bytes-000257',op:'concat',parts:['bytes-000255','bytes-000256'],byteLengthU64:'0',sha256Hex:emptyHash});
  probe('reachable-nodes',reachable,/recipe_reachable_limit/);
  probe('slice-bounds', [zero, { id: 'bytes-000001', op: 'slice', source: zero.id, fromU64: '1', takeU64: '0', byteLengthU64: '0', sha256Hex: emptyHash }], /recipe_slice_bounds/);
  probe('insert-bounds', [zero, { id: 'bytes-000001', op: 'insert', source: zero.id, insert: zero.id, atU64: '1', byteLengthU64: '0', sha256Hex: emptyHash }], /recipe_insert_bounds/);
  const counter = { id: 'bytes-000000', op: 'counter', startU64: '0', stepU64: '1', width: 8, endian: 'little', outputBytesU64: '0', byteLengthU64: '0', sha256Hex: emptyHash };
  probe('counter-width', [{ ...counter, width: 3 }], /recipe_counter_parameters/);
  probe('counter-endian', [{ ...counter, endian: 'host' }], /recipe_counter_parameters/);
  probe('counter-value', [{ ...counter, startU64: '18446744073709551616' }], /recipe_integer_overflow/);
  probe('designated-length', [{ ...counter, outputBytesU64: '1073741825', byteLengthU64: '1073741825' }], /recipe_length_limit/);
  probe('materialization', [{ ...counter, outputBytesU64: '33554433', byteLengthU64: '33554433' }], /recipe_materialization_limit/, book => { book.buffer(counter.id); });
  deepStrictEqual(canonicalSpans([{ offsetU64: '0', lengthU64: '2' }, { offsetU64: '2', lengthU64: '1' }]).toString('hex'), '303a320a323a310a');
  throws(() => canonicalSpans([{ offsetU64: '1', lengthU64: '1' }]), /invalid_canonical_spans/);
  throws(() => canonicalSpans([{ offsetU64: '0', lengthU64: '0' }]), /invalid_canonical_spans/);
  results.push({ id: 'recipe.canonical-span-encoding', result: 'pass', path: 'src/recipes.ts/canonicalSpans' });
  return results;
}
export function enforceSizes(files: { file: string; bytes: number }[], totalLimit: number, fileLimit?: number): void {
  if (files.some(row => !Number.isSafeInteger(row.bytes) || row.bytes < 0)) throw Error('invalid_size_inventory');
  if (fileLimit !== undefined && files.some(row => row.bytes > fileLimit)) throw Error('delivery_file_limit');
  if (files.reduce((sum, row) => sum + row.bytes, 0) > totalLimit) throw Error('delivery_total_limit');
}
export async function deliverySizeGate(): Promise<{ id: string; result: 'pass'; path: string; evidence: unknown }> {
  const files=(await listDeliveredFiles()).map(row=>({file:(row.root==='package'?'conformance/reference/':'testdata/')+row.file,bytes:row.bytes}));
  enforceSizes(files, 8388608, 1048576);
  throws(() => enforceSizes([{file:'probe',bytes:1048577}], 8388608, 1048576), /delivery_file_limit/);
  throws(() => enforceSizes(Array.from({length:9}, (_, i) => ({file:'probe-'+i,bytes:1048576})),8388608,1048576), /delivery_total_limit/);
  return { id: 'delivery.hard-size-bounds', result: 'pass', path: 'src/format-gates.ts', evidence: {
    deliveryBytes: files.reduce((sum, row) => sum + row.bytes, 0),
    largest: [...files].sort((a,b) => b.bytes-a.bytes || (a.file < b.file ? -1 : 1)).slice(0,10) } };
}
