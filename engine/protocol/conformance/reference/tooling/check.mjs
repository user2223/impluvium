// SPDX-License-Identifier: 0BSD
import{writeReport}from'./output.mjs';
import{primitiveGates,fastcdcGates,signedGates,profileGates,behaviorGates}from'../src/gates.ts';
import{readIndex,loadCorpus,loadAsset,recipeBook}from'../src/loader.ts';
import{deliverySizeGate}from'../src/format-gates.ts';
import{publicationGate}from'../src/publication.ts';
import{provenanceGate,attachCaseAcceptance}from'../src/provenance.ts';
import{importInventory}from'./imports.mjs';
import{parseOptions}from'./options.mjs';
import{counterexampleGates}from'../src/counterexamples.ts';
import{coverageGate}from'../src/coverage.ts';
const command=process.argv[2];
if(!['imports','corpus','coverage','primitives','vectors','limits','counterexamples'].includes(command))throw Error('Unknown contract command');
const{out,stage}=parseOptions(process.argv.slice(3),command);
let report;
try{
 const common=[await deliverySizeGate(),await publicationGate(),await provenanceGate()];
 const calculation=common[2];
 if(command==='imports')report={...await importInventory(),checks:common};
 else if(command==='corpus'||command==='coverage'){
  const index=await readIndex(),groups=[];
  for(const entry of index.groups){const corpus=await loadCorpus(entry.file,entry.sha256);if(corpus.cases.length!==entry.count)throw Error('Indexed case count mismatch');groups.push({file:entry.file,count:entry.count,classifications:[...new Set(corpus.cases.map(row=>row.classification??'known-answer'))]});}
  for(const asset of index.assets)await loadAsset(asset.file,index);
  const book=await recipeBook(index);for(const id of book.nodes.keys())book.verify(id);
  report={format:'corpus-contract-v2',groups,specificationConditions:index.blocked,cases:groups.reduce((sum,row)=>sum+row.count,0),verifiedAssets:index.assets.length,checks:common,expectationsAccepted:calculation.evidence.expectationsAccepted};
  if(command==='coverage'){const execution=[...await primitiveGates(),...await fastcdcGates(),...await signedGates(),...await profileGates(),...await behaviorGates()];report={...report,...await coverageGate(execution,calculation.evidence.pending)};if(!report.complete)process.exitCode=1;}
 }else if(command==='counterexamples'){
  const suite=await counterexampleGates();report={format:'counterexample-contract-v1',status:!suite.passed?'failed':calculation.evidence.pending?'checks-pass-with-pending-requirements':'checks-pass',checks:common,counterexamples:suite,executedMutations:suite.counts.mutants,counts:{pass:common.filter(row=>row.result==='pass').length+Number(suite.baseline.passed)+suite.mutants.filter(row=>['detected','ineffective'].includes(row.verdict)).length,pending:common.filter(row=>row.result==='pending').length,fail:suite.mutants.filter(row=>!['detected','ineffective'].includes(row.verdict)).length},expectationsAccepted:calculation.evidence.expectationsAccepted};if(!suite.passed)process.exitCode=1;
 }else{
  const selected=command==='primitives'?await primitiveGates():command==='limits'?await profileGates():stage===4?[...await fastcdcGates(),...await signedGates()]:[...await primitiveGates(),...await fastcdcGates(),...await signedGates(),...await profileGates(),...await behaviorGates()];
  if(!selected.some(row=>row.result==='pass'))throw Error('Empty executed contract');
  const results=[...common,...selected];attachCaseAcceptance(results);report={format:'executed-contract-v2',results,counts:{pass:results.filter(row=>row.result==='pass').length,pending:results.filter(row=>row.result==='pending').length},expectationsAccepted:calculation.evidence.expectationsAccepted};
 }
}catch(error){report={format:'contract-error-v1',status:'failed',error:error instanceof Error?error.stack:String(error)};process.exitCode=1;}
await writeReport(out,command+(stage?'-stage-'+stage:''),report);
console.log(JSON.stringify({command,...(report.counts?{counts:report.counts}:{}),...(report.cases?{cases:report.cases}:{}),...(report.graphs?{localModules:report.graphs.local.modules.length,containerModules:report.graphs.container.modules.length}:{}),...(report.complete!==undefined?{complete:report.complete}:{}),...(report.error?{error:report.error}:{}),...(report.status?{status:report.status}:{})}));
