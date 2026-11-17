// SPDX-License-Identifier: 0BSD
import { rawHttp, errorBody } from './transport.ts';
import { fromHex } from './primitives.ts';
import { reject, scanBinary } from './profile.ts';
import type { Schema } from './profile.ts';
import { scanJson } from './json-profile.ts';
import { genericFields, genericContext } from './api.ts';
import { downloadResponse } from './flows.ts';
import type { UnicodeMaps } from './paths.ts';
export function rpcExchange(input: any, schema: Schema & { services: any[] }, unicode: UnicodeMaps): any {
  const request = rawHttp(fromHex(input.requestHex)), response = rawHttp(fromHex(input.responseHex));
  const target = request.start.split(' ')[1], methodName = target.split('/').at(-1)!, method = schema.services.find(row => row.method === methodName);
  if (!method || ['StartPairing', 'PollPairing', 'BeginSession', 'CompleteSession'].includes(methodName)) throw Error('RPC operation outside token-checked exchange domain');
  const coding = request.headers.get('content-encoding')?.[0];
  if (request.start.split(' ')[0] !== 'POST' || coding !== undefined && coding.toLowerCase() !== 'identity') return { serverCode: 'unimplemented' };
  if (request.body.length > 65536) return { serverCode: 'resource_exhausted', localReason: 'body_too_large' };
  const context = input.context;
  const authorization = request.headers.get('authorization');
  if (!context.tokenKnown || authorization?.length !== 1 || authorization[0] !== 'Bearer ' + context.knownTokenText) return { serverCode: 'unauthenticated', wireReason: 0 };
  if (BigInt(context.nowU64) >= BigInt(context.tokenExpiresU64)) return { serverCode: 'unauthenticated', wireReason: 4 };
  if (context.deviceRevoked) return { serverCode: 'unauthenticated', wireReason: 5 };
  if (BigInt(context.nowU64) >= BigInt(context.activityU64) + BigInt(context.idleSecondsU64)) return { serverCode: 'unauthenticated', wireReason: 6 };
  const codec = request.headers.get('content-type')?.[0].split(';')[0].trim();
  if (!['application/json', 'application/proto'].includes(codec ?? '')) throw Error('Synthetic RPC has unsupported codec');
  const read = (bytes: Uint8Array, name: string, limit: number) => codec === 'application/json' ? scanJson(bytes, name, schema, limit) : scanBinary(bytes, name, schema, { limit });
  const decodedRequest = read(request.body, method.request, 65536);
  genericFields(method.request, decodedRequest, schema);
  if (decodedRequest.page_token) {
    if (!context.pageTokenValid || !context.pageTokenBound) return { serverCode: 'invalid_argument', localReason: 'invalid_page_token' };
    if (BigInt(context.nowU64) >= BigInt(context.pageTokenIssuedU64) + 3600n) return { serverCode: 'failed_precondition', wireReason: 18 };
  }
  if (!context.existsOrAccessible) return { serverCode: 'not_found', wireReason: ['GetGame', 'ListVersions'].includes(methodName) ? 16 : ['GetVersion', 'OpenDownload', 'RefreshTicket'].includes(methodName) ? 15 : 0 };
  const responseStatus = Number(response.start.split(' ')[1]);
  if (responseStatus !== 200) {
    const error = errorBody(response.body, schema); if (!error) reject('broken_error_body');
    return { serverAccepted: true, responseError: error };
  }
  const service = method.source.split('/').at(-1), limit = service === 'library.proto' ? 4194304 : service === 'download.proto' ? 2097152 : service === 'device.proto' ? 1048576 : 65536;
  if (response.headers.get('content-type')?.[0].split(';')[0].trim() !== codec) reject('broken_error_body');
  if (response.body.length > limit) reject('response_too_large');
  const decodedResponse = read(response.body, method.response, limit);
  const expectedContext = { ...context, gameId: decodedRequest.game_id ?? context.gameId, versionId: decodedRequest.version_id ?? context.versionId };
  if (method.response === 'OpenDownloadResponse') downloadResponse(decodedResponse, expectedContext, schema, unicode);
  else { genericFields(method.response, decodedResponse, schema); genericContext(method.response, decodedResponse, expectedContext); }
  if(methodName==='RevokeDevice'&&typeof context.authenticatingDeviceId!=='string')throw Error('Missing authenticated device identity');
  return { serverAccepted: true, responseAccepted: true, revokesCallingDevice: methodName === 'RevokeSelf' || methodName === 'RevokeDevice' && decodedRequest.device_id === context.authenticatingDeviceId };
}
