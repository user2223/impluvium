// SPDX-License-Identifier: 0BSD
import { deepStrictEqual, throws } from 'node:assert';
import { fromHex, digest, hkdfSha256, ed25519KnownAnswer, ed25519Verify, seedPublicKey, pae, ed25519EncodingSubstage } from './primitives.ts';
import { validateCorpus, loadCorpus, loadCorpusBytes, readIndex, loadAsset, groupCases, recipeBook } from './loader.ts';
import { fastcdc } from './fastcdc.ts';
import type { CutWitness } from './fastcdc.ts';
import { Rejection, scanBinary } from './profile.ts';
import { verifyEnvelope, manifestSemantics, descriptorSemantics, descriptorContext, ticketSemantics, proofSemantics, descriptorCommandBudget } from './documents.ts';
import type { Purpose } from './documents.ts';
import { scanJson } from './json-profile.ts';
import { genericFields, genericContext, versionDecision, discoverySemantics, pageSize, normalizeUserCode } from './api.ts';
import { responseDecision, responseHeadSize, rangeSpan, transportDecision, redirectDecision } from './transport.ts';
import { sessionModel, pairingModel, libraryModel } from './models.ts';
import { receiveEvent, resumeDecision, openEventStream, supersessionKey, eventOpenModel } from './events.ts';
import { basePath, collisionKey, windowsTree } from './paths.ts';
import { downloadResponse, reconstruct, trustModel } from './flows.ts';
import { publication, ticketCacheModel, sizePrecheck, u64AccumulatorProbe, acceptanceSetModel } from './state-contracts.ts';
import { rpcExchange } from './rpc.ts';
import { serverGet } from './get-routes.ts';
import type { Case } from './loader.ts';
import { recipeFormatGates } from './format-gates.ts';
import { ed25519PointProfile, ed25519VerifyProfile } from './ed25519.ts';
import { authRpcFlow } from './auth-rpc.ts';
import { readDataBytes } from './loader.ts';
import { assertCaseConfirmation } from './provenance.ts';
import { createPrivateKey, sign } from 'node:crypto';
import type { Schema } from './profile.ts';
import type { UnicodeMaps } from './paths.ts';

export type GateResult = { id: string; result: 'pass' | 'pending'; path: string; detail?: string; evidence?: unknown };
export async function behaviorGates(): Promise<GateResult[]> {
  const index = await readIndex(), schema = await loadAsset('schema.json', index), unicode = await loadAsset('unicode16.json', index), results: GateResult[] = [];
  const book = await recipeBook(index);
  for await (const entry of groupCases('behavior', index)) {
    assertCaseConfirmation(entry);
    if(['blake3-http-response','blake3-reconstruct'].includes(entry.operation)){
      results.push({id:entry.id,result:'pending',path:'container/blake3.ts',detail:'Pinned BLAKE3 dependency required; no local import'});continue;
    }
    const input = entry.input; let actual: any;
    try {
      if (entry.operation === 'generic-api') {
        const model = scanJson(fromHex(input.rawJsonHex), input.message, schema);
        genericFields(input.message, model, schema); genericContext(input.message, model, input.context);
        actual = { accepted: true };
      } else if(entry.operation==='generic-api-codec'){
        const model=input.codec==='json'?scanJson(fromHex(input.rawJsonHex),input.message,schema):scanBinary(fromHex(input.wireHex),input.message,schema);
        genericFields(input.message,model,schema);genericContext(input.message,model,input.context);actual={accepted:true};
      } else if (entry.operation === 'version-decision') actual = { decision: versionDecision(input.server, input.client) };
      else if (entry.operation === 'range') {
        const span = rangeSpan(input.range, BigInt(input.sizeU64));
        actual = span ? { firstU64: span.first.toString(), lastU64: span.last.toString() } : { satisfiable: false };
      } else if (entry.operation === 'http-response') actual = responseDecision(input, schema, unicode);
      else if(entry.operation==='response-head-size')actual=responseHeadSize(input as {protocol:string;heads:{nameHex:string;valueHex:string}[][]});
      else if (entry.operation === 'transport') actual = transportDecision(input);
      else if (entry.operation === 'redirect') actual = redirectDecision(input);
      else if (entry.operation === 'session-model') actual = sessionModel(input);
      else if (entry.operation === 'pairing-model') actual = pairingModel(input);
      else if (entry.operation === 'library-model') actual = libraryModel(input);
      else if (entry.operation === 'event-receive') actual = receiveEvent(input, schema);
      else if (entry.operation === 'resume') actual = { decision: resumeDecision(input) };
      else if (entry.operation === 'base-path') { basePath(fromHex(input.pathHex).toString('utf8'), input.targetOf === undefined ? '' : 'Entry.symlink_target', input.targetOf); actual = { accepted: true }; }
      else if (entry.operation === 'collision') actual = { collision: collisionKey(fromHex(input.leftHex).toString('utf8'), unicode) === collisionKey(fromHex(input.rightHex).toString('utf8'), unicode) };
      else if (entry.operation === 'windows-tree') { const warnings = windowsTree(input.entries, unicode, input.variant); actual = { warnings, ...(warnings.length ? { warningFields: warnings.map(() => 'Entry.type') } : {}) }; }
      else if (entry.operation === 'discovery') { discoverySemantics(scanJson(fromHex(input.rawJsonHex), 'Discovery', schema), schema); actual = { accepted: true }; }
      else if (entry.operation === 'download-response') { downloadResponse(scanJson(fromHex(input.rawJsonHex), 'OpenDownloadResponse', schema), input.context, schema, unicode); actual = { accepted: true }; }
      else if (entry.operation === 'reconstruct') actual = reconstruct(input, schema, unicode);
      else if (entry.operation === 'trust-model') actual = trustModel(input);
      else if (entry.operation === 'publication') actual = publication(input);
      else if (entry.operation === 'ticket-cache-model') actual = ticketCacheModel(input);
      else if (entry.operation === 'size-precheck') actual = sizePrecheck(input, book.chunks(input.streamRecipe));
      else if (entry.operation === 'rpc-exchange') actual = rpcExchange(input, schema, unicode);
      else if (entry.operation === 'event-open') actual = openEventStream(input);
      else if (entry.operation === 'event-open-model') actual = eventOpenModel(input);
      else if (entry.operation === 'supersession') { const left = supersessionKey(input.left); actual = { supersedes: left !== null && left === supersessionKey(input.right) }; }
      else if (entry.operation === 'server-get') actual = serverGet(input, schema);
      else if (entry.operation === 'auth-rpc-flow') actual = authRpcFlow(input,schema);
      else if (entry.operation === 'u64-accumulator-probe') actual = u64AccumulatorProbe(input as {valuesU64:string[]});
      else if (entry.operation === 'page-size') actual = { pageSize:pageSize(input.pageSize) };
      else if(entry.operation==='page-size-codec'){const model=input.codec==='json'?scanJson(fromHex(input.rawJsonHex),input.message,schema):scanBinary(fromHex(input.wireHex),input.message,schema);genericFields(input.message,model,schema);actual={pageSize:pageSize(model.page_size)};}
      else if (entry.operation === 'normalize-user-code') actual = { normalized:normalizeUserCode(fromHex(input.textHex).toString('utf8')) };
      else if(entry.operation==='user-code-input'){const normalized=normalizeUserCode(fromHex(input.textHex).toString('utf8'));actual={normalized,valid:/^[0-9A-HJKMNP-TV-Z]{8}$/.test(normalized)};}
      else if(entry.operation==='acceptance-set-model')actual=acceptanceSetModel(input);
      else if(entry.operation==='command-budget'){descriptorCommandBudget(input.model);actual={accepted:true};}
      else if(entry.operation==='domain-separation'){
        const leftType=fromHex(input.leftTypeHex),rightType=fromHex(input.rightTypeHex),body=fromHex(input.bodyHex),left=pae(leftType,body),right=pae(rightType,body),seed=fromHex(input.seedHex);
        const key=createPrivateKey({key:Buffer.concat([Buffer.from('302e020100300506032b657004220420','hex'),seed]),format:'der',type:'pkcs8'}),a=sign(null,left,key),b=sign(null,right,key);
        actual={equalTypeLengths:leftType.length===rightType.length,paeDifferent:!left.equals(right),signatureDifferent:!a.equals(b),crossVerification:ed25519Verify(seedPublicKey(seed),right,a)};
      }
      else throw Error('unhandled_behavior_operation:' + entry.operation);
    } catch (error) {
      if (!(error instanceof Rejection)) throw error;
      actual = { reason: error.reason, field: error.field };
    }
    deepStrictEqual(actual, entry.expected, entry.id);
    results.push({ id: entry.id, result: 'pass', path: 'src/' + entry.operation, detail: entry.classification });
  }
  for (const group of index.blocked.filter(group => !group.group.startsWith('primitives.') && !group.group.startsWith('signed.')))
    results.push({ id: group.group, result: 'pending', path: 'not-executed', detail: group.condition });
  return results;
}
export async function profileGates(): Promise<GateResult[]> {
  const index = await readIndex(), schema = await loadAsset('schema.json', index), results: GateResult[] = [];
  for (const name of ['profile', 'large-limits']) {
    for await (const entry of groupCases(name, index)) {
    assertCaseConfirmation(entry);
      let actual: any;
      try {
        if (entry.operation === 'scan-json') actual = { value: scanJson(fromHex(entry.input.rawJsonHex), entry.input.message, schema) };
        else if(entry.operation === 'json-precheck'){scanJson(fromHex(entry.input.rawJsonHex),entry.input.message,schema,entry.input.limit??65536,false);actual={accepted:true};}
        else if (entry.operation === 'scan-binary' || entry.operation === 'binary-precheck') {
          const value = scanBinary(fromHex(entry.input.wireHex), entry.input.message, schema, { limit: entry.input.limit, collect: entry.operation !== 'binary-precheck' });
          actual = entry.operation === 'binary-precheck' ? { accepted: true } : { value };
        } else throw Error('unhandled_profile_operation:' + entry.operation);
      } catch (error) {
        if (!(error instanceof Rejection)) throw error;
        actual = { reason: error.reason, field: error.field };
      }
      deepStrictEqual(actual, entry.expected, entry.id);
      results.push({ id: entry.id, result: 'pass', path: 'src/' + (['scan-json','json-precheck'].includes(entry.operation) ? 'json-profile.ts/scanJson' : 'profile.ts/scanBinary'), detail: entry.classification });
    }
  }
  return results;
}
export type SignedOperations = { pae:typeof pae; selectPurpose:(purpose:Purpose)=>Purpose; verifyEnvelope:typeof verifyEnvelope; ticketSemantics:typeof ticketSemantics; trace:(path:string)=>void; blake3?: (bytes:Uint8Array)=>Uint8Array };
export function evaluateSignedCase(entry:Case,schema:Schema,unicode:UnicodeMaps,overrides:Partial<SignedOperations>={}):any {
  const operations:SignedOperations={pae,selectPurpose:purpose=>purpose,verifyEnvelope,ticketSemantics,trace:()=>{},...overrides},input=entry.input;
  try {
    if(entry.operation==='derive-key'){
      operations.trace('primitives.ts/hkdfSha256');const derived=hkdfSha256(fromHex(input.ikmHex),fromHex(input.saltHex),fromHex(input.infoHex),input.length),key=seedPublicKey(fromHex(derived.okmHex));
      return{prkHex:derived.prkHex,seedHex:derived.okmHex,publicKeyHex:key.toString('hex'),keyIdHex:digest('sha256',key).subarray(0,8).toString('hex')};
    }
    if(entry.operation==='pae'){operations.trace('primitives.ts/pae');return{paeHex:operations.pae(fromHex(input.typeHex),fromHex(input.bodyHex)).toString('hex')};}
    if(!['signed-document','blake3-signed-document'].includes(entry.operation))throw Error('unhandled_signed_operation:'+entry.operation);
    const purpose=operations.selectPurpose(input.purpose as Purpose);operations.trace('documents.ts/verifyEnvelope/'+purpose);
    const checked=operations.verifyEnvelope(fromHex(input.envelopeHex),purpose,input.keys,schema,operations.pae);operations.trace('documents.ts/verified-envelope');let warnings:string[]=[];
    if(purpose==='manifest'){operations.trace('documents.ts/manifestSemantics');warnings=manifestSemantics(checked.model,unicode,input.context,operations.blake3).warnings;}
    else if(purpose==='descriptor'){
      operations.trace('documents.ts/descriptorSemantics');descriptorSemantics(checked.model,unicode);const bytes=fromHex(input.context.manifestPayloadHex),manifest=scanBinary(bytes,'Manifest',schema,{limit:536870912});manifestSemantics(manifest,unicode,{},operations.blake3);warnings=descriptorContext(checked.model,checked.payload,manifest,bytes,unicode,input.context);
    }else if(purpose==='ticket'){operations.trace('documents.ts/ticketSemantics');operations.ticketSemantics(checked.model,checked.validKeys,input.context);}
    else{operations.trace('documents.ts/proofSemantics');proofSemantics(checked.model,purpose,input.context);}
    return{accepted:true,warnings,...(warnings.length?{warningFields:warnings.map(()=>'Entry.type')}:{})};
  }catch(error){if(!(error instanceof Rejection))throw error;return{reason:error.reason,field:error.field};}
}
export async function signedGates(): Promise<GateResult[]> {
  const index = await readIndex(), schema = await loadAsset('schema.json', index), unicode = await loadAsset('unicode16.json', index);
  const results: GateResult[] = [];
  for (const name of ['keys', 'pae', 'signed']) {
    for await (const entry of groupCases(name, index)) {
    assertCaseConfirmation(entry);
      if (entry.operation === 'blake3-signed-document') {
        results.push({ id: entry.id, result: 'pending', path: 'container/blake3.ts', detail: 'Requires pinned BLAKE3 dependency in container; no transitive local import' });
        continue;
      }
      const actual = evaluateSignedCase(entry,schema,unicode);
      deepStrictEqual(actual, entry.expected, entry.id);
      results.push({ id: entry.id, result: 'pass', path: 'src/' + (entry.operation === 'signed-document' ? 'documents.ts/verifyEnvelope' : 'primitives.ts/' + entry.operation),
        detail: entry.classification });
    }
  }
  for (const blocked of index.blocked.filter(item => item.group.startsWith('signed.') && item.group !== 'signed.full-profile'))
    results.push({ id: blocked.group, result: 'pending', path: 'not-executed', detail: blocked.condition });
  return results;
}
export async function fastcdcGates(): Promise<GateResult[]> {
  const index = await readIndex();
  const constants = await loadAsset('fastcdc-constants.json', index);
  const results: GateResult[] = [];
  for (let value = 0; value < 256; value++) {
    const actual = BigInt('0x' + digest('md5', Buffer.alloc(64, value)).subarray(0, 8).toString('hex'));
    deepStrictEqual(actual.toString(), constants.gearU64[value], 'gear index ' + value);
    results.push({ id: 'fastcdc.gear-md5.' + value, result: 'pass', path: 'src/primitives.ts/md5-to-big-endian-u64' });
  }
  deepStrictEqual(BigInt(constants.maskSmallU64).toString(2).replace(/0/g, '').length, 19);
  deepStrictEqual(BigInt(constants.maskLargeU64).toString(2).replace(/0/g, '').length, 17);
  results.push({ id: 'fastcdc.mask-popcounts', result: 'pass', path: 'src/gates.ts/fastcdcGates' });
  for await (const entry of groupCases('fastcdc', index)) {
    assertCaseConfirmation(entry);
    if (entry.operation !== 'fastcdc') throw Error('unhandled_fastcdc_operation');
    const trace: CutWitness[] = [];
    deepStrictEqual({ chunks: fastcdc(fromHex(entry.input.messageHex), constants, trace) }, entry.expected, entry.id);
    results.push({ id: entry.id, result: 'pass', path: 'src/fastcdc.ts/pair-loop-no-seed', evidence: { appliedSeed: false, trace } });
  }
  return results;
}
export async function primitiveGates(): Promise<GateResult[]> {
  const index = await readIndex();
  const group = index.groups.find(item => item.name === 'primitives' || item.file === 'primitives.json');
  if (!group) throw Error('missing_primitive_group');
  const corpus = await loadCorpus(group.file, group.sha256);
  const results: GateResult[] = [];
  for await (const entry of groupCases('primitives', index)) {
    assertCaseConfirmation(entry);
    const input = entry.input;
    let actual;
    if (entry.operation === 'blake3') {
      results.push({ id: entry.id, result: 'pending', path: 'container/blake3.ts', detail: 'Pinned independent dependency and installation/license evidence required' });
      continue;
    }
    if (entry.operation === 'sha256' || entry.operation === 'sha512') actual = { digestHex: digest(entry.operation, fromHex(input.messageHex)).toString('hex') };
    else if (entry.operation === 'hkdf-sha256') actual = hkdfSha256(fromHex(input.ikmHex), fromHex(input.saltHex), fromHex(input.infoHex), input.length);
    else if (entry.operation === 'ed25519-kat') {
      actual = ed25519KnownAnswer(fromHex(input.seedHex), fromHex(input.messageHex));
      if (!ed25519Verify(fromHex(entry.expected.publicKeyHex), fromHex(input.messageHex), fromHex(entry.expected.signatureHex))) throw Error('known_answer_verify_failed:' + entry.id);
    } else if (entry.operation === 'ed25519-encoding-substage') actual = ed25519EncodingSubstage(fromHex(input.publicKeyHex), fromHex(input.signatureHex));
    else throw Error('unhandled_primitive:' + entry.operation);
    deepStrictEqual(actual, entry.expected, entry.id);
    results.push({ id: entry.id, result: 'pass', path: 'src/primitives.ts/' + entry.operation });
  }
  const sample = structuredClone(corpus);
  const provenancePath = Object.keys(sample.cases[0].provenance)[0];
  delete sample.cases[0].provenance[provenancePath];
  throws(() => validateCorpus(sample), /missing_provenance/);
  results.push({ id: 'loader.detect-missing-provenance', result: 'pass', path: 'src/loader.ts/validateCorpus' });
  const badHex = structuredClone(corpus);
  badHex.cases[0].input.messageHex = 'AA';
  throws(() => validateCorpus(badHex), /invalid_hex/);
  results.push({ id: 'loader.detect-uppercase-hex', result: 'pass', path: 'src/loader.ts/validateCorpus' });
  const badFormat = structuredClone(corpus);
  badFormat.format = 'unknown';
  throws(() => validateCorpus(badFormat), /invalid_corpus_format/);
  results.push({ id: 'loader.detect-wrong-format', result: 'pass', path: 'src/loader.ts/validateCorpus' });
  const raw = await readDataBytes(group.file);
  loadCorpusBytes(raw, group.sha256);
  const disk = JSON.parse(raw.toString('utf8'));
  const target = disk.format === 'reference-corpus-v2' ? disk.values[disk.cases[0].expected.$value] : disk.cases[0].expected;
  const before = target.digestHex; target.digestHex = (before[0] === '0' ? '1' : '0') + before.slice(1);
  throws(() => loadCorpusBytes(Buffer.from(JSON.stringify(disk) + '\n'), group.sha256), /corpus_integrity_mismatch/);
  results.push({ id: 'loader.detect-expectation-integrity-change', result: 'pass', path: 'src/loader.ts/loadCorpus' });
  const empty = structuredClone(corpus); empty.cases = [];
  throws(() => validateCorpus(empty), /invalid_corpus_format/);
  results.push({ id: 'loader.reject-empty-run', result: 'pass', path: 'src/loader.ts/validateCorpus' });
  for (const value of [1, '18446744073709551616', '01']) {
    const bad = structuredClone(corpus); bad.cases[0].input.probeU64=value;
    bad.cases[0].provenance['/input/probeU64']=structuredClone(corpus.cases[0].provenance[Object.keys(corpus.cases[0].provenance)[0]]);
    throws(()=>validateCorpus(bad),/invalid_u64/);
  }
  results.push({id:'loader.reject-numeric-overflow-and-noncanonical-u64',result:'pass',path:'src/loader.ts/validateCorpus'});
  try { digest('md5', Buffer.alloc(64)); }
  catch { throw Error('required_md5_unavailable'); }
  results.push({ id: 'primitives.md5-available', result: 'pass', path: 'src/primitives.ts/digest' });
  for await (const entry of groupCases('ed25519-profile', index)) {
    assertCaseConfirmation(entry);
    const input = entry.input;
    const actual = entry.operation === 'ed25519-point-profile' ? ed25519PointProfile(fromHex(input.pointHex))
      : entry.operation === 'ed25519-verify-profile' ? ed25519VerifyProfile(fromHex(input.publicKeyHex), fromHex(input.messageHex), fromHex(input.signatureHex))
      : (() => { throw Error('unhandled_ed25519_operation'); })();
    deepStrictEqual(actual, entry.expected, entry.id);
    results.push({ id: entry.id, result: 'pass', path: 'src/ed25519.ts/' + entry.operation, detail: entry.classification });
  }
  results.push(...recipeFormatGates());
  return results;
}
async function throwsAsync(action: () => Promise<unknown>, pattern: RegExp): Promise<void> {
  try { await action(); } catch (error) {
    if (error instanceof Error && pattern.test(error.message)) return;
    throw error;
  }
  throw Error('expected_gate_rejection');
}
