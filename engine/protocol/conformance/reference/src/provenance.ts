// SPDX-License-Identifier: 0BSD
import { readIndex, loadAsset, loadCorpus, recipeBook, readPackageText } from './loader.ts';
import type { Case } from './loader.ts';
import { materialize } from './recipes.ts';
import type { RecipeBook } from './recipes.ts';
import { scanBinary } from './profile.ts';
import { fromHex, pae } from './primitives.ts';
import { throws } from 'node:assert';
import { createHash } from 'node:crypto';
type FieldRecord = { fieldId:string; rule:string; method:string; sources:string[]; secondCalculation:{method:string;result:'pending'|'confirmed';checks?:number;valueSha256?:string} };
const caseReview = new Map<string,{fields:number;pending:number}>(), boundFields = new Map<string,FieldRecord>();
const expectedSnapshots=new Map<string,string>();
let activeBook: RecipeBook | undefined;
export class ConfirmationMismatch extends Error {
  fieldId:string;
  constructor(fieldId:string){super('confirmation_value_mismatch:'+fieldId);this.name='ConfirmationMismatch';this.fieldId=fieldId;}
}
export class ExpectedSnapshotMismatch extends Error {
  constructor(caseId:string){super('expectation_snapshot_mismatch:'+caseId);this.name='ExpectedSnapshotMismatch';}
}
export function canonicalValue(value:any):string {
  if(Array.isArray(value))return '['+value.map(canonicalValue).join(',')+']';
  if(value!==null&&typeof value==='object')return '{'+Object.keys(value).sort().map(key=>JSON.stringify(key)+':'+canonicalValue(value[key])).join(',')+'}';
  if(value!==null&&!['string','number','boolean'].includes(typeof value)||typeof value==='number'&&!Number.isSafeInteger(value))throw Error('invalid_confirmation_value');
  const text=JSON.stringify(value);if(text===undefined)throw Error('invalid_confirmation_value');return text;
}
export function confirmationDigest(fieldId:string,value:any):string {
  return createHash('sha256').update('reference-confirmation-v1\n'+JSON.stringify(fieldId)+'\n'+canonicalValue(value)).digest('hex');
}
function pointer(value:any,path:string):any {
  if(!/^\/(?:input|expected)(?:\/|$)/.test(path))throw Error('invalid_confirmation_reference');
  for(const raw of path.slice(1).split('/')){if(/~(?![01])/.test(raw))throw Error('invalid_confirmation_reference');const key=raw.replace(/~1/g,'/').replace(/~0/g,'~');if(!value||typeof value!=='object'||!Object.hasOwn(value,key))throw Error('missing_confirmation_projection');value=value[key];}return value;
}
function checkValue(fieldId:string,value:any,fields:Map<string,FieldRecord>):void {
  const record=fields.get(fieldId);if(!record)throw Error('missing_calculation_field:'+fieldId);
  if(record.secondCalculation.result==='confirmed'&&confirmationDigest(fieldId,value)!==record.secondCalculation.valueSha256)throw new ConfirmationMismatch(fieldId);
}
export function assertCaseConfirmation(entry:Case):void {
  if(!activeBook||!boundFields.size)throw Error('confirmation_gate_not_executed');
  for(const[path,origin]of Object.entries(entry.provenance))if(origin.kind==='calculated')checkValue(origin.fieldId,materialize(pointer(entry,path),activeBook),boundFields);
  const snapshot=expectedSnapshots.get(entry.id);if(snapshot===undefined||snapshot!==canonicalValue(entry.expected))throw new ExpectedSnapshotMismatch(entry.id);
}
export function attachCaseAcceptance<T extends{id:string;result:string}>(results:T[]):void {
  for(const result of results){const review=caseReview.get(result.id);if(review)Object.assign(result,{expectationReview:{...review,accepted:review.pending===0}});}
}
export function assertConfirmedCase(id:string):void {
  const review=caseReview.get(id);if(!review||review.pending)throw Error('counterexample_witness_confirmation_prerequisite:'+id);
}
export function validateProvenance(data:any):void {
  if(data?.format!=='field-calculations-v1'||!data.sources||!data.rules||!data.methods||!Array.isArray(data.fields)||!data.fields.length)throw Error('invalid_calculation_metadata');
  for(const source of Object.values(data.sources) as any[])if(typeof source.document!=='string'||!source.document||!Array.isArray(source.sections)||!source.sections.length||source.sections.some((s:any)=>typeof s!=='string'||!s)||!/^[a-f0-9]{64}$/.test(source.sha256))throw Error('invalid_calculation_source');
  for(const method of Object.values(data.methods))if(typeof method!=='string'||!method.endsWith('.')||/[\r\n]/.test(method as string))throw Error('invalid_calculation_method');
  for(const rule of Object.values(data.rules) as any[])if(typeof rule.text!=='string'||!rule.text||!Array.isArray(rule.sources)||!rule.sources.length||rule.sources.some((id:string)=>!Object.hasOwn(data.sources,id)))throw Error('invalid_calculation_rule');
  const ids=new Set<string>();
  for(const field of data.fields as FieldRecord[]){
    if(typeof field.fieldId!=='string'||!field.fieldId||ids.has(field.fieldId)||!Object.hasOwn(data.rules,field.rule)||!Object.hasOwn(data.methods,field.method)||!Array.isArray(field.sources)||!field.sources.length||field.sources.some(id=>!Object.hasOwn(data.sources,id)))throw Error('invalid_calculation_field');
    ids.add(field.fieldId);const second=field.secondCalculation;
    if(!second||!['pending','confirmed'].includes(second.result)||typeof second.method!=='string')throw Error('invalid_independent_calculation');
    if(second.result==='pending'?(second.method!==''||second.checks!==undefined||second.valueSha256!==undefined):(second.method!=='independent'||![1,3].includes(second.checks??0)||!/^[a-f0-9]{64}$/.test(second.valueSha256??'')))throw Error('invalid_independent_calculation');
  }
}
export async function provenanceGate():Promise<{id:string;result:'pass'|'pending';path:string;evidence:{fields:number;confirmed:number;pending:number;withoutExpectedResults:number;expectationsAccepted:boolean}}> {
  const index=await readIndex(),fields=new Map<string,FieldRecord>(),assets=index.assets.filter(row=>/^provenance-[0-9]{3}\.json$/.test(row.file));
  if(!assets.length)throw Error('missing_calculation_metadata');
  const formatDigest=createHash('sha256').update(await readPackageText('FORMAT.md')).digest('hex');
  for(const asset of assets){const data=await loadAsset(asset.file,index);validateProvenance(data);for(const source of Object.values(data.sources) as any[])if(source.document==='conformance/reference/FORMAT.md'&&source.sha256!==formatDigest)throw Error('calculation_source_integrity');for(const field of data.fields){if(fields.has(field.fieldId))throw Error('duplicate_calculation_field');fields.set(field.fieldId,field);}}
  const book=await recipeBook(index),cases=new Map<string,Case>(),seen=new Map<string,string>();
  const register=(fieldId:string,value:any):void=>{const digest=confirmationDigest(fieldId,value);if(seen.has(fieldId)&&seen.get(fieldId)!==digest)throw Error('conflicting_calculation_value:'+fieldId);seen.set(fieldId,digest);checkValue(fieldId,value,fields);};
  for(const group of index.groups)for(const entry of(await loadCorpus(group.file,group.sha256)).cases){if(cases.has(entry.id))throw Error('duplicate_case_id');cases.set(entry.id,entry);for(const[path,origin]of Object.entries(entry.provenance))if(origin.kind==='calculated')register(origin.fieldId,materialize(pointer(entry,path),book));}
  for(const node of book.nodes.values())for(const property of ['byteLengthU64','sha256Hex'])register('recipe.'+node.id+'.'+property,node[property]);
  const constructionAssets=index.assets.filter(row=>/^construction-values-[0-9]{3}\.json$/.test(row.file)),schema=await loadAsset('schema.json',index);
  for(const asset of constructionAssets){const data=await loadAsset(asset.file,index);if(data.format!=='construction-confirmation-values-v1'||!Array.isArray(data.fields)||data.fields.length>1024)throw Error('invalid_construction_values');
    for(const row of data.fields){if(typeof row.fieldId!=='string'||seen.has(row.fieldId)||Object.keys(row).some(key=>!['fieldId','result','valueRef'].includes(key))||Object.hasOwn(row,'result')===Object.hasOwn(row,'valueRef'))throw Error('invalid_construction_value');let value=row.result;
      if(row.valueRef){const ref=row.valueRef;if(!ref||typeof ref.caseId!=='string'||typeof ref.path!=='string'||Object.keys(ref).some(key=>!['caseId','path','projection'].includes(key))||!cases.has(ref.caseId))throw Error('invalid_confirmation_reference');value=materialize(pointer(cases.get(ref.caseId),ref.path),book);
        if(ref.projection!==undefined){if(!['envelope-payload','envelope-pae'].includes(ref.projection)||typeof value!=='string')throw Error('invalid_confirmation_projection');const envelope=scanBinary(fromHex(value),'Envelope',schema,{limit:1064960});value=ref.projection==='envelope-payload'?envelope.payload:pae(Buffer.from(envelope.payload_type),fromHex(envelope.payload)).toString('hex');}
      }register(row.fieldId,value);
    }
  }
  for(const fieldId of fields.keys())if(!seen.has(fieldId))throw Error('missing_confirmation_value:'+fieldId);
  caseReview.clear();for(const entry of cases.values()){
    const dependencies=new Set<string>(),visited=new Set<string>();for(const origin of Object.values(entry.provenance))if(origin.kind==='calculated')dependencies.add(origin.fieldId);
    function recipe(id:string):void{if(visited.has(id))return;visited.add(id);const node=book.nodes.get(id);if(!node)throw Error('missing_recipe_calculation');for(const property of ['byteLengthU64','sha256Hex'])dependencies.add('recipe.'+id+'.'+property);for(const child of node.parts??[])recipe(child);if(node.source)recipe(node.source);if(node.insert)recipe(node.insert);}
    function bytes(value:any):void{if(value&&typeof value==='object'){if(value.$bytes)recipe(value.$bytes);else if(value.$utf8)recipe(value.$utf8);else Object.values(value).forEach(bytes);}}
    bytes(entry.input);bytes(entry.expected);if(entry.input.streamRecipe)recipe(entry.input.streamRecipe);
    caseReview.set(entry.id,{fields:dependencies.size,pending:[...dependencies].filter(id=>fields.get(id)!.secondCalculation.result==='pending').length});
  }
  boundFields.clear();for(const[id,record]of fields)boundFields.set(id,record);activeBook=book;
  expectedSnapshots.clear();for(const[id,entry]of cases)expectedSnapshots.set(id,canonicalValue(materialize(entry.expected,book)));
  const sample=await loadAsset(assets[0].file,index),missingMethod=structuredClone(sample);delete missingMethod.fields[0].method;throws(()=>validateProvenance(missingMethod),/invalid_calculation_field/);
  const falseConfirmation=structuredClone(sample);falseConfirmation.fields[0].secondCalculation={method:'',result:'confirmed'};throws(()=>validateProvenance(falseConfirmation),/invalid_independent_calculation/);
  const invalidSource=structuredClone(sample);Object.values(invalidSource.sources as Record<string,any>)[0].sha256='00';throws(()=>validateProvenance(invalidSource),/invalid_calculation_source/);
  const confirmed=[...fields.values()].filter(row=>row.secondCalculation.result==='confirmed').length,pending=fields.size-confirmed,withoutExpectedResults=[...fields.values()].filter(row=>row.secondCalculation.checks===3).length;
  return{id:'provenance.independent-calculations',result:pending?'pending':'pass',path:'src/provenance.ts',evidence:{fields:fields.size,confirmed,pending,withoutExpectedResults,expectationsAccepted:pending===0}};
}
