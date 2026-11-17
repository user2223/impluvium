// SPDX-License-Identifier: 0BSD
import { rawHttp } from './transport.ts';
import { fromHex, digest } from './primitives.ts';
import { ed25519PointProfile } from './ed25519.ts';
import { scanBinary, reject, Rejection } from './profile.ts';
import type { Schema } from './profile.ts';
import { scanJson } from './json-profile.ts';
import { genericFields } from './api.ts';
import { verifyEnvelope, proofSemantics, uuid } from './documents.ts';
const ns = (value: any): bigint => BigInt(value.seconds) * 1000000000n + BigInt(value.nanos);
const stamp = (value: bigint) => ({ seconds: (value / 1000000000n).toString(), nanos: Number(value % 1000000000n) });
const sizeReasons = new Set(['body_too_large', 'envelope_too_large', 'byte_limit_exceeded', 'element_limit_exceeded', 'nesting_too_deep']);
function capability(values: string[]): void {
  const seen = new Set<string>();
  for (const value of values) {
    if (!/^(?:[a-z0-9_-]+\.)+v[1-9][0-9]*$/.test(value)) reject('invalid_name', 'CompleteSessionRequest.client_capabilities');
    if (seen.has(value)) reject('duplicate_capability', 'CompleteSessionRequest.client_capabilities'); seen.add(value);
  }
}
function text(value: string, field: string): void {
  if (!value.length || /[\u0000-\u001f\u007f-\u009f]/u.test(value)) reject('invalid_text', field);
}
// Input time, issuance octets, storage and atomic changes are fixed synthetic context, never host inputs.
export function authRpcFlow(input: any, schema: Schema): any {
  const context = input.context, state = structuredClone(context.state), results: any[] = [];
  function fail(reason: string, code: string, wireReason = 0, extra: any = {}): any {
    return { serverCode: code, localReason: reason, wireReason, ...extra };
  }
  for (const exchange of input.exchanges) {
    const request = rawHttp(fromHex(exchange.requestHex)), name = request.start.split(' ')[1].split('/').at(-1)!;
    if (!['StartPairing','PollPairing','BeginSession','CompleteSession'].includes(name)) throw Error('Unknown public Auth RPC');
    if(request.start.split(' ')[0]!=='POST'||request.headers.get('content-encoding')?.some(value=>value.toLowerCase()!=='identity')){results.push({serverCode:'unimplemented',wireReason:0});continue;}
    if(name==='StartPairing'&&state.addressLimited){results.push({serverCode:'resource_exhausted',localReason:'rate_limited',wireReason:19});continue;}
    const codec = request.headers.get('content-type')?.[0].split(';')[0].trim();
    if (!['application/proto','application/json'].includes(codec ?? '')) throw Error('Synthetic Auth RPC codec');
    const read = (bytes: Uint8Array, message: string) => codec === 'application/json' ? scanJson(bytes,message,schema,65536) : scanBinary(bytes,message,schema,{limit:65536});
    const now = ns(exchange.now), atomic = ns(exchange.atomicNow ?? exchange.now);
    let phase = 'request', outcome: any;
    try {
      const model = read(request.body,name+'Request');
      if (name === 'BeginSession') {
        uuid(model.device_id,'BeginSessionRequest.device_id');
        if (!state.deviceKnown || model.device_id !== context.deviceId) outcome=fail('unknown_device','unauthenticated');
        else if (state.challengeStoreFull) outcome=fail('rate_limited','resource_exhausted',19);
        else {
          state.challenge={deviceId:model.device_id,bytesHex:context.issuedChallengeHex,expiresAt:stamp(now+60000000000n),consumed:false};
        }
      } else if (name === 'StartPairing') {
        text(model.device_name,'StartPairingRequest.device_name');text(model.platform,'StartPairingRequest.platform');text(model.arch,'StartPairingRequest.arch');
        if (!ed25519PointProfile(fromHex(model.public_key)).accepted) reject('invalid_public_key','StartPairingRequest.public_key');
        text(model.client_version,'StartPairingRequest.client_version');
        if (state.addressLimited) outcome=fail('rate_limited','resource_exhausted',19);
      } else if (name === 'CompleteSession') {
        uuid(model.device_id,'CompleteSessionRequest.device_id');
        if (model.challenge.length!==64) reject('invalid_challenge_length','CompleteSessionRequest.challenge');
        if (!model.proof) reject('missing_field','CompleteSessionRequest.proof');
        if (model.scope!==1) reject('unsupported_session_scope','CompleteSessionRequest.scope');
        capability(model.client_capabilities);
        if (context.requiredCapabilities.some((value:string)=>!model.client_capabilities.includes(value))) outcome=fail('client_too_old','failed_precondition',2);
        else if (model.proof.length/2>4096) reject('envelope_too_large','CompleteSessionRequest.proof');
        else if (!state.deviceKnown || model.device_id!==context.deviceId) outcome=fail('unknown_device','unauthenticated',12);
        else {
          phase='proof';
          const checked=verifyEnvelope(fromHex(model.proof),'session',[{publicKeyHex:context.devicePublicKeyHex,role:'device'}],schema);
          proofSemantics(checked.model,'session',{instanceId:context.instanceId,deviceId:model.device_id,challengeHex:model.challenge});
          phase='state';
          const check=(time:bigint,atomicStep:boolean):any=>{
            if (state.revoked || atomicStep && exchange.atomicRevoked) return fail('device_revoked','unauthenticated',5);
            if (time>=ns(state.activityAt)+BigInt(context.idleSecondsU64)*1000000000n) return fail('device_expired','unauthenticated',6);
            if (!state.challenge || state.challenge.deviceId!==model.device_id || state.challenge.bytesHex!==model.challenge) return fail('challenge_unknown','unauthenticated',12);
            if (time>=ns(state.challenge.expiresAt)) return fail('challenge_expired','unauthenticated',12);
            if (state.challenge.consumed || atomicStep && exchange.atomicConsumed) return fail('challenge_consumed','unauthenticated',12);
            return undefined;
          };
          outcome=check(now,false);
          if (!outcome) {
            if (exchange.atomicRevoked) state.revoked=true;
            if (exchange.atomicConsumed) state.challenge.consumed=true;
            outcome=check(atomic,true);
          }
          if (!outcome) { state.challenge.consumed=true;state.activityAt=stamp(atomic);state.sessionCapabilities=model.client_capabilities.filter((value:string)=>context.implementedCapabilities.includes(value)); }
        }
      } else {
        if (model.pairing_id.length!==32) reject('invalid_pairing_id_length','PollPairingRequest.pairing_id');
        if (!model.proof) reject('missing_field','PollPairingRequest.proof');
        if (model.proof.length/2>4096) reject('envelope_too_large','PollPairingRequest.proof');
        const pairing=state.pairing;
        if (!pairing?.exists || pairing.completed || model.pairing_id!==context.pairingIdHex) outcome=fail('pairing_unknown','failed_precondition',11);
        else {
          phase='proof';
          const checked=verifyEnvelope(fromHex(model.proof),'pairing',[{publicKeyHex:context.devicePublicKeyHex,role:'device'}],schema);
          proofSemantics(checked.model,'pairing',{instanceId:context.instanceId,pairingIdHex:model.pairing_id});
          phase='state';
          if (now>=ns(pairing.expiresAt)) outcome=fail('pairing_expired','failed_precondition',11);
          else if (pairing.denied) outcome=fail('pairing_denied','permission_denied',10);
          else if (pairing.lastPollAt && now<ns(pairing.lastPollAt)+BigInt(pairing.intervalSeconds)*1000000000n) {
            pairing.intervalSeconds+=5;pairing.lastPollAt=exchange.now;
            outcome=fail('slow_down','resource_exhausted',9,{retryAfterSeconds:pairing.intervalSeconds});
          } else {
            pairing.lastPollAt=exchange.now;
            if (!pairing.approved) outcome=fail('pairing_pending','failed_precondition',8);
            else {
              if(exchange.atomicCompleted)pairing.completed=true;
              if(exchange.atomicDeleted)pairing.exists=false;
              if(exchange.atomicDenied)pairing.denied=true;
              if(exchange.atomicApprovalRemoved)pairing.approved=false;
              if (!pairing.exists || pairing.completed) outcome=fail('pairing_unknown','failed_precondition',11);
              else if (atomic>=ns(pairing.expiresAt)) outcome=fail('pairing_expired','failed_precondition',11);
              else if (pairing.denied) outcome=fail('pairing_denied','permission_denied',10);
              else if (!pairing.approved) outcome=fail('pairing_pending','failed_precondition',8);
              else pairing.completed=true;
            }
          }
        }
      }
      if (!outcome) {
        phase='response';
        const response=rawHttp(fromHex(exchange.responseHex));
        if (Number(response.start.split(' ')[1])!==200 || response.headers.get('content-type')?.[0].split(';')[0].trim()!==codec) reject('broken_error_body');
        if (response.body.length>65536) reject('response_too_large');
        const model=read(response.body,name+'Response');
        genericFields(name+'Response',model,schema);
        if (name==='PollPairing') {
          if(model.instance_id!==context.instanceId)reject('instance_mismatch','PollPairingResponse.instance_id');
          if(!ed25519PointProfile(fromHex(model.content_public_key)).accepted)reject('invalid_trusted_key','PollPairingResponse.content_public_key');
          if(context.contentFingerprintsHex.length && !context.contentFingerprintsHex.includes(digest('sha256',fromHex(model.content_public_key)).toString('hex')))reject('content_key_changed','PollPairingResponse.content_public_key');
        }
        outcome={serverAccepted:true,responseAccepted:true};
      }
    } catch(error) {
      if(!(error instanceof Rejection))throw error;
      outcome=phase==='proof'?fail(error.reason,'unauthenticated',12):phase==='response'?{serverAccepted:true,clientReason:error.reason,field:error.field}:fail(error.reason,sizeReasons.has(error.reason)?'resource_exhausted':'invalid_argument');
    }
    results.push(outcome);
  }
  return {results,challengeConsumed:state.challenge?.consumed??false,activityAt:state.activityAt,
    pairingCompleted:state.pairing?.completed??false,pollInterval:state.pairing?.intervalSeconds??5,
    lastPollAt:state.pairing?.lastPollAt??null,sessionCapabilities:state.sessionCapabilities??[]};
}
