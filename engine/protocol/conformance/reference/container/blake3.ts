// SPDX-License-Identifier: 0BSD
// Container-only entry: never imported by the local standard-library stage graph.
import { deepStrictEqual } from 'node:assert';
import { readIndex, groupCases, loadAsset } from '../src/loader.ts';
import { fromHex } from '../src/primitives.ts';
import { verifyEnvelope, manifestSemantics } from '../src/documents.ts';
import { Rejection } from '../src/profile.ts';
import { responseDecision } from '../src/transport.ts';
import { reconstruct } from '../src/flows.ts';
export async function blake3Gates(blake3: (bytes: Uint8Array, options?: {dkLen: number}) => Uint8Array): Promise<{ id: string; result: 'pass' }[]> {
  const index = await readIndex();
  const results: { id: string; result: 'pass' }[] = [];
  for await (const entry of groupCases('primitives', index)) {
    if (entry.operation !== 'blake3') continue;
    const digest = Buffer.from(blake3(fromHex(entry.input.messageHex), { dkLen: 32 })).toString('hex');
    deepStrictEqual({ digestHex: digest }, entry.expected, entry.id);
    results.push({ id: entry.id, result: 'pass' });
  }
  const schema = await loadAsset('schema.json', index), unicode = await loadAsset('unicode16.json', index);
  for await (const entry of groupCases('signed', index)) {
    if (entry.operation !== 'blake3-signed-document') continue;
    let actual;
    try {
      const checked = verifyEnvelope(fromHex(entry.input.envelopeHex), 'manifest', entry.input.keys, schema);
      const warnings = manifestSemantics(checked.model, unicode, entry.input.context, bytes => blake3(bytes, { dkLen: 32 })).warnings;
      actual = { accepted: true, warnings, ...(warnings.length ? { warningFields: warnings.map(() => 'Entry.type') } : {}) };
    } catch (error) {
      if (!(error instanceof Rejection)) throw error;
      actual = { reason: error.reason, field: error.field };
    }
    deepStrictEqual(actual, entry.expected, entry.id);
    results.push({ id: entry.id, result: 'pass' });
  }
  for await(const entry of groupCases('behavior',index)){
    if(!['blake3-http-response','blake3-reconstruct'].includes(entry.operation))continue;
    const hash=(bytes:Uint8Array)=>blake3(bytes,{dkLen:32});
    const actual=entry.operation==='blake3-http-response'?responseDecision(entry.input,schema,unicode,hash):reconstruct(entry.input,schema,unicode,hash);
    deepStrictEqual(actual,entry.expected,entry.id);results.push({id:entry.id,result:'pass'});
  }
  if (results.length!==43) throw Error('Incomplete BLAKE3 run');
  return results;
}
