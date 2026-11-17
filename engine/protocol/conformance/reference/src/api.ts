// SPDX-License-Identifier: 0BSD
import { reject } from './profile.ts';
import type { Schema } from './profile.ts';
import { uuid } from './documents.ts';
import { base64Bytes } from './json-profile.ts';
type Rule = { required?: boolean; requiredReason?: string; bytes?: number; text?: number; syntax?: string; min?: bigint; max?: bigint; equals?: number; reason?: string; unique?: boolean; ascending?: boolean; ascendingReason?:string };
const rules: Record<string, Record<string, Rule>> = {
  Discovery: { server_name: { required: true, text: 1 }, server_version: { required: true, text: 1 }, instance_id: { required: true, syntax: 'uuid' }, protocol: { required: true }, manifest_formats: { required: true, unique: true }, capabilities: { syntax: 'capability', unique: true }, required_client_capabilities: { syntax: 'capability', unique: true }, commit: { text: 0 }, source_url: { required: true, syntax: 'url' } },
  OpenDownloadResponse: { descriptor: { required: true }, manifest_ids: { required: true, bytes: 32, ascending: true, ascendingReason: 'unsorted_manifest_ids' }, ticket: { required: true }, depot_endpoints: { required: true } },
  DepotEndpoint: { base_url: { required: true } },
  StartPairingResponse: { pairing_id: { required: true, bytes: 16, reason: 'invalid_pairing_id_length' }, user_code: { required: true, bytes: 9, syntax: 'user-code', reason: 'invalid_user_code' }, verification_uri: { required: true, syntax: 'url' }, verification_uri_complete: { required: true, syntax: 'url' }, interval_seconds: { required: true, min: 1n, max: 60n }, expires_at: { required: true } },
  PollPairingResponse: { device_id: { required: true, syntax: 'uuid' }, instance_id: { required: true, syntax: 'uuid' }, content_public_key: { required: true, bytes: 32, reason: 'invalid_trusted_key' } },
  BeginSessionResponse: { challenge: { required: true, bytes: 32, reason: 'invalid_challenge_length' }, expires_at: { required: true } },
  CompleteSessionResponse: { access_token: { required: true, bytes: 43, syntax: 'token', reason: 'invalid_base64' }, expires_at: { required: true }, accepted_capabilities: { syntax: 'capability', unique: true }, scope: { required: true, requiredReason: 'unsupported_session_scope', equals: 1, reason: 'unsupported_session_scope' } },
  Device: { device_id: { required: true, syntax: 'uuid' }, name: { required: true, text: 1 }, platform: { required: true, text: 1 }, arch: { required: true, text: 1 }, client_version: { required: true, text: 1 }, created_at: { required: true } },
  RevokeDeviceRequest: { device_id: { required: true, syntax: 'uuid' } },
  ListLibraryResponse: { epoch: { required: true, bytes: 16 } },
  GameSummary: { game_id: { required: true, syntax: 'uuid' }, name: { required: true, text: 1 }, platforms: { unique: true }, icon_object_id: { syntax: 'uuid' }, cover_object_id: { syntax: 'uuid' } },
  GetGameRequest: { game_id: { required: true, syntax: 'uuid' } }, GetGameResponse: { game: { required: true } },
  ListVersionsRequest: { game_id: { required: true, syntax: 'uuid' } },
  VersionSummary: { version_id: { required: true, syntax: 'uuid' }, game_id: { required: true, syntax: 'uuid' }, display_name: { text: 0 }, kind: { required: true }, platforms: { unique: true }, manifest_size: { required: true, min: 1n, max: 536870912n }, manifest_id: { required: true, bytes: 32 }, descriptor_revision: { required: true, min: 1n } },
  GetVersionRequest: { version_id: { required: true, syntax: 'uuid' } }, GetVersionResponse: { version: { required: true } },
  OpenDownloadRequest: { version_id: { required: true, syntax: 'uuid' } }, RefreshTicketRequest: { version_id: { required: true, syntax: 'uuid' } },
  RefreshTicketResponse: { ticket: { required: true }, manifest_ids: { required: true, bytes: 32, ascending: true, ascendingReason: 'unsorted_manifest_ids' } },
  ProtocolVersion: { major: { required: true, min: 1n }, revision: { required: true, min: 1n }, min_client_revision: { required: true, min: 1n } }
};
export function absoluteUrl(value: string, base = false): URL {
  let url: URL;
  try { url = new URL(value); } catch { return reject('invalid_url'); }
  if (!['http:', 'https:'].includes(url.protocol) || url.username || url.password || (base && (/[?#]/.test(url.href) || value.endsWith('/')))) reject('invalid_url');
  return url;
}
export function genericFields(message: string, model: any, schema: Schema): void {
  const definition = schema.messages[message];
  if (!definition) throw Error('Unknown API schema');
  for (const field of definition.fields) {
    const value = model[field.name], rule = rules[message]?.[field.name] ?? {};
    const absent = value === undefined || value === '' || value === 0 || value === '0' || value === false || (Array.isArray(value) && value.length === 0);
    if (rule.required && absent) reject(rule.requiredReason ?? 'missing_field', message + '.' + field.name);
    if (absent && !rule.required) continue;
    const values = field.repeated ? value : [value], seen = new Set<string>(); let previous: string | undefined;
    for (const item of values) {
      if (rule.bytes !== undefined) {
        const length = field.type === 'bytes' ? item.length / 2 : Buffer.byteLength(item);
        if (length !== rule.bytes) reject(rule.reason ?? 'invalid_length', message + '.' + field.name);
      }
      if (rule.syntax === 'uuid') uuid(item, message + '.' + field.name);
      else if (rule.syntax === 'url') { try { absoluteUrl(item); } catch { reject('invalid_url', message + '.' + field.name); } }
      else if (rule.syntax === 'user-code' && !/^[0-9A-HJKMNP-TV-Z]{4}-[0-9A-HJKMNP-TV-Z]{4}$/.test(item)) reject('invalid_user_code', message + '.' + field.name);
      else if (rule.syntax === 'token') base64Bytes(item, message + '.' + field.name, true);
      else if (rule.syntax === 'capability' && !/^(?:[a-z0-9_-]+\.)+v[1-9][0-9]*$/.test(item)) reject('invalid_name', message + '.' + field.name);
      if (rule.text !== undefined && (Buffer.byteLength(item) < rule.text || /[\u0000-\u001f\u007f-\u009f]/u.test(item))) reject('invalid_text', message + '.' + field.name);
      if (rule.min !== undefined && BigInt(item) < rule.min || rule.max !== undefined && BigInt(item) > rule.max || rule.equals !== undefined && item !== rule.equals) reject(rule.reason ?? 'value_out_of_range', message + '.' + field.name);
      if (rule.unique && seen.has(JSON.stringify(item))) reject('duplicate_element', message + '.' + field.name);
      seen.add(JSON.stringify(item));
      if (rule.ascending && previous !== undefined && item <= previous) reject(rule.ascendingReason ?? 'duplicate_element', message + '.' + field.name);
      previous = item;
      if (schema.messages[field.type] && field.type !== 'Timestamp') genericFields(field.type, item, schema);
    }
  }
}
export function genericContext(message: string, model: any, context: any): void {
  if (message === 'GetGameResponse' && model.game.game_id !== context.gameId) reject('response_identity_mismatch', 'GameSummary.game_id');
  if (message === 'GetVersionResponse' && model.version.version_id !== context.versionId) reject('response_identity_mismatch', 'VersionSummary.version_id');
  if (message === 'ListVersionsResponse') for (const version of model.versions) if (version.game_id !== context.gameId) reject('response_identity_mismatch', 'VersionSummary.game_id');
}
export function versionDecision(server: any, client: any): string {
  if (server.major !== client.major) return 'protocol_unsupported';
  if (server.revision < client.min_server_revision) return 'server_too_old';
  if (client.revision < server.min_client_revision) return 'client_too_old';
  return 'compatible';
}
export function discoverySemantics(model: any, schema: Schema): void {
  if (model.discovery !== 1 || model.product !== 'impluvium') reject('not_an_impluvium_server');
  genericFields('Discovery', model, schema);
  if (!model.manifest_formats.includes(3)) reject('protocol_unsupported', 'Discovery.manifest_formats');
}
