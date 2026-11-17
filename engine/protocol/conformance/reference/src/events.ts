// SPDX-License-Identifier: 0BSD
import { reject, utf8 } from './profile.ts';
import type { Schema } from './profile.ts';
import { scanJson } from './json-profile.ts';
import { fromHex } from './primitives.ts';
import { genericFields } from './api.ts';
import { uuid } from './documents.ts';
import { rawHttp } from './transport.ts';
export function receiveEvent(input: any, schema: Schema): any {
  const bytes = fromHex(input.eventHex);
  if (bytes.length > 65536) reject('event_too_large');
  const text = utf8(bytes, 'event_malformed'), fields: Record<string, string[]> = {};
  for (const line of text.split(/\r\n|\r|\n/)) {
    if (!line || line.startsWith(':')) continue;
    const colon = line.indexOf(':'), name = colon < 0 ? line : line.slice(0, colon);
    const value = colon < 0 ? '' : line.slice(colon + 1).replace(/^ /, '');
    (fields[name] ??= []).push(value);
  }
  if (fields.event?.length !== 1 || !fields.event[0].length || fields.data?.length !== 1) reject('event_malformed');
  const model = scanJson(Buffer.from(fields.data[0], 'utf8'), 'ServerEvent', schema);
  const name = fields.event[0], context = input.context;
  if (context.first ? name !== 'connected' : name === 'connected') reject('event_out_of_order');
  const known = ['connected', 'heartbeat', 'resync_required', 'session_ended', 'superseded', 'library_changed'];
  const present = known.filter(key => model[key] !== undefined);
  if (known.includes(name)) {
    if (model[name] === undefined) reject('missing_field', 'ServerEvent.' + name);
    if (present.some(key => key !== name)) reject('event_name_mismatch');
  } else if (present.length) reject('event_name_mismatch');
  if (!model.epoch) reject('missing_field', 'ServerEvent.epoch');
  if (model.epoch.length !== 32) reject('invalid_length', 'ServerEvent.epoch');
  if (!model.time) reject('missing_field', 'ServerEvent.time');
  if (name === 'library_changed') {
    if (!model.library_changed.game_id) reject('missing_field', 'LibraryChanged.game_id');
    uuid(model.library_changed.game_id, 'LibraryChanged.game_id');
    if (model.library_changed.game) genericFields('GameSummary', model.library_changed.game, schema);
  }
  if (!context.first && model.epoch !== context.epochHex) reject('event_epoch_mismatch', 'ServerEvent.epoch');
  const id = fields.id?.at(-1), hasId = id !== undefined;
  if ((name === 'library_changed' || name === 'heartbeat') && !hasId || known.includes(name) && !['library_changed', 'heartbeat'].includes(name) && hasId) reject('event_id_mismatch');
  if (hasId) {
    const match = /^([a-f0-9]{32})-(0|[1-9][0-9]*)$/.exec(id);
    if (!match || match[1] !== model.epoch || BigInt(match[2]) !== BigInt(model.seq)) reject('event_id_mismatch');
  }
  const data = name === 'library_changed' || !known.includes(name) && hasId;
  if (data && (BigInt(model.seq) < 1n || BigInt(model.prev_seq) >= BigInt(model.seq) || BigInt(model.seq) <= BigInt(context.lastDataU64))) reject('event_sequence_invalid');
  if (name === 'heartbeat' && BigInt(model.seq) < BigInt(context.lastDataU64)) reject('event_sequence_invalid');
  if (name === 'library_changed') {
    const payload = model.library_changed;
    if (!!payload.game === payload.deleted) reject('event_payload_invalid', 'LibraryChanged.game');
    if (payload.game && (payload.game.game_id !== payload.game_id || payload.game.entry_version !== model.seq)) reject('event_identity_mismatch');
  }
  return { accepted: true, dataEvent: data, epochHex: model.epoch, lastDataU64: data ? model.seq : context.lastDataU64 };
}
export function resumeDecision(input: any): string {
  if (input.point === undefined) return 'live';
  const match = /^([a-f0-9]{32})-(0|[1-9][0-9]*)$/.exec(input.point);
  if (Buffer.byteLength(input.point) > 53 || !match || BigInt(match[2]) > 18446744073709551615n) return 'MALFORMED';
  if (match[1] !== input.epochHex) return 'FOREIGN_EPOCH';
  const value = BigInt(match[2]);
  if (value > BigInt(input.headU64)) return 'AHEAD';
  if (input.missingAfterU64 !== undefined && BigInt(input.missingAfterU64) > value) return 'TOO_OLD';
  return 'resumed';
}
export function supersessionKey(input: any): string | null {
  if (input.auth === 'bearer' && input.client === undefined) return null;
  return input.auth + ':' + input.owner + ':' + input.client;
}
export function openEventStream(input: any): any {
  const request = rawHttp(fromHex(input.requestHex)), query = new URL(request.start.split(' ')[1], input.serverOrigin).searchParams;
  if(input.serverStarting)return {status:503,wireReason:20};
  if(request.start.split(' ')[0]!=='GET')return {status:405,wireReason:0};
  const authorization=request.headers.get('authorization')??[],cookieName=input.sessionCookieName??'session';
  const sessions=(request.headers.get('cookie')??[]).flatMap(line=>line.split(';').map(value=>value.trim())).filter(value=>value.slice(0,value.indexOf('='))===cookieName);
  const bearer=authorization.length===1,cookie=sessions.length===1;
  if(authorization.length>1||sessions.length>1||authorization.length&&sessions.length||!bearer&&!cookie||bearer&&!authorization[0].startsWith('Bearer '))return {status:401};
  const valid=bearer?(input.bearerValid??input.credentialValid):(input.cookieValid??input.credentialValid);
  if (!valid || cookie && request.headers.get('origin')?.[0] !== input.serverOrigin) return { status: 401 };
  for (const name of ['topics', 'client', 'last_event_id']) if (query.getAll(name).length > 1) return { status: 400, reason: 'invalid_parameter' };
  if ((request.headers.get('last-event-id')?.length ?? 0) > 1) return { status: 400, reason: 'invalid_parameter' };
  const topics = query.get('topics')?.split(',');
  if (!topics || !topics.length || topics.length > 8 || topics.some(topic => topic !== 'library') || new Set(topics).size !== topics.length) return { status: 400, reason: 'invalid_topic' };
  const client = query.get('client');
  if (client !== null && !/^[A-Za-z0-9_-]{1,64}$/.test(client) || cookie && client === null) return { status: 400, reason: 'invalid_client_id' };
  if (!input.topicAuthorized) return { status: 403 };
  if (client !== null && input.newClientIdentifier && input.clientIdentifierCount >= 256) return { status: 429, reason: 'client_table_full' };
  return { status: 200, contentType: 'text/event-stream', cacheControl: 'no-cache', accelBuffering: 'no' };
}
export function eventOpenModel(input:any):any {
 let owner=input.initialOwner??null,identifierEntries=input.initialIdentifierEntries??0;const owners=new Set<string>(input.initialOwners??(owner===null?[]:[owner])),results=[];
 const generations=new Map<string,string>((input.initialGenerations??[]).map((row:any)=>[row.key,row.streamId]));
 for(const attempt of input.attempts){
  if(attempt.kind==='release'){const released=generations.get(attempt.key)===attempt.streamId;if(released){generations.delete(attempt.key);owners.delete(attempt.key);identifierEntries--;if(owner===attempt.key)owner=null;}results.push({released});continue;}
  let response=openEventStream({...attempt,newClientIdentifier:false});let supersedes=false,createdIdentifier=false;
  if(response.status===200){const request=rawHttp(fromHex(attempt.requestHex)),client=new URL(request.start.split(' ')[1],attempt.serverOrigin).searchParams.get('client');
   if(client!==null){const auth=request.headers.has('authorization')?'bearer':'cookie',key=auth+':'+(auth==='bearer'?attempt.bearerOwner:attempt.cookieOwner)+':'+client;
    if(!owners.has(key)&&identifierEntries>=256)response={status:429,reason:'client_table_full'};
    else{supersedes=owners.has(key);createdIdentifier=!supersedes;if(createdIdentifier)identifierEntries++;owners.add(key);owner=key;generations.set(key,attempt.streamId??String(results.length));}
   }
  }
  results.push({status:response.status,supersedes,createdIdentifier});
 }
 return {results,owner,identifierEntries,...(input.observeGenerations?{generations:[...generations].map(([key,streamId])=>({key,streamId}))}:{})};
}
