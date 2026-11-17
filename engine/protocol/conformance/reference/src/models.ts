// SPDX-License-Identifier: 0BSD
const ns = (value: any) => BigInt(value.seconds) * 1000000000n + BigInt(value.nanos);
export function sessionModel(input: any): any {
  const state = structuredClone(input.state), results: string[] = [];
  const check = (now: bigint, revoked: boolean): string | undefined => {
    if (revoked) return 'device_revoked';
    if (now >= ns(state.activityAt) + BigInt(state.idleSeconds) * 1000000000n) return 'device_expired';
    if (!state.challenge || state.challenge.deviceId !== state.deviceId) return 'challenge_unknown';
    if (now >= ns(state.challenge.expiresAt)) return 'challenge_expired';
    if (state.challenge.consumed) return 'challenge_consumed';
    return undefined;
  };
  for (const operation of input.operations) {
    if (operation.kind === 'restart') {
      if (operation.policy === 'unknown') state.challenge = undefined;
      else if (operation.policy !== 'preserve') throw Error('Restart policy is required model context');
      results.push('restarted'); continue;
    }
    if (operation.kind === 'begin') {
      results.push(state.storeFull ? 'rate_limited' : 'challenge-issued'); continue;
    }
    if (!operation.proofValid) { results.push('invalid_signature'); continue; }
    const early = check(ns(operation.now), state.revoked);
    if (early) { results.push(early); continue; }
    const atomic = ns(operation.atomicNow ?? operation.now), revoked = operation.atomicRevoked ?? state.revoked;
    const final = check(atomic, revoked);
    if (final) { results.push(final); continue; }
    state.challenge.consumed = true; state.activityAt = operation.atomicNow ?? operation.now;
    results.push('success');
  }
  return { results, consumed: state.challenge?.consumed ?? false, activityAt: state.activityAt, challengePresent: state.challenge !== undefined };
}
export function pairingModel(input: any): any {
  const state = structuredClone(input.state), results: string[] = [];
  for (const poll of input.polls) {
    if (!state.exists || state.completed) { results.push('pairing_unknown'); continue; }
    if (!poll.proofValid) { results.push('invalid_signature'); continue; }
    if (ns(poll.now) >= ns(state.expiresAt)) { results.push('pairing_expired'); continue; }
    if (state.denied) { results.push('pairing_denied'); continue; }
    if (state.lastPoll !== undefined && ns(poll.now) < ns(state.lastPoll) + BigInt(state.intervalSeconds) * 1000000000n) {
      state.intervalSeconds += 5; state.lastPoll = poll.now; results.push('slow_down'); continue;
    }
    state.lastPoll = poll.now;
    if (!state.approved) { results.push('pairing_pending'); continue; }
    if (ns(poll.atomicNow ?? poll.now) >= ns(state.expiresAt)) { results.push('pairing_expired'); continue; }
    if (poll.atomicDenied) { results.push('pairing_denied'); continue; }
    state.completed = true; results.push('success');
  }
  return { results, completed: state.completed, intervalSeconds: state.intervalSeconds };
}
export function libraryModel(input: any): any {
  let epoch = input.epoch, applied = BigInt(input.appliedU64), rebuild = false;
  const entries = new Map<string, any>(), buffered: any[] = [], decisions: string[] = [];
  for (const old of input.entries ?? []) entries.set(old.gameId, structuredClone(old));
  const merge = (entry: any, version: bigint) => {
    const previous = entries.get(entry.gameId);
    if (!previous || version > BigInt(previous.versionU64)) entries.set(entry.gameId, { gameId: entry.gameId, versionU64: String(version), deleted: !!entry.deleted, name: entry.name ?? '' });
  };
  for (const operation of input.operations) {
    if (operation.kind === 'connected') {
      if (operation.epoch !== epoch) { epoch = operation.epoch; applied = 0n; entries.clear(); buffered.length = 0; rebuild = true; decisions.push('new-epoch-rebuild'); }
      continue;
    }
    if (operation.kind === 'page') {
      if (operation.epoch !== epoch) { rebuild = true; decisions.push('snapshot-epoch-rebuild'); continue; }
      if (operation.first) applied = BigInt(operation.snapshotU64);
      for (const entry of operation.entries) merge(entry, BigInt(entry.versionU64));
      decisions.push('page-merged'); continue;
    }
    if (operation.kind === 'heartbeat') {
      if (BigInt(operation.seqU64) > applied) { rebuild = true; decisions.push('heartbeat-rebuild'); } else decisions.push('heartbeat-current');
      continue;
    }
    const seq = BigInt(operation.seqU64), previous = BigInt(operation.prevU64);
    if (seq <= applied) { decisions.push('duplicate'); continue; }
    if (previous > applied) { buffered.push(operation); decisions.push('held-gap'); continue; }
    if (operation.entry) merge(operation.entry, seq);
    applied = seq; decisions.push('applied');
    let progress = true;
    while (progress) {
      progress = false;
      for (let i = 0; i < buffered.length; i++) if (BigInt(buffered[i].prevU64) <= applied && BigInt(buffered[i].seqU64) > applied) {
        const event = buffered.splice(i, 1)[0]; if (event.entry) merge(event.entry, BigInt(event.seqU64)); applied = BigInt(event.seqU64); progress = true; break;
      }
    }
  }
  return { epoch, appliedU64: String(applied), rebuild, buffered: buffered.length, decisions,
    entries: [...entries.values()].sort((a, b) => a.gameId < b.gameId ? -1 : a.gameId > b.gameId ? 1 : 0) };
}
