// SPDX-License-Identifier: 0BSD
import{writeReport}from'./output.mjs';
import{createHash}from'node:crypto';
import{primitiveGates,fastcdcGates,signedGates,profileGates,behaviorGates}from'../src/gates.ts';
import{deliverySizeGate}from'../src/format-gates.ts';
import{publicationGate}from'../src/publication.ts';
import{provenanceGate,attachCaseAcceptance}from'../src/provenance.ts';
import{readDataBytes}from'../src/loader.ts';
import{counterexampleGates}from'../src/counterexamples.ts';
import{parseOptions}from'./options.mjs';
const{out,stage}=parseOptions(process.argv.slice(2),'stage');
const report={format:'stage-report-v2',stage,runtime:process.version,
 corpusIndexSha256:createHash('sha256').update(await readDataBytes('INDEX.json')).digest('hex'),
 results:[],counts:{},expectationsAccepted:false,status:'running'};
try{
  {
    const common=[await deliverySizeGate(),await publicationGate(),await provenanceGate()];
    if(stage===6){const suite=await counterexampleGates();report.counterexamples=suite;report.results=[...common,{id:'counterexamples.baseline',result:suite.baseline.passed?'pass':'fail',path:'src/counterexamples.ts'},...suite.mutants.map(item=>({id:item.id,result:['detected','ineffective'].includes(item.verdict)?'pass':'fail',path:'src/counterexamples.ts',evidence:item}))];}
    else report.results=stage===3?[...common,...await primitiveGates()]:stage===4?[...common,...await fastcdcGates(),...await signedGates()]:[...common,...await profileGates(),...await behaviorGates()];
    attachCaseAcceptance(report.results);
    const pass=report.results.filter(row=>row.result==='pass').length,pending=report.results.filter(row=>row.result==='pending').length,fail=report.results.filter(row=>row.result==='fail').length;
    if(!pass)throw Error('Empty executed stage');
    report.counts={pass,pending,fail};report.status=fail?'failed':pending?'checks-pass-with-pending-requirements':'checks-pass';report.expectationsAccepted=common[2].evidence.expectationsAccepted;if(fail)process.exitCode=1;
  }
}catch(error){report.status='failed';report.error=error instanceof Error?error.stack:String(error);process.exitCode=1;}
await writeReport(out,'stage-'+stage,report);
console.log(JSON.stringify({stage,status:report.status,counts:report.counts,expectationsAccepted:report.expectationsAccepted,...(report.error?{error:report.error}:{})}));
