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
  let candidate:Map<string,any>|undefined, candidateApplied=0n, stale=false;
  let modelEpoch=input.modelEpoch??epoch,candidateEpoch:string|null=null;
  for (const old of input.entries ?? []) entries.set(old.gameId, structuredClone(old));
  const merge = (entry: any, version: bigint, target=entries) => {
    const previous = target.get(entry.gameId);
    if (!previous || version > BigInt(previous.versionU64)) target.set(entry.gameId, { gameId: entry.gameId, versionU64: String(version), deleted: !!entry.deleted, name: entry.name ?? '' });
  };
  for (const operation of input.operations) {
    if (operation.kind === 'connected') {
      if (operation.epoch !== epoch) { epoch = operation.epoch; applied = 0n; candidate=new Map();candidateEpoch=epoch; candidateApplied=0n; buffered.length = 0; rebuild = true; decisions.push('new-epoch-rebuild'); }
      continue;
    }
    if(operation.kind==='begin-rebuild'){candidate=new Map();candidateEpoch=epoch;candidateApplied=0n;rebuild=true;decisions.push('rebuild-started');continue;}
    if(operation.kind==='fail-rebuild'){candidate=undefined;candidateEpoch=null;buffered.length=0;stale=true;rebuild=true;decisions.push('rebuild-failed');continue;}
    if(operation.kind==='complete-rebuild'){
      if(!candidate)throw Error('missing_rebuild_candidate');
      let progress=true;while(progress){progress=false;for(let i=0;i<buffered.length;i++){const event=buffered[i];if(BigInt(event.seqU64)<=candidateApplied){buffered.splice(i--,1);continue;}if(BigInt(event.prevU64)<=candidateApplied){if(event.entry)merge(event.entry,BigInt(event.seqU64),candidate);candidateApplied=BigInt(event.seqU64);buffered.splice(i,1);progress=true;break;}}}
      if(buffered.length){candidate=undefined;candidateEpoch=null;stale=true;rebuild=true;decisions.push('rebuild-gap');continue;}
      entries.clear();for(const[key,value]of candidate)entries.set(key,value);applied=candidateApplied;modelEpoch=candidateEpoch;candidate=undefined;candidateEpoch=null;rebuild=false;stale=false;decisions.push('rebuild-replaced');continue;
    }
    if (operation.kind === 'page') {
      if (operation.epoch !== epoch) { candidate=new Map();candidateEpoch=epoch;candidateApplied=0n;buffered.length=0;rebuild = true; decisions.push('snapshot-epoch-rebuild'); continue; }
      if(rebuild&&!candidate){candidate=new Map();candidateEpoch=epoch;}
      if (operation.first) {if(candidate)candidateApplied=BigInt(operation.snapshotU64);else applied = BigInt(operation.snapshotU64);}
      for (const entry of operation.entries) merge(entry, BigInt(entry.versionU64),candidate??entries);
      decisions.push('page-merged'); continue;
    }
    if(candidate&&operation.kind==='data'){buffered.push(operation);decisions.push('buffered-building');continue;}
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
  const buildObservations:Record<string,any>={stale,modelEpoch,candidateEpoch,candidateEntries:candidate?[...candidate.values()]:[],candidateAppliedU64:String(candidateApplied)};
  const selected=input.observeBuild===true?Object.keys(buildObservations):Array.isArray(input.observeBuild)?input.observeBuild:[];
  if(selected.some((key:any)=>typeof key!=='string'||!Object.hasOwn(buildObservations,key))||new Set(selected).size!==selected.length)throw Error('invalid_build_observation');
  return { epoch, appliedU64: String(applied), rebuild, buffered: buffered.length, decisions,
    ...Object.fromEntries(selected.map((key:string)=>[key,buildObservations[key]])),
    entries: [...entries.values()].sort((a, b) => a.gameId < b.gameId ? -1 : a.gameId > b.gameId ? 1 : 0) };
}
