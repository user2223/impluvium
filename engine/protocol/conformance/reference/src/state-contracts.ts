// SPDX-License-Identifier: 0BSD
import { ticketSemantics } from './documents.ts';
import { Rejection } from './profile.ts';
export function publication(input: any): any {
  const versions = new Map<string, any>(input.published.map((row: any) => [row.versionId, row]));
  versions.set(input.next.versionId, input.next);
  function closure(root: string): { ids: Set<string>; missing: boolean; cycle: boolean } {
    const ids = new Set<string>(), active = new Set<string>(); let missing = false, cycle = false;
    function visit(id: string): void {
      if (active.has(id)) { cycle = true; return; }
      if (ids.has(id)) return;
      ids.add(id); const row = versions.get(id);
      if (!row) { missing = true; return; }
      active.add(id); for (const dependency of row.requires) visit(dependency); active.delete(id);
    }
    visit(root); return { ids, missing, cycle };
  }
  const affected = [...versions.keys()].filter(id => id !== input.next.versionId && closure(id).ids.has(input.next.versionId)).sort();
  for (const root of [input.next.versionId, ...affected]) {
    const result = closure(root);
    if (result.missing) return { reason: 'dependency_not_found', versionId: root };
    if (result.cycle) return { reason: 'dependency_cycle', versionId: root };
    const manifests = new Set([...result.ids].map(id => versions.get(id).manifestIdHex));
    if (manifests.size > 8) return { reason: 'ticket_set_too_large', versionId: root };
  }
  return { accepted: true, checkedVersions: [input.next.versionId, ...affected] };
}
export function ticketCacheModel(input: any): any {
  let stored: any; const results = [];
  for (const request of input.requests) {
    const match = stored && stored.ticketHex === request.ticketHex && stored.role === request.role && stored.generationU64 === request.generationU64;
    const notExpired = stored && BigInt(request.context.now.seconds) * 1000000000n + BigInt(request.context.now.nanos) < BigInt(stored.claims.exp.seconds) * 1000000000n + BigInt(stored.claims.exp.nanos);
    const hit = !!match && !!notExpired;
    if (!hit && !request.freshVerificationValid) { results.push({ hit: false, reason: request.freshReason }); continue; }
    const claims = hit ? stored.claims : input.claims;
    try {
      ticketSemantics(claims, request.validKeys, request.context);
      if (!hit) stored = { ticketHex: request.ticketHex, role: request.role, generationU64: request.generationU64, claims: structuredClone(claims) };
      results.push({ hit, accepted: true });
    } catch (error) { if (!(error instanceof Rejection)) throw error; results.push({ hit, reason: error.reason }); }
  }
  return { results };
}
export function sizePrecheck(input: any, stream: Iterable<Uint8Array>): any {
  const limits: Record<string, { size: bigint; reason: string;field?:string }> = {
    'manifest-envelope': { size: 536887296n, reason: 'envelope_too_large' },
    'descriptor-envelope': { size: 1064960n, reason: 'envelope_too_large' },
    'small-envelope': { size: 4096n, reason: 'envelope_too_large' },
    'rpc-request': { size: 65536n, reason: 'body_too_large' },
    'auth-success': { size: 65536n, reason: 'response_too_large' },
    'device-success': { size: 1048576n, reason: 'response_too_large' },
    'library-success': { size: 4194304n, reason: 'response_too_large' },
    'download-success': { size: 2097152n, reason: 'response_too_large' },
    'envelope-manifest-payload': {size:536870912n,reason:'byte_limit_exceeded',field:'Envelope.payload'},
    'envelope-descriptor-payload': {size:1048576n,reason:'byte_limit_exceeded',field:'Envelope.payload'}
  };
  const rule = limits[input.surface]; if (!rule) throw Error('Unbound size-precheck surface');
  let count = 0n;
  for (const chunk of stream) {
    count += BigInt(chunk.length);
    if (count > rule.size) return { sizeAccepted: false, reason: rule.reason, ...(rule.field?{field:rule.field}:{}),firstRejectedByteU64: (rule.size + 1n).toString() };
  }
  return { sizeAccepted: true, receivedU64: count.toString() };
}
export function u64AccumulatorProbe(input: {valuesU64:string[]}): any {
  let sum=0n;
  for(let index=0;index<input.valuesU64.length;index++){
    sum+=BigInt(input.valuesU64[index]);
    if(sum>18446744073709551615n)return {reason:'size_overflow',field:'',firstOverflowIndexU64:String(index)};
  }
  return {sumU64:sum.toString()};
}
export function acceptanceSetModel(input:any):any {
  const keys=new Map<string,any>(input.keys.map((key:any)=>[key.id,structuredClone(key)])),results:string[]=[];let generation=BigInt(input.generationU64);
  const step=BigInt(input.generationStepU64);if(step<1n)throw Error('invalid_model_generation_step');
  for(const operation of input.operations){
    const key=keys.get(operation.id);
    if(operation.kind==='hard-revoke'){if(!key)throw Error('unknown_model_key');keys.delete(operation.id);generation+=step;results.push('revoked');continue;}
    if(operation.kind==='remove'){
      if(!key||key.status!=='retiring')throw Error('invalid_retiring_model_key');
      if(BigInt(operation.nowU64)<BigInt(key.switchU64)+3600n){results.push('wait');continue;}
      keys.delete(operation.id);generation+=step;results.push('removed');continue;
    }
    if(operation.kind!=='switch'||keys.has(operation.id))throw Error('invalid_model_switch');
    if(keys.size===8){results.push('wait');continue;}
    const current=[...keys.values()].filter(key=>key.status==='current');if(current.length!==1)throw Error('invalid_current_model_key');
    current[0].status='retiring';current[0].switchU64=operation.nowU64;keys.set(operation.id,{id:operation.id,status:'current'});generation+=step;results.push('switched');
  }
  return{results,generationU64:String(generation),keys:[...keys.values()]};
}
