// SPDX-License-Identifier: 0BSD
import { rawHttp, rangeSpan } from './transport.ts';
import { fromHex } from './primitives.ts';
import { verifyEnvelope, ticketSemantics, uuid } from './documents.ts';
import { base64Bytes } from './json-profile.ts';
import { Rejection } from './profile.ts';
import type { Schema } from './profile.ts';
export function serverGet(input: any, schema: Schema): any {
  const request = rawHttp(fromHex(input.requestHex)), [method, target] = request.start.split(' '), context = input.context;
  if (context.serverStarting) return {status:503,wireReason:20};
  if (method !== 'GET') return {status:405,wireReason:0};
  if(context.purpose==='discovery')return context.addressLimited?{status:429,wireReason:19}:{status:200,headers:{'Content-Type':'application/json','Content-Length':String(context.discoveryHex.length/2)},bodyHex:context.discoveryHex};
  const parts = target.split('/'), id = parts.at(-1)!, manifestId = parts.at(-2)!;
  const depot = context.purpose === 'chunk';
  let claims;
  if (depot) {
    const headers = request.headers.get('authorization');
    if (headers?.length !== 1 || headers[0].length > 5469 || !headers[0].startsWith('Bearer ')) return { status: 401, wireReason: 0 };
    try {
      const ticket = base64Bytes(headers[0].slice(7), '', true), checked = verifyEnvelope(ticket, 'ticket', context.keys, schema);
      claims = checked.model;
      ticketSemantics(claims, checked.validKeys, { ...context.ticketContext, manifestIdHex: claims.manifest_ids[0] });
    } catch (error) { if (!(error instanceof Rejection)) throw error; return { status: 401, wireReason: error.reason === 'ticket_expired' ? 13 : 0 }; }
    if(context.rateLimited||context.concurrencyLimited){
      const status=context.limitStatus??429;
      if(![429,503].includes(status))throw Error('Invalid depot limit status');
      return {status,wireReason:19};
    }
  } else {
    const authorization = request.headers.get('authorization');
    if(authorization?.length!==1||!authorization[0].startsWith('Bearer '))return {status:401,wireReason:0};
    const tokenError = !context.tokenKnown || authorization[0] !== 'Bearer '+context.knownTokenText ? 0 : BigInt(context.nowU64 ?? 0) >= BigInt(context.tokenExpiresU64 ?? 1) ? 4 : context.deviceRevoked ? 5 : context.deviceExpired ? 6 : undefined;
    if (tokenError !== undefined) return { status: 401, wireReason: tokenError };
    if(context.rateLimited||context.concurrencyLimited)return {status:429,wireReason:19};
  }
  let validPath = true;
  if (depot || context.purpose === 'manifest') validPath = /^[a-f0-9]{64}$/.test(id) && (!depot || /^[a-f0-9]{64}$/.test(manifestId));
  else { try { uuid(id, 'path'); } catch (error) { if (!(error instanceof Rejection)) throw error; validPath = false; } }
  if (!validPath) return { status: 404, wireReason: context.purpose === 'descriptor' ? 15 : 0 };
  if (depot && !claims.manifest_ids.includes(manifestId)) return { status: 403, wireReason: 14 };
  const object = context.objects.find((row: any) => row.idHex === id || row.id === id);
  if (!object || !context.accessible) return { status: 404, wireReason: context.purpose === 'descriptor' ? 15 : 0 };
  if (depot && context.sourceChanged) return { status: 409, wireReason: 17 };
  const headers: Record<string, string> = { 'Content-Type': context.purpose === 'object' ? object.contentType : 'application/octet-stream', 'Content-Length': String(object.bodyHex.length / 2) };
  if (context.purpose === 'manifest') { headers.ETag = '"' + id + '"'; headers['Cache-Control'] = 'private, immutable'; }
  if (context.purpose === 'descriptor') headers['Cache-Control'] = 'private, no-cache';
  if (depot) {
    const range = request.headers.get('range')?.[0], span = range === undefined ? undefined : rangeSpan(range, BigInt(object.bodyHex.length / 2));
    if (range !== undefined && (!span || request.headers.get('range')?.length !== 1)) return { status: 416, wireReason: 0, contentRange: 'bytes */' + object.bodyHex.length / 2 };
    headers.ETag = '"' + id + '"'; headers['Accept-Ranges'] = 'bytes'; headers['Cache-Control'] = 'private, immutable';
    if (span) {
      const bodyHex = object.bodyHex.slice(Number(span.first) * 2, (Number(span.last) + 1) * 2);
      headers['Content-Range'] = 'bytes ' + span.first + '-' + span.last + '/' + object.bodyHex.length / 2; headers['Content-Length'] = String(bodyHex.length / 2);
      return { status: 206, headers, bodyHex };
    }
  }
  return { status: 200, headers, bodyHex: object.bodyHex };
}
