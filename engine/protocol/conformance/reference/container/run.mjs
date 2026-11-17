// SPDX-License-Identifier: 0BSD
import { blake3 } from '@noble/hashes/blake3';
import { writeReport } from '../tooling/output.mjs';
import { blake3Gates } from './blake3.ts';
import { primitiveGates, fastcdcGates, signedGates, profileGates, behaviorGates } from '../src/gates.ts';
import { deliverySizeGate } from '../src/format-gates.ts';
import { publicationGate } from '../src/publication.ts';
import { provenanceGate, attachCaseAcceptance } from '../src/provenance.ts';
import { parseOptions } from '../tooling/options.mjs';
const {out}=parseOptions(process.argv.slice(2),'container');
if (!/^v24\./.test(process.version)) throw Error('Container requires the pinned Node 24 runtime');
const results = [await deliverySizeGate(), await publicationGate(), await provenanceGate(), ...await primitiveGates(), ...await fastcdcGates(), ...await signedGates(), ...await profileGates(), ...await behaviorGates()];
const foreign = await blake3Gates(blake3);
const foreignIds=new Set(foreign.map(item=>item.id));
const reconciled=[...results.filter(item=>!foreignIds.has(item.id)),...foreign];
attachCaseAcceptance(reconciled);
const report = { format: 'container-gates-v1', runtime: process.version,
  dependency: { module: '@noble/hashes/blake3' },
  results:reconciled, counts:{pass:reconciled.filter(item=>item.result==='pass').length,pending:reconciled.filter(item=>item.result==='pending').length}, blake3: foreign, expectationsAccepted:reconciled.every(item=>item.result==='pass'),
  acceptanceRequirements: ['installation artifact and license validation', 'resolved import scanner', 'tsc --noEmit', 'confirmed independent calculations', 'complete specification conditions'] };
await writeReport(out,'container',report);
console.log(JSON.stringify({ runtime: process.version, executed: reconciled.filter(item => item.result === 'pass').length, blake3: foreign.length, expectationsAccepted:report.expectationsAccepted }));
