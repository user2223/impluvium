// SPDX-License-Identifier: 0BSD
import { fromHex, digest } from './primitives.ts';
import { reject, scanBinary, Rejection } from './profile.ts';
import type { Schema } from './profile.ts';
import { scanJson } from './json-profile.ts';
import { absoluteUrl } from './api.ts';
import { verifyEnvelope, manifestSemantics, descriptorSemantics, descriptorContext } from './documents.ts';
import type { UnicodeMaps } from './paths.ts';
export function rangeSpan(text: string, size: bigint): { first: bigint; last: bigint } | undefined {
  const match = /^bytes=(\d*)-(\d*)$/.exec(text);
  if (!match || !match[1] && !match[2] || size < 1n) return undefined;
  if (!match[1]) { const suffix = BigInt(match[2]); return suffix ? { first: suffix >= size ? 0n : size - suffix, last: size - 1n } : undefined; }
  const first = BigInt(match[1]), requestedLast = match[2] ? BigInt(match[2]) : size - 1n;
  if (first >= size || requestedLast < first) return undefined;
  return { first, last: requestedLast >= size ? size - 1n : requestedLast };
}
export function rawHttp(bytes: Uint8Array): { start: string; headers: Map<string, string[]>; headerBytes: number; headBytes: number[]; body: Buffer } {
  const buffer = Buffer.from(bytes), response = buffer.subarray(0,5).toString('ascii') === 'HTTP/';
  const headBytes: number[] = []; let offset = 0;
  while (true) {
    const end = buffer.indexOf('\r\n\r\n', offset);
    if (end < 0) { if(response && buffer.length-offset>65536)reject('headers_too_large');throw Error('Incomplete synthetic HTTP header section'); }
    const counted = end + 4 - offset; headBytes.push(counted);
    if (response && counted > 65536) reject('headers_too_large');
    const lines = buffer.subarray(offset, end).toString('latin1').split('\r\n'), start = lines.shift()!;
    const headers = new Map<string, string[]>();
    for (const line of lines) {
      const colon = line.indexOf(':'); if (colon < 1) throw Error('Invalid synthetic HTTP field');
      const name = line.slice(0, colon).toLowerCase(), value = line.slice(colon + 1).trim();
      headers.set(name, [...headers.get(name) ?? [], value]);
    }
    offset = end + 4;
    const status = response ? Number(start.split(' ')[1]) : 0;
    if (status >= 100 && status < 200) continue;
    return { start, headers, headerBytes: counted, headBytes, body: buffer.subarray(offset) };
  }
}
// Decoded header-list precheck only: it does not decode HPACK or assert a full HTTP/2 exchange.
export function responseHeadSize(input: { protocol: string; heads: {nameHex:string;valueHex:string}[][] }): any {
  if(input.protocol!=='h2')throw Error('Unsupported response-head precheck protocol');
  const sizesU64:string[]=[];
  for(let index=0;index<input.heads.length;index++){
    let size=0n;for(const field of input.heads[index])size+=BigInt(fromHex(field.nameHex).length)+BigInt(fromHex(field.valueHex).length)+32n;
    sizesU64.push(size.toString());if(size>65536n)return {reason:'headers_too_large',field:'',headIndex:index,measuredU64:size.toString()};
  }
  return {accepted:true,sizesU64};
}
const fallback = (status: number): string => status === 413 ? 'request_too_large' : status === 429 ? 'rate_limited' : [502, 503, 504].includes(status) ? 'server_unavailable' : 'broken_error_body';
export function errorBody(bytes: Uint8Array, schema: Schema): any | undefined {
  if (bytes.length > 65536) return undefined;
  // This JSON-only control schema is defined by errors.md 3.2, not by a fabricated protobuf message.
  const control = { ...schema, messages: { ...schema.messages,
    ErrorBody: { fields: [{ no: 1, name: 'code', type: 'string', maxBytes: 64 }, { no: 2, name: 'message', type: 'string', maxBytes: 4096 }, { no: 3, name: 'details', type: 'ErrorDetail', repeated: true, maxElements: 16 }] },
    ErrorDetail: { fields: [{ no: 1, name: 'type', type: 'string', maxBytes: 256 }, { no: 2, name: 'value', type: 'bytes', maxBytes: 8192 }] } } };
  try {
    const model = scanJson(bytes, 'ErrorBody', control,65536,true,{
      required:{ErrorBody:['code'],ErrorDetail:['type','value']},
      nonNull:{ErrorBody:['code','message','details'],ErrorDetail:['type','value']}
    });
    if (!model.code.length) return undefined;
    for (const detail of model.details) if (!detail.type.length) return undefined;
    const info = model.details.filter((detail: any) => detail.type === 'impluvium.v2.ErrorInfo');
    if (info.length > 1) return undefined;
    let reason = 0;
    if (info.length) {
      const decoded = scanBinary(fromHex(info[0].value), 'ErrorInfo', schema, { limit: 8192 });
      for (const pair of decoded.metadata) if (!/^[\x21-\x7e]{1,128}$/.test(pair.key) || /[\u0000-\u001f\u007f-\u009f]/u.test(pair.value)) return undefined;
      if (decoded.retry_after_seconds > 86400) return undefined;
      reason = decoded.reason;
    }
    const known = ['canceled', 'unknown', 'invalid_argument', 'deadline_exceeded', 'not_found', 'already_exists', 'permission_denied', 'resource_exhausted', 'failed_precondition', 'aborted', 'out_of_range', 'unimplemented', 'internal', 'unavailable', 'data_loss', 'unauthenticated'];
    return { code: known.includes(model.code) ? model.code : 'unknown', reason };
  } catch (error) { if (error instanceof Rejection) return undefined; throw error; }
}
export function responseDecision(input: any, schema: Schema, unicode: UnicodeMaps, blake3?: (bytes:Uint8Array)=>Uint8Array): any {
  let response: ReturnType<typeof rawHttp>;
  try { response = rawHttp(fromHex(input.responseHex)); }
  catch(error) { if(!(error instanceof Rejection))throw error;return {reason:error.reason,field:error.field,keepCredentials:true}; }
  const status = Number(response.start.split(' ')[1]);
  const one = (name: string) => response.headers.get(name)?.[0];
  const context = { ...input.context }, success = status === context.successStatus;
  if (input.requestHex !== undefined) {
    const request = rawHttp(fromHex(input.requestHex)), target = request.start.split(' ')[1], last = target.split('?')[0].split('/').at(-1);
    if (context.purpose === 'manifest') context.manifestIdHex = last;
    if (context.purpose === 'descriptor') context.versionId = last;
    if (context.purpose === 'chunk') { context.chunkIdHex = last; context.range = request.headers.get('range')?.[0]; }
  }
  if (status >= 300 && status < 400 && context.purpose !== 'chunk') return { reason: 'redirect_not_allowed', keepCredentials: true };
  if (one('content-encoding') !== undefined && one('content-encoding')!.toLowerCase() !== 'identity') return { reason: 'unsupported_content_encoding', keepCredentials: true };
  const type = one('content-type')?.split(';')[0].trim().toLowerCase();
  const expectedType = success ? context.contentType : 'application/json';
  if (expectedType !== undefined && type !== expectedType) return { reason: fallback(status), keepCredentials: true };
  if (context.deadlineExceeded || context.incompleteTransfer) return { decision: 'transport-failure', keepCredentials: true };
  const limit = success ? context.successLimit : 65536;
  if (response.body.length > limit) return { reason: success ? 'response_too_large' : fallback(status), keepCredentials: true };
  if (!success) {
    const error = errorBody(response.body, schema);
    if (!error) return { reason: fallback(status), keepCredentials: true };
    const cleanup = context.serverOrigin && context.verifiedConnection && context.purpose !== 'chunk' && error.code === 'unauthenticated' && [5, 6].includes(error.reason);
    return { error, keepCredentials: !cleanup };
  }
  if (context.purpose === 'manifest' || context.purpose === 'descriptor') {
    const checked = verifyEnvelope(response.body, context.purpose, context.keys, schema);
    if (context.purpose === 'manifest') {
      manifestSemantics(checked.model, unicode, {}, blake3);
      if (digest('sha256', checked.payload).toString('hex') !== context.manifestIdHex) reject('manifest_id_mismatch');
    } else {
      descriptorSemantics(checked.model, unicode);
      const manifestBytes = fromHex(context.manifestPayloadHex), model = scanBinary(manifestBytes, 'Manifest', schema, { limit: 536870912 });
      manifestSemantics(model, unicode, {}, blake3);
      descriptorContext(checked.model, checked.payload, model, manifestBytes, unicode, context);
    }
  } else if (context.purpose === 'chunk') {
    const complete = fromHex(context.completeChunkHex);
    if(context.hashAlgorithm===2&&!blake3)throw Error('BLAKE3 chunk gate requires the container dependency');
    const completeHash=context.hashAlgorithm===2?Buffer.from(blake3!(complete)):digest('sha256',complete);
    if (complete.length !== context.chunkSize || completeHash.toString('hex') !== context.chunkIdHex) return { decision: 'discard-chunk', keepCredentials: true };
    if (context.range !== undefined) {
      const span = rangeSpan(context.range, BigInt(complete.length));
      if (!span || !response.body.equals(complete.subarray(Number(span.first), Number(span.last) + 1))) return { decision: 'discard-chunk', keepCredentials: true };
    } else if (!response.body.equals(complete)) return { decision: 'discard-chunk', keepCredentials: true };
  }
  return { decision: 'accept', keepCredentials: true };
}
export function transportDecision(input: any): any {
  if(input.baseUrl!==undefined)absoluteUrl(input.baseUrl.replace(/\/$/,''),true);
  const url = absoluteUrl(input.url);
  if(url.hash)reject('invalid_url');
  if (!input.platformTrustAvailable) return { reason: 'platform_trust_unavailable', credentialRead: false, connected: false };
  const host = url.hostname, octets = host.split('.');
  const ipv4 = octets.length === 4 && octets.every(part => /^(?:0|[1-9][0-9]{0,2})$/.test(part) && Number(part) <= 255);
  const loopback = host === 'localhost' || host.endsWith('.localhost') || host === '[::1]' || ipv4 && octets[0] === '127';
  if (url.protocol === 'http:' && !loopback && !input.insecureMode) return { reason: 'insecure_scheme_not_allowed', credentialRead: false, connected: false };
  if (input.purpose === 'chunk' && !input.allowedUrls.includes(url.href)) return { reason: 'origin_not_allowed', credentialRead: false, connected: false };
  if(input.purpose!=='chunk'&&input.baseUrl!==undefined){const base=absoluteUrl(input.baseUrl.replace(/\/$/,''),true),prefix=base.pathname.replace(/\/$/,''),pathAllowed=input.purpose==='discovery'?url.pathname===prefix+'/api/info':url.pathname.startsWith(prefix+'/api/v2/');if(url.origin!==base.origin||!pathAllowed)return {reason:'origin_not_allowed',credentialRead:false,connected:false};}
  if(input.purpose!=='discovery'&&['instance_mismatch','content_key_changed'].includes(input.trustState))return {reason:input.trustState,credentialRead:false,connected:false};
  if(input.verifiedConnection===false&&input.purpose!=='discovery'&&input.purpose!=='chunk')return {decision:'verify-connection',credentialRead:false,connected:true};
  if(input.purpose==='chunk'&&input.ticketFromVerifiedConnection===false)return {decision:'obtain-authenticated-download',credentialRead:false,connected:true};
  return { decision: 'connect', credential: input.purpose === 'discovery' ? 'none' : input.purpose === 'chunk' ? 'ticket' : 'access-token' };
}
export function redirectDecision(input: any): any {
  if (input.purpose !== 'chunk') return { reason: 'redirect_not_allowed' };
  if (input.followed >= 5) return { reason: 'redirect_limit' };
  const original = absoluteUrl(input.original);let resolved:URL;try{resolved=new URL(input.location,input.current);}catch{return reject('invalid_url');}const target=absoluteUrl(resolved.href);
  if (target.origin !== original.origin) return { reason: 'redirect_cross_origin' };
  if (!input.allowedUrls.includes(target.href)) return { reason: 'redirect_target_not_allowed' };
  const preflight = transportDecision({ ...input, url:target.href });
  if (preflight.reason || preflight.decision !== 'connect') return preflight;
  return { decision: 'follow', target: target.href, credential: 'ticket' };
}
