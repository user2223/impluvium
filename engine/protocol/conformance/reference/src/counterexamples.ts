// SPDX-License-Identifier: 0BSD
import { AssertionError, deepStrictEqual } from 'node:assert';
import { readIndex, loadAsset, groupCases } from './loader.ts';
import type { Case } from './loader.ts';
import { evaluateSignedCase } from './gates.ts';
import type { SignedOperations } from './gates.ts';
import { assertCaseConfirmation, provenanceGate, ConfirmationMismatch, ExpectedSnapshotMismatch } from './provenance.ts';
import { pae } from './primitives.ts';
import { verifyEnvelope, ticketSemantics, uuid } from './documents.ts';
import { reject } from './profile.ts';
import type { Schema } from './profile.ts';
import type { UnicodeMaps } from './paths.ts';

type Kind='pae'|'purpose'|'claims'|'expectation'|'priority'|'control';
type Definition={id:string;kind:Kind;rule:string;operation:string;witnesses:string[];violatesRule:boolean;explanation:string};
const definitions:Definition[]=[
  {id:'pae.body-length',kind:'pae',rule:'envelope.md 3',operation:'body-length-plus-one',witnesses:['pae.utf8-bytes','signed.manifest.golden'],violatesRule:true,explanation:'Advertise one additional body octet in the pre-authentication encoding.'},
  {id:'pae.character-length',kind:'pae',rule:'envelope.md 3',operation:'type-character-count',witnesses:['pae.utf8-bytes'],violatesRule:true,explanation:'Count Unicode characters instead of UTF-8 type octets.'},
  {id:'pae.version-marker',kind:'pae',rule:'envelope.md 3',operation:'wrong-pae-prefix',witnesses:['pae.ascii','signed.ticket.golden'],violatesRule:true,explanation:'Use a different DSSE version marker.'},
  {id:'purpose.manifest-descriptor',kind:'purpose',rule:'envelope.md 2; envelope.md 7 step 5',operation:'manifest-as-descriptor',witnesses:['signed.manifest.golden'],violatesRule:true,explanation:'Dispatch a manifest to the descriptor context.'},
  {id:'purpose.ticket-manifest',kind:'purpose',rule:'envelope.md 2; envelope.md 7 step 5',operation:'ticket-as-manifest',witnesses:['signed.ticket.golden'],violatesRule:true,explanation:'Dispatch a depot ticket to the manifest context.'},
  {id:'purpose.pairing-session',kind:'purpose',rule:'envelope.md 2; envelope.md 7 step 5',operation:'pairing-as-session',witnesses:['signed.pairing.golden'],violatesRule:true,explanation:'Dispatch a pairing proof to the session-proof context.'},
  {id:'claims.audience',kind:'claims',rule:'tickets.md 4 step 7',operation:'ignore-audience-binding',witnesses:['signed.ticket.wrong-audience'],violatesRule:true,explanation:'Replace the caller audience inside the claim validator with the supplied claim.'},
  {id:'claims.subject',kind:'claims',rule:'tickets.md 4 step 11',operation:'ignore-subject-binding',witnesses:['signed.ticket.wrong-subject'],violatesRule:true,explanation:'Replace the caller subject inside the claim validator with the supplied claim.'},
  {id:'claims.scope',kind:'claims',rule:'tickets.md 4 step 12',operation:'ignore-manifest-scope',witnesses:['signed.ticket.scope'],violatesRule:true,explanation:'Replace the requested manifest inside the validator with the first allowed manifest.'},
  {id:'expectation.acceptance',kind:'expectation',rule:'FORMAT.md Behavioral counterexamples',operation:'change-acceptance-expectation',witnesses:['signed.manifest.golden'],violatesRule:true,explanation:'Change a fixed normative acceptance literal after its indexed data snapshot was checked.'},
  {id:'expectation.reason',kind:'expectation',rule:'FORMAT.md Behavioral counterexamples',operation:'change-reason-expectation',witnesses:['signed.ticket.wrong-audience'],violatesRule:true,explanation:'Change a fixed normative local reason after its indexed data snapshot was checked.'},
  {id:'expectation.confirmed-acceptance',kind:'expectation',rule:'FORMAT.md Value-bound independent confirmations',operation:'change-confirmed-acceptance',witnesses:['signed.ed25519.baseline'],violatesRule:true,explanation:'Change an independently confirmed acceptance value while retaining its value-bound confirmation.'},
  {id:'expectation.confirmed-reason',kind:'expectation',rule:'FORMAT.md Value-bound independent confirmations',operation:'change-confirmed-reason',witnesses:['witness.descriptor-invalid-key'],violatesRule:true,explanation:'Change an independently confirmed local reason while retaining its value-bound confirmation.'},
  {id:'priority.audience-expiry',kind:'priority',rule:'tickets.md 4 steps 3 and 6',operation:'audience-syntax-before-expiry',witnesses:['signed.ticket.expired-before-audience'],violatesRule:true,explanation:'Move audience syntax before expiry. This witness intentionally has both defects; it tests the first reason.'},
  {id:'priority.expiry-window',kind:'priority',rule:'tickets.md 4 steps 2 and 3',operation:'expiry-before-window',witnesses:['signed.ticket.zero-window'],violatesRule:true,explanation:'Move the expiry check before lifetime validation.'},
  {id:'control.identical',kind:'control',rule:'envelope.md 3',operation:'identical-replacement',witnesses:['pae.utf8-bytes','signed.manifest.golden'],violatesRule:false,explanation:'An identical operation replacement preserves all results.'},
  {id:'control.comment',kind:'control',rule:'envelope.md 3',operation:'comment-only',witnesses:['pae.utf8-bytes'],violatesRule:false,explanation:'An explanatory source comment changes no operation.'},
  {id:'control.metadata',kind:'control',rule:'FORMAT.md Corpus shards',operation:'metadata-only',witnesses:['signed.manifest.golden'],violatesRule:false,explanation:'An informational case-rule annotation changes no input or expectation.'},
  {id:'control.unknown-envelope',kind:'control',rule:'encoding.md 2.5; envelope.md 6',operation:'unknown-envelope-field',witnesses:['signed.manifest.golden'],violatesRule:false,explanation:'Append a permitted unknown outer Envelope field. Signed payload bytes are unchanged.'}
];
type WitnessResult={id:string;executed:boolean;targetExecuted:boolean;path:string[];outcome:'pass'|'comparison-failure'|'confirmation-failure'|'expectation-failure'|'invalid-execution';failure?:string;actual?:unknown;expected:unknown};
type MutationResult=Definition&{verdict:'detected'|'ineffective'|'survived'|'invalid-execution';survived:boolean;requiredTestFailure:string;witnessResults:WitnessResult[]};
export type CounterexampleSuite={format:string;baseline:{passed:boolean;witnesses:WitnessResult[]};mutants:MutationResult[];counts:{mutants:number;detected:number;ineffective:number;survived:number;invalidExecution:number};byKind:Record<string,{total:number;detected:number;ineffective:number;survived:number;invalidExecution:number}>;passed:boolean};
function identicalPae(type:Uint8Array,body:Uint8Array):Buffer{return pae(type,body);}
function commentedPae(type:Uint8Array,body:Uint8Array):Buffer{
  // A descriptive comment does not alter the pre-authentication operation.
  return pae(type,body);
}
function operations(definition:Definition,entry:Case,path:string[]):Partial<SignedOperations>{
  const hit=():void=>{path.push('mutation/'+definition.operation);},op=definition.operation;
  if(['body-length-plus-one','type-character-count','wrong-pae-prefix','identical-replacement','comment-only'].includes(op))return{pae:(type,body)=>{
    hit();if(op==='identical-replacement')return identicalPae(type,body);if(op==='comment-only')return commentedPae(type,body);
    const prefix=op==='wrong-pae-prefix'?'DSSEv0':'DSSEv1',typeLength=op==='type-character-count'?[...Buffer.from(type).toString('utf8')].length:type.length,bodyLength=body.length+(op==='body-length-plus-one'?1:0);
    return Buffer.concat([Buffer.from(prefix+' '+typeLength+' '),Buffer.from(type),Buffer.from(' '+bodyLength+' '),Buffer.from(body)]);
  }};
  if(['manifest-as-descriptor','ticket-as-manifest','pairing-as-session'].includes(op))return{selectPurpose:()=>{hit();return op==='manifest-as-descriptor'?'descriptor':op==='ticket-as-manifest'?'manifest':'session';}};
  if(['ignore-audience-binding','ignore-subject-binding','ignore-manifest-scope','audience-syntax-before-expiry','expiry-before-window'].includes(op))return{ticketSemantics:(model,keys,context)=>{
    hit();if(op==='audience-syntax-before-expiry')uuid(model.aud,'DepotTicketClaims.aud');
    if(op==='expiry-before-window'){const ns=(t:any):bigint=>BigInt(t.seconds)*1000000000n+BigInt(t.nanos);if(ns(context.now)>=ns(model.exp))reject('ticket_expired','DepotTicketClaims.exp');}
    const replaced=op==='ignore-audience-binding'?{...context,audience:model.aud}:op==='ignore-subject-binding'?{...context,subject:model.sub}:op==='ignore-manifest-scope'?{...context,manifestIdHex:model.manifest_ids[0]}:context;
    ticketSemantics(model,keys,replaced);
  }};
  if(op==='unknown-envelope-field')return{verifyEnvelope:(bytes,purpose,keys,schema,encodePae)=>{hit();return verifyEnvelope(Buffer.concat([Buffer.from(bytes),Buffer.from('980100','hex')]),purpose,keys,schema,encodePae);}};
  hit();if(op==='metadata-only'){entry.rule+=' Informational annotation.';return{};}
  if(['change-acceptance-expectation','change-confirmed-acceptance'].includes(op)){if(typeof entry.expected.accepted!=='boolean')throw Error('invalid_mutation_witness');entry.expected.accepted=!entry.expected.accepted;return{};}
  if(['change-reason-expectation','change-confirmed-reason'].includes(op)){if(typeof entry.expected.reason!=='string')throw Error('invalid_mutation_witness');entry.expected.reason='ticket_expired';return{};}
  throw Error('unknown_mutation_operator');
}
function runWitness(original:Case,schema:Schema,unicode:UnicodeMaps,definition?:Definition):WitnessResult{
  const entry=structuredClone(original),path:string[]=[],result:WitnessResult={id:entry.id,executed:true,targetExecuted:!definition,path,outcome:'invalid-execution',expected:original.expected};
  try{
    const supplied=definition?operations(definition,entry,path):{};path.push('provenance.ts/assertCaseConfirmation');assertCaseConfirmation(entry);
    result.actual=evaluateSignedCase(entry,schema,unicode,{...supplied,trace:point=>path.push(point)});
    path.push('node:assert/deepStrictEqual');deepStrictEqual(result.actual,entry.expected,entry.id);result.outcome='pass';
  }catch(error){
    if(error instanceof AssertionError&&error.code==='ERR_ASSERTION'&&path.includes('node:assert/deepStrictEqual')){result.outcome='comparison-failure';result.failure='ERR_ASSERTION:actual-versus-fixed-expectation';}
    else if(definition?.kind==='expectation'&&error instanceof ConfirmationMismatch&&path.includes('provenance.ts/assertCaseConfirmation')){result.outcome='confirmation-failure';result.failure=error.message;}
    else if(definition?.kind==='expectation'&&error instanceof ExpectedSnapshotMismatch&&path.includes('provenance.ts/assertCaseConfirmation')){result.outcome='expectation-failure';result.failure=error.message;}
    else{result.outcome='invalid-execution';result.failure=error instanceof Error?error.name+': '+error.message:String(error);}
  }
  result.targetExecuted=!definition||path.includes('mutation/'+definition.operation);return result;
}
export async function counterexampleGates():Promise<CounterexampleSuite>{
  const confirmations=await provenanceGate();if(confirmations.evidence.pending)throw Error('counterexample_confirmation_prerequisite');
  const index=await readIndex();if(index.blocked.length)throw Error('counterexample_specification_prerequisite');
  const schema=await loadAsset('schema.json',index),unicode=await loadAsset('unicode16.json',index),wanted=new Set(definitions.flatMap(row=>row.witnesses)),cases=new Map<string,Case>();
  for(const group of['pae','signed'])for await(const entry of groupCases(group,index))if(wanted.has(entry.id))cases.set(entry.id,entry);
  if(!definitions.length||cases.size!==wanted.size)throw Error('empty_or_missing_mutation_witnesses');
  const baseline=[...cases.values()].map(entry=>runWitness(entry,schema,unicode)),baselinePassed=baseline.every(row=>row.outcome==='pass');
  if(!baselinePassed)throw Error('counterexample_baseline_failed:'+baseline.filter(row=>row.outcome!=='pass').map(row=>row.id+':'+row.failure).join(','));
  const mutants:MutationResult[]=definitions.map(definition=>{
    const witnessResults=definition.witnesses.map(id=>runWitness(cases.get(id)!,schema,unicode,definition)),invalid=witnessResults.some(row=>!row.executed||!row.targetExecuted||row.outcome==='invalid-execution');
    const failure=definition.kind==='expectation'?(definition.operation.includes('confirmed')?'confirmation-failure':'expectation-failure'):'comparison-failure';
    const detected=witnessResults.some(row=>row.outcome===failure),ineffective=witnessResults.every(row=>row.outcome==='pass');
    const verdict:MutationResult['verdict']=invalid?'invalid-execution':definition.violatesRule?(detected?'detected':'survived'):(ineffective?'ineffective':'invalid-execution');
    return{...definition,verdict,survived:verdict==='survived'||verdict==='ineffective',requiredTestFailure:definition.violatesRule?(definition.kind==='expectation'?(definition.operation.includes('confirmed')?'confirmation_value_mismatch':'expectation_snapshot_mismatch'):'ERR_ASSERTION:actual-versus-fixed-expectation'):'none:equivalent-or-normatively-ignored',witnessResults};
  });
  const counts={mutants:mutants.length,detected:mutants.filter(r=>r.verdict==='detected').length,ineffective:mutants.filter(r=>r.verdict==='ineffective').length,survived:mutants.filter(r=>r.verdict==='survived').length,invalidExecution:mutants.filter(r=>r.verdict==='invalid-execution').length},byKind:CounterexampleSuite['byKind']={};
  for(const row of mutants){const group=byKind[row.kind]??={total:0,detected:0,ineffective:0,survived:0,invalidExecution:0};group.total++;if(row.verdict==='invalid-execution')group.invalidExecution++;else group[row.verdict]++;}
  return{format:'behavioral-counterexamples-v1',baseline:{passed:baselinePassed,witnesses:baseline},mutants,counts,byKind,passed:baselinePassed&&counts.survived===0&&counts.invalidExecution===0&&counts.detected>0&&counts.ineffective===4};
}
