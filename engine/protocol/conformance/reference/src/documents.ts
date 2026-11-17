// SPDX-License-Identifier: 0BSD
import { digest, fromHex, ed25519Verify, pae } from './primitives.ts';
import { ed25519PointProfile } from './ed25519.ts';
import { reject, scanBinary } from './profile.ts';
import type { Schema } from './profile.ts';
import { basePath, collisionKey, windowsComponent, windowsTree } from './paths.ts';
import type { UnicodeMaps } from './paths.ts';
export const registry = {
  manifest: { type: 'application/vnd.impluvium.manifest.v3+protobuf', message: 'Manifest', role: 'content', payload: 536870912, whole: 536887296 },
  descriptor: { type: 'application/vnd.impluvium.release.v1+protobuf', message: 'ReleaseDescriptor', role: 'content', payload: 1048576, whole: 1064960 },
  ticket: { type: 'application/vnd.impluvium.depot-ticket.v1+protobuf', message: 'DepotTicketClaims', role: 'depot-ticket', payload: 4096, whole: 4096 },
  session: { type: 'application/vnd.impluvium.session-proof.v1+protobuf', message: 'SessionProof', role: 'device', payload: 4096, whole: 4096 },
  pairing: { type: 'application/vnd.impluvium.pairing-proof.v1+protobuf', message: 'PairingProof', role: 'device', payload: 4096, whole: 4096 }
};
export type Purpose = keyof typeof registry;
export function uuid(value: string, field: string): void {
  if (!/^[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}$/.test(value) || value === '00000000-0000-0000-0000-000000000000') reject('invalid_uuid', field);
}
function features(values: string[], field: string): void {
  for (const value of values) if (!/^(?:[a-z0-9_-]+\.)+v[1-9][0-9]*$/.test(value) || Buffer.byteLength(value) > 128) reject('invalid_name', field);
  for (let i = 1; i < values.length; i++) if (Buffer.compare(Buffer.from(values[i - 1]), Buffer.from(values[i])) >= 0) reject('unsorted_required_features', field);
  if (values.length) reject('unknown_required_feature', field);
}
function display(value: string, minimum: number, maximum: number, reason: string, field: string): void {
  if (Buffer.byteLength(value) < minimum || Buffer.byteLength(value) > maximum || /[\u0000-\u001f\u007f-\u009f]/u.test(value)) reject(reason, field);
}
const u64max = (1n << 64n) - 1n;
export function manifestSemantics(model: any, unicode: UnicodeMaps, context: any = {}, blake3?: (bytes: Uint8Array) => Uint8Array): { warnings: string[] } {
  if (!model.format) reject('missing_field', 'Manifest.format');
  if (model.format !== 3) reject('unsupported_manifest_format', 'Manifest.format');
  uuid(model.game_id, 'Manifest.game_id'); uuid(model.version_id, 'Manifest.version_id');
  if (!model.created_at) reject('missing_field', 'Manifest.created_at');
  if (!model.header) reject('missing_header', 'Manifest.header');
  if (!model.header.hash_algorithm) reject('missing_field', 'Header.hash_algorithm');
  if (![1, 2].includes(model.header.hash_algorithm)) reject('unsupported_hash_algorithm', 'Header.hash_algorithm');
  if (!model.header.chunking) reject('missing_chunking', 'Header.chunking');
  if (!model.header.chunking.algorithm) reject('missing_field', 'Chunking.algorithm');
  features(model.required_features, 'Manifest.required_features');
  let previous = '';
  for (const chunk of model.chunks) {
    if (chunk.id.length !== 64) reject('invalid_chunk_id', 'Chunk.id');
    if (BigInt(chunk.size) === 0n) reject('chunk_size_zero', 'Chunk.size');
    if (BigInt(chunk.size) > 16777216n) reject('chunk_size_too_large', 'Chunk.size');
    if (previous && chunk.id <= previous) reject('unsorted_chunks', 'Manifest.chunks'); previous = chunk.id;
  }
  previous = ''; const paths = new Set<string>(), referenced = new Set<number>(); let total = 0n;
  for (const entry of model.entries) {
    basePath(entry.path, 'Entry.path');
    if (previous && Buffer.compare(Buffer.from(entry.path), Buffer.from(previous)) <= 0) reject('unsorted_entries', 'Manifest.entries');
    const components = entry.path.split('/');
    for (let i = 1; i < components.length; i++) if (paths.has(components.slice(0, i).join('/'))) reject('path_parent_conflict', 'Entry.path');
    previous = entry.path; paths.add(entry.path);
    if (![1, 2, 3].includes(entry.type)) reject('invalid_entry_type', 'Entry.type');
    const regular = entry.type === 1, size = BigInt(entry.size);
    if (!regular && size !== 0n) reject('invalid_entry_fields', 'Entry.size');
    if (regular ? entry.hash.length !== 64 : entry.hash.length !== 0) reject(regular ? 'invalid_hash_length' : 'invalid_entry_fields', 'Entry.hash');
    if (entry.type !== 2 && entry.symlink_target.length) reject('invalid_entry_fields', 'Entry.symlink_target');
    if ((!regular || size === 0n) && entry.chunk_indexes.length) reject('invalid_entry_fields', 'Entry.chunk_indexes');
    if (regular && size > 0n && !entry.chunk_indexes.length) reject('invalid_entry_fields', 'Entry.chunk_indexes');
    if (entry.mode > 511 || (entry.type === 2 && entry.mode !== 0)) reject('invalid_mode', 'Entry.mode');
    if (regular && size === 0n) {
      if (model.header.hash_algorithm === 2 && !blake3) throw Error('BLAKE3 semantic gate requires container dependency');
      const empty = model.header.hash_algorithm === 2 ? Buffer.from(blake3!(Buffer.alloc(0))) : digest('sha256', Buffer.alloc(0));
      if (entry.hash !== empty.toString('hex')) reject('empty_file_hash_mismatch', 'Entry.hash');
    }
    if (regular && size > 0n) {
      let sum = 0n;
      // Index validity precedes summation for all indexes, as stated by 10.1 step 7.
      for (const index of entry.chunk_indexes) if (index >= model.chunks.length) reject('chunk_index_out_of_range', 'Entry.chunk_indexes');
      for (const index of entry.chunk_indexes) { referenced.add(index); sum += BigInt(model.chunks[index].size); if (sum > u64max) reject('size_overflow'); }
      if (sum !== size) reject('chunk_size_mismatch', 'Entry.size');
      if (entry.chunk_indexes.length === 1 && entry.hash !== model.chunks[entry.chunk_indexes[0]].id) reject('single_chunk_hash_mismatch', 'Entry.hash');
    }
    if (entry.type === 2) basePath(entry.symlink_target, 'Entry.symlink_target', entry.path);
    if (regular) total += size;
  }
  if (total > u64max) reject('size_overflow');
  if (total !== BigInt(model.total_size)) reject('total_size_mismatch', 'Manifest.total_size');
  for (let index = 0; index < model.chunks.length; index++) if (!referenced.has(index)) reject('unreferenced_chunk', 'Manifest.chunks');
  return { warnings: context.windows ? windowsTree(model.entries, unicode, context.windows) : [] };
}
function command(model: any, platform: number, unicode: UnicodeMaps): void {
  basePath(model.executable, 'Command.executable');
  for (const arg of model.args) if (arg.includes('\0')) reject('invalid_argument_text', 'Command.args');
  if (model.working_dir) basePath(model.working_dir, 'Command.working_dir');
  for (const pair of model.env) {
    display(pair.key, 1, 128, 'invalid_env_name', 'Command.env');
    if (pair.key.includes('=')) reject('invalid_env_name', 'Command.env');
    if (pair.value.includes('\0')) reject('invalid_env_value', 'Command.env');
  }
  if (platform === 2) {
    const keys = new Set<string>();
    for (const pair of model.env) { const key = collisionKey(pair.key, unicode); if (keys.has(key)) reject('duplicate_env_name', 'Command.env'); keys.add(key); }
  }
}
export function descriptorSemantics(model: any, unicode: UnicodeMaps): void {
  uuid(model.game_id, 'ReleaseDescriptor.game_id'); uuid(model.version_id, 'ReleaseDescriptor.version_id');
  if (BigInt(model.revision) < 1n) reject('invalid_revision', 'ReleaseDescriptor.revision');
  if (model.manifest_id.length !== 64) reject('invalid_manifest_id', 'ReleaseDescriptor.manifest_id');
  if (BigInt(model.manifest_size) < 1n || BigInt(model.manifest_size) > 536870912n) reject('invalid_manifest_size', 'ReleaseDescriptor.manifest_size');
  if (!model.kind) reject('missing_field', 'ReleaseDescriptor.kind');
  if (![1, 2].includes(model.kind)) reject('unsupported_release_kind', 'ReleaseDescriptor.kind');
  const requires = new Set<string>();
  for (const value of model.requires) {
    uuid(value, 'ReleaseDescriptor.requires'); if (requires.has(value)) reject('duplicate_requirement', 'ReleaseDescriptor.requires');
    if (value === model.version_id) reject('self_requirement', 'ReleaseDescriptor.requires'); requires.add(value);
  }
  features(model.required_features, 'ReleaseDescriptor.required_features');
  for (const option of model.launch_options) {
    display(option.key, 1, 128, 'invalid_key', 'LaunchOption.key'); display(option.name, 1, 128, 'invalid_text', 'LaunchOption.name');
    if (!option.platform) reject('missing_field', 'LaunchOption.platform');
    if (!option.command) reject('missing_field', 'LaunchOption.command'); command(option.command, option.platform, unicode);
    if (option.emulator) {
      uuid(option.emulator.game_id, 'EmulatorRef.game_id'); display(option.emulator.launch_key, 1, 128, 'invalid_key', 'EmulatorRef.launch_key');
      for (const suggestion of option.emulator.suggestions) display(suggestion, 1, 256, 'invalid_text', 'EmulatorRef.suggestions');
    }
    for (const pair of option.hints) {
      if (!/^[\x21-\x7e]{1,128}$/.test(pair.key)) reject('invalid_hint_key', 'LaunchOption.hints');
      display(pair.value, 0, 4096, 'invalid_text', 'LaunchOption.hints');
    }
    if (option.legacy_command.includes('\0')) reject('invalid_text', 'LaunchOption.legacy_command');
    display(option.legacy_launch_id, 0, 128, 'invalid_text', 'LaunchOption.legacy_launch_id');
  }
  const launchKeys = new Set<string>();
  for (const option of model.launch_options) { const key = option.platform + '/' + option.key; if (launchKeys.has(key)) reject('duplicate_launch_key', 'ReleaseDescriptor.launch_options'); launchKeys.add(key); }
  for (const step of model.setup_steps) {
    display(step.id, 1, 128, 'invalid_key', 'SetupStep.id'); if (!step.platform) reject('missing_field', 'SetupStep.platform');
    if (!step.command) reject('missing_field', 'SetupStep.command'); command(step.command, step.platform, unicode);
    if (step.legacy_command.includes('\0')) reject('invalid_text', 'SetupStep.legacy_command');
    display(step.legacy_setup_id, 0, 128, 'invalid_text', 'SetupStep.legacy_setup_id');
  }
  const setupIds = new Set<string>();
  for (const step of model.setup_steps) { if (setupIds.has(step.id)) reject('duplicate_setup_id', 'ReleaseDescriptor.setup_steps'); setupIds.add(step.id); }
  if (model.kind === 2 && model.launch_options.length) reject('setup_only_with_launch_options', 'ReleaseDescriptor.launch_options');
  let bytes = 0;
  for (const row of [...model.launch_options, ...model.setup_steps]) {
    const c = row.command;
    bytes += [c.executable, c.working_dir, ...c.args, ...c.env.flatMap((pair: any) => [pair.key, pair.value]), row.legacy_command].reduce((sum: number, value: string) => sum + Buffer.byteLength(value), 0);
  }
  if (bytes > 262144) reject('byte_limit_exceeded', '');
}
export function descriptorContext(model: any, payload: Uint8Array, manifest: any, manifestBytes: Uint8Array, unicode: UnicodeMaps, context: any): string[] {
  if (model.version_id !== context.versionId) reject('descriptor_version_mismatch', 'ReleaseDescriptor.version_id');
  if (context.gameId !== undefined && model.game_id !== context.gameId) reject('descriptor_game_mismatch', 'ReleaseDescriptor.game_id');
  if (digest('sha256', manifestBytes).toString('hex') !== model.manifest_id) reject('manifest_id_mismatch', 'ReleaseDescriptor.manifest_id');
  if (BigInt(manifestBytes.length) !== BigInt(model.manifest_size)) reject('manifest_size_mismatch', 'ReleaseDescriptor.manifest_size');
  if (manifest.game_id !== model.game_id || manifest.version_id !== model.version_id) reject('manifest_binding_mismatch');
  if (manifest.total_size !== model.install_size) reject('install_size_mismatch', 'ReleaseDescriptor.install_size');
  if (context.knownManifestIdHex !== undefined && context.knownManifestIdHex !== model.manifest_id) reject('manifest_changed_for_version', 'ReleaseDescriptor.manifest_id');
  if (context.knownRevisionU64 !== undefined) {
    if (BigInt(model.revision) < BigInt(context.knownRevisionU64)) reject('stale_revision', 'ReleaseDescriptor.revision');
    if (model.revision === context.knownRevisionU64 && Buffer.from(payload).toString('hex') !== context.knownPayloadHex) reject('revision_conflict');
  }
  for (const row of [...model.launch_options, ...model.setup_steps]) {
    const c = row.command, entry = manifest.entries.find((entry: any) => entry.path === c.executable);
    if (!entry) reject('executable_not_found', 'Command.executable'); if (entry.type !== 1) reject('executable_not_regular', 'Command.executable');
    if (c.working_dir && !manifest.entries.some((entry: any) => (entry.path === c.working_dir && entry.type === 3) || entry.path.startsWith(c.working_dir + '/'))) reject('working_dir_not_found', 'Command.working_dir');
  }
  if ([...model.launch_options, ...model.setup_steps].some(row => row.platform === 2)) {
    const variant = context.consumerRole === 'client' ? 'installation' : context.windows === 'publication' ? 'publication' : 'installation';
    const warnings = windowsTree(manifest.entries, unicode, variant);
    for (const row of [...model.launch_options, ...model.setup_steps].filter(row => row.platform === 2)) {
      for (const field of ['executable', 'working_dir']) if (row.command[field]) for (const component of row.command[field].split('/')) windowsComponent(component, 'Command.' + field);
    }
    return warnings;
  }
  return [];
}
const ns = (stamp: any): bigint => BigInt(stamp.seconds) * 1000000000n + BigInt(stamp.nanos);
export function ticketSemantics(model: any, validKeys: any[], context: any): void {
  if (!model.iat || !model.exp) reject('missing_field');
  const lifetime = ns(model.exp) - ns(model.iat), now = ns(context.now);
  if (lifetime <= 0n || lifetime > 3600000000000n) reject('invalid_validity_window');
  if (now >= ns(model.exp)) reject('ticket_expired', 'DepotTicketClaims.exp');
  if (now < ns(model.iat)) reject('ticket_not_yet_valid', 'DepotTicketClaims.iat');
  for (const key of validKeys) if (key.status === 'retiring' && ns(model.iat) >= ns(key.switchTime)) reject('ticket_key_retired', 'DepotTicketClaims.iat');
  uuid(model.aud, 'DepotTicketClaims.aud'); if (model.aud !== context.audience) reject('wrong_audience', 'DepotTicketClaims.aud');
  uuid(model.sub, 'DepotTicketClaims.sub'); uuid(model.user_id, 'DepotTicketClaims.user_id');
  if (!model.manifest_ids.length) reject('missing_field', 'DepotTicketClaims.manifest_ids');
  let previous = '';
  for (const value of model.manifest_ids) { if (value.length !== 64) reject('invalid_manifest_id', 'DepotTicketClaims.manifest_ids'); if (previous && value <= previous) reject('unsorted_manifest_ids', 'DepotTicketClaims.manifest_ids'); previous = value; }
  if ((context.subject !== undefined && model.sub !== context.subject) || (context.userId !== undefined && model.user_id !== context.userId)) reject('wrong_subject');
  if (!model.manifest_ids.includes(context.manifestIdHex)) reject('ticket_scope', 'DepotTicketClaims.manifest_ids');
}
export function proofSemantics(model: any, purpose: string, context: any): void {
  const message = purpose === 'session' ? 'SessionProof' : 'PairingProof';
  uuid(model.instance_id, message + '.instance_id');
  if (purpose === 'session') { uuid(model.device_id, 'SessionProof.device_id'); if (model.challenge.length !== 64) reject('invalid_challenge_length', 'SessionProof.challenge'); }
  else if (model.pairing_id.length !== 32) reject('invalid_pairing_id_length', 'PairingProof.pairing_id');
  if (model.instance_id !== context.instanceId) reject('proof_instance_mismatch', message + '.instance_id');
  if (purpose === 'session') {
    if (model.device_id !== context.deviceId) reject('proof_device_mismatch', 'SessionProof.device_id');
    if (model.challenge !== context.challengeHex) reject('proof_challenge_mismatch', 'SessionProof.challenge');
  } else if (model.pairing_id !== context.pairingIdHex) reject('proof_pairing_mismatch', 'PairingProof.pairing_id');
}
// Envelope checks use the complete, cofactorless Ed25519 acceptance profile.
export function verifyEnvelope(bytes: Uint8Array, purpose: Purpose, keys: any[], schema: Schema, encodePae: typeof pae = pae): { model: any; payload: Buffer; validKeys: any[] } {
  const expected = registry[purpose];
  if (keys.length < 1 || keys.length > (purpose === 'ticket' ? 8 : 1)) reject('trusted_set_size');
  for (const key of keys) if (!ed25519PointProfile(fromHex(key.publicKeyHex)).accepted) reject('invalid_trusted_key');
  for (const key of keys) if (key.role !== expected.role) reject('role_mismatch');
  const ids = keys.map(key => digest('sha256', fromHex(key.publicKeyHex)).subarray(0, 8).toString('hex'));
  if (new Set(ids).size !== ids.length) reject('trusted_key_id_collision');
  if (purpose === 'ticket' && (keys.filter(key => key.status === 'current').length > 1 || keys.some(key => key.status === 'retiring' && !key.switchTime))) reject('invalid_ticket_key_set');
  if (bytes.length > expected.whole) reject('envelope_too_large');
  const envelope = scanBinary(bytes, 'Envelope', schema, { limit: expected.whole, envelopePayloadLimit: expected.payload });
  const payload = fromHex(envelope.payload);
  if (bytes.length - payload.length > 16384) reject('byte_limit_exceeded');
  if (!envelope.signatures.length) reject('no_signatures', 'Envelope.signatures');
  for (const signature of envelope.signatures) if (signature.key_id.length !== 16 || signature.sig.length !== 128) reject('malformed_signature_entry', 'Envelope.signatures');
  if (!/^[\x21-\x7e]{1,128}$/.test(envelope.payload_type)) reject('invalid_payload_type', 'Envelope.payload_type');
  const seen = new Set<string>();
  for (const signature of envelope.signatures) { if (seen.has(signature.key_id)) reject('duplicate_signature_key_id', 'Envelope.signatures'); seen.add(signature.key_id); }
  if (!Object.values(registry).some(row => row.type === envelope.payload_type)) reject('unknown_payload_type', 'Envelope.payload_type');
  if (envelope.payload_type !== expected.type) reject('unexpected_payload_type', 'Envelope.payload_type');
  const message = encodePae(Buffer.from(envelope.payload_type, 'utf8'), payload), validKeys = [];
  for (const signature of envelope.signatures) {
    const index = ids.indexOf(signature.key_id);
    if (index < 0) continue;
    if (!ed25519Verify(fromHex(keys[index].publicKeyHex), message, fromHex(signature.sig))) reject('invalid_signature', 'Signature.sig');
    validKeys.push(keys[index]);
  }
  if (!validKeys.length) reject('no_trusted_signature');
  const model = scanBinary(payload, expected.message, schema, { limit: expected.payload, totalReason: 'byte_limit_exceeded' });
  return { model, payload, validKeys };
}
