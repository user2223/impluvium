// SPDX-License-Identifier: 0BSD
import { digest, fromHex } from './primitives.ts';
import { reject, scanBinary } from './profile.ts';
import type { Schema } from './profile.ts';
import { genericFields, absoluteUrl, versionDecision } from './api.ts';
import { verifyEnvelope, descriptorSemantics, manifestSemantics } from './documents.ts';
import type { UnicodeMaps } from './paths.ts';
export function downloadResponse(model: any, context: any, schema: Schema, unicode: UnicodeMaps): void {
  genericFields('OpenDownloadResponse', model, schema, {deferDepotUrl:true});
  const descriptor = verifyEnvelope(fromHex(model.descriptor), 'descriptor', context.keys, schema);
  descriptorSemantics(descriptor.model, unicode);
  if (descriptor.model.version_id !== context.versionId) reject('descriptor_version_mismatch', 'ReleaseDescriptor.version_id');
  if (context.gameId !== undefined && descriptor.model.game_id !== context.gameId) reject('descriptor_game_mismatch', 'ReleaseDescriptor.game_id');
  if (!model.manifest_ids.includes(descriptor.model.manifest_id)) reject('manifest_set_mismatch', 'OpenDownloadResponse.manifest_ids');
  for (const endpoint of model.depot_endpoints) { try { absoluteUrl(endpoint.base_url, true); } catch { reject('invalid_url', 'DepotEndpoint.base_url'); } }
}
export function reconstruct(input: any, schema: Schema, unicode: UnicodeMaps, blake3?: (bytes:Uint8Array)=>Uint8Array): any {
  const model = scanBinary(fromHex(input.manifestPayloadHex), 'Manifest', schema, { limit: 536870912 });
  manifestSemantics(model, unicode, {}, blake3);
  if(model.header.hash_algorithm===2&&!blake3)throw Error('BLAKE3 reconstruction requires the container dependency');
  const contentHash=(bytes:Uint8Array)=>model.header.hash_algorithm===2?Buffer.from(blake3!(bytes)):digest('sha256',bytes);
  const chunks = new Map<string, Buffer>();
  for (const chunk of model.chunks) {
    const source = input.chunks.find((candidate: any) => candidate.idHex === chunk.id);
    if (!source) return { decision: 'missing-chunk' };
    const bytes = fromHex(source.bodyHex);
    if (BigInt(bytes.length) !== BigInt(chunk.size) || contentHash(bytes).toString('hex') !== chunk.id) return { decision: 'discard-chunk' };
    chunks.set(chunk.id, bytes);
  }
  const files = [];
  for (const entry of model.entries) if (entry.type === 1) {
    const bytes = Buffer.concat(entry.chunk_indexes.map((index: number) => chunks.get(model.chunks[index].id)!));
    if (BigInt(bytes.length) !== BigInt(entry.size) || contentHash(bytes).toString('hex') !== entry.hash) return { decision: 'discard-file' };
    files.push({ path: entry.path, bodyHex: bytes.toString('hex') });
  }
  return { decision: 'reconstructed', files };
}
export function trustModel(input: any): any {
  const verified = new Set<string>(), decisions: any[] = [];
  let state = 'trusted';
  for (const operation of input.operations) {
    if (operation.kind === 'open') { verified.delete(operation.connection); decisions.push({ verified: false }); continue; }
    if (operation.kind === 'discovery') {
      if (state === 'instance_mismatch' || state === 'content_key_changed') { decisions.push({ state, verified: false }); continue; }
      if (operation.instanceId !== input.pinnedInstanceId) state = 'instance_mismatch';
      else if (operation.contentFingerprintHexes.length && !operation.contentFingerprintHexes.includes(digest('sha256', fromHex(input.pinnedContentPublicKeyHex)).toString('hex'))) state = 'content_key_changed';
      else {
        state = versionDecision(operation.protocol, input.client);
        if (state === 'compatible') state = operation.requiredCapabilities.every((name: string) => input.capabilities.includes(name)) ? 'trusted' : 'client_too_old';
      }
      if (state === 'trusted') verified.add(operation.connection); else verified.delete(operation.connection);
      decisions.push({ state, verified: verified.has(operation.connection) }); continue;
    }
    if (operation.kind === 'credential') decisions.push({ credentialSent: state === 'trusted' && verified.has(operation.connection), state });
    else if (operation.kind === 'close') verified.delete(operation.connection);
  }
  return { decisions, state, credentialsDeleted: false };
}
