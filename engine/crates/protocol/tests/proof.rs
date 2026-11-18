// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: 0BSD

use buffa::Message;
use impluvium_protocol::{
    decode::{self, Diagnostic, InputLimit, Reason},
    envelope,
    keys::{Device, KeyInput, SigningKey, TrustedKeySet},
    proof::{
        self, PairingContext, SessionContext, pairing,
        session::{self, Time},
        user_code::{self, UserCode},
    },
    proto::impluvium::{
        signed::v1::{Envelope, KeyRole},
        v2::SessionProof,
    },
};

const INSTANCE: &str = "33333333-3333-3333-3333-333333333333";
const DEVICE: &str = "44444444-4444-4444-4444-444444444444";
const OTHER: &str = "55555555-5555-5555-5555-555555555555";
fn trusted(key: &SigningKey<Device>) -> TrustedKeySet<Device> {
    TrustedKeySet::new(&[KeyInput {
        public_key: &key.public_key(),
        role: KeyRole::Device,
        ticket_status: None,
    }])
    .unwrap()
}
fn time(seconds: i64, nanos: u32) -> Time {
    Time { seconds, nanos }
}
fn diagnostic(reason: Reason, field: &'static str) -> Diagnostic {
    Diagnostic { reason, field }
}
fn rejects<T>(result: Result<T, Diagnostic>, reason: Reason, field: &'static str) {
    assert_eq!(result.err(), Some(diagnostic(reason, field)));
}

#[test]
fn issued_proofs_round_trip_and_require_the_callers_bindings_and_key() {
    let key = SigningKey::from_seed(&[7; 32]);
    let trust = trusted(&key);
    let context = SessionContext {
        instance_id: INSTANCE,
        device_id: DEVICE,
        challenge: &[5; 32],
    };
    let bytes = proof::issue_session(&context, &key).unwrap();
    let verified = proof::verify_session(&bytes, &trust, &context).unwrap();
    assert_eq!(verified.instance_id(), INSTANCE);
    assert_eq!(verified.device_id(), DEVICE);
    for (changed, reason, field) in [
        (
            SessionContext {
                instance_id: OTHER,
                ..context
            },
            Reason::proof_instance_mismatch,
            "SessionProof.instance_id",
        ),
        (
            SessionContext {
                device_id: OTHER,
                ..context
            },
            Reason::proof_device_mismatch,
            "SessionProof.device_id",
        ),
        (
            SessionContext {
                challenge: &[6; 32],
                ..context
            },
            Reason::proof_challenge_mismatch,
            "SessionProof.challenge",
        ),
    ] {
        rejects(
            proof::verify_session(&bytes, &trust, &changed),
            reason,
            field,
        );
    }
    let pairing = PairingContext {
        instance_id: INSTANCE,
        pairing_id: &[8; 16],
    };
    let pair_bytes = proof::issue_pairing(&pairing, &key).unwrap();
    let verified = proof::verify_pairing(&pair_bytes, &trust, &pairing).unwrap();
    assert_eq!(verified.instance_id(), INSTANCE);
    for (changed, reason, field) in [
        (
            PairingContext {
                instance_id: OTHER,
                ..pairing
            },
            Reason::proof_instance_mismatch,
            "PairingProof.instance_id",
        ),
        (
            PairingContext {
                pairing_id: &[9; 16],
                ..pairing
            },
            Reason::proof_pairing_mismatch,
            "PairingProof.pairing_id",
        ),
    ] {
        rejects(
            proof::verify_pairing(&pair_bytes, &trust, &changed),
            reason,
            field,
        );
    }
    let other_key = SigningKey::from_seed(&[8; 32]);
    rejects(
        proof::verify_pairing(&pair_bytes, &trusted(&other_key), &pairing),
        Reason::no_trusted_signature,
        "",
    );
    rejects(
        proof::verify_session(&bytes, &trusted(&other_key), &context),
        Reason::no_trusted_signature,
        "",
    );
    rejects(
        proof::verify_session(&pair_bytes, &trust, &context),
        Reason::unexpected_payload_type,
        "Envelope.payload_type",
    );
    rejects(
        proof::verify_pairing(&bytes, &trust, &pairing),
        Reason::unexpected_payload_type,
        "Envelope.payload_type",
    );
}

#[test]
fn changing_the_type_without_resigning_fails_before_payload_decoding() {
    let key = SigningKey::from_seed(&[7; 32]);
    let context = SessionContext {
        instance_id: INSTANCE,
        device_id: DEVICE,
        challenge: &[5; 32],
    };
    let bytes = proof::issue_session(&context, &key).unwrap();
    let mut env: Envelope =
        decode::decode_binary(&bytes, InputLimit::envelope(envelope::SESSION_PROOF)).unwrap();
    env.payload_type = envelope::PAIRING_PROOF.as_str().into();
    rejects(
        proof::verify_pairing(
            &env.encode_to_vec(),
            &trusted(&key),
            &PairingContext {
                instance_id: INSTANCE,
                pairing_id: &[8; 16],
            },
        ),
        Reason::invalid_signature,
        "Signature.sig",
    );
}

#[test]
fn producers_refuse_invalid_context_and_payload_fields_precede_bindings() {
    let key = SigningKey::from_seed(&[7; 32]);
    rejects(
        proof::issue_session(
            &SessionContext {
                instance_id: "bad",
                device_id: DEVICE,
                challenge: &[0; 32],
            },
            &key,
        ),
        Reason::invalid_uuid,
        "SessionProof.instance_id",
    );
    rejects(
        proof::issue_pairing(
            &PairingContext {
                instance_id: "bad",
                pairing_id: &[0; 16],
            },
            &key,
        ),
        Reason::invalid_uuid,
        "PairingProof.instance_id",
    );
    let p = SessionContext {
        instance_id: INSTANCE,
        device_id: DEVICE,
        challenge: &[0; 32],
    };
    let bytes = proof::issue_session(&p, &key).unwrap();
    let mut env: Envelope =
        decode::decode_binary(&bytes, InputLimit::envelope(envelope::SESSION_PROOF)).unwrap();
    let mut payload: SessionProof =
        decode::decode_binary(&env.payload, InputLimit::payload(4096)).unwrap();
    payload.device_id = "bad".into();
    payload.challenge.clear();
    env.payload = payload.encode_to_vec();
    env.signatures =
        vec![envelope::sign_payload(envelope::SESSION_PROOF, &env.payload, &key).unwrap()];
    rejects(
        proof::verify_session(
            &env.encode_to_vec(),
            &trusted(&key),
            &SessionContext {
                instance_id: OTHER,
                ..p
            },
        ),
        Reason::invalid_uuid,
        "SessionProof.device_id",
    );
    // Unknown fields, such as an old timestamp, cannot replace challenge field 3.
    payload.device_id = DEVICE.into();
    env.payload = payload.encode_to_vec();
    env.payload.extend_from_slice(&[0x20, 1]);
    env.signatures =
        vec![envelope::sign_payload(envelope::SESSION_PROOF, &env.payload, &key).unwrap()];
    rejects(
        proof::verify_session(&env.encode_to_vec(), &trusted(&key), &p),
        Reason::invalid_challenge_length,
        "SessionProof.challenge",
    );
}

#[test]
fn user_codes_have_exact_normalization_grouping_and_alphabet() {
    let code = UserCode::parse(" --k7 qm-2xzb-- ").unwrap();
    assert_eq!(code.grouped(), "K7QM-2XZB");
    assert!(code.matches(&UserCode::parse("K7QM2XZB").unwrap()));
    assert!(!code.matches(&UserCode::parse("K7QM2XZ0").unwrap()));
    for input in [
        "",
        "K7QM2XZ",
        "K7QM2XZBB",
        "ILOU0000",
        "k7qm\t2xzb",
        "K7QM\n2XZB",
        "K7QM–2XZB",
        "Ｋ7QM2XZB",
        "K7QM\u{a0}2XZB",
        "K7QM\0XZB",
    ] {
        rejects(UserCode::parse(input), Reason::invalid_user_code, "");
    }
    assert_eq!(user_code::normalize("ILOU\tß"), "ILOU\tß");
    for byte in 0..=u8::MAX {
        let generated = UserCode::from_random(&[byte; 8]).grouped();
        assert_eq!(generated.as_bytes()[4], b'-');
        assert_eq!(
            generated.as_bytes()[0],
            user_code::ALPHABET[usize::from(byte & 31)]
        );
        assert!(UserCode::parse(&generated).is_ok());
    }
}

fn sessions() -> session::State {
    session::State {
        device: session::Device {
            device_id: DEVICE.into(),
            registered: true,
            revoked: false,
            activity_at: time(0, 0),
            idle_seconds: 180,
        },
        challenges: Vec::new(),
        capacity: 1,
    }
}
fn begun() -> session::State {
    let (state, result) = sessions().begin(time(0, 0), [1; 32]);
    assert!(result.is_ok());
    assert_eq!(state.challenges[0].expires_at, time(60, 0));
    state
}
fn complete(state: &session::State, now: Time) -> (session::State, Result<(), Diagnostic>) {
    state.complete(&[1; 32], Ok(()), now, |current| (current, now))
}

#[test]
fn consumed_challenges_occupy_capacity_until_expiry_and_are_never_reopened() {
    let (state, result) = complete(&begun(), time(1, 0));
    assert!(result.is_ok());
    assert_eq!(state.device.activity_at, time(1, 0));
    let (unchanged, result) = state.begin(time(59, 999_999_999), [2; 32]);
    rejects(result, Reason::rate_limited, "");
    assert_eq!(unchanged.challenges.len(), 1);
    assert!(unchanged.challenges[0].consumed);
    rejects(
        complete(&state, time(59, 999_999_999)).1,
        Reason::challenge_consumed,
        "CompleteSessionRequest.challenge",
    );
    rejects(
        complete(&state, time(60, 0)).1,
        Reason::challenge_expired,
        "CompleteSessionRequest.challenge",
    );
    let (cleaned, result) = state.begin(time(60, 0), [2; 32]);
    assert!(result.is_ok());
    assert_eq!(cleaned.challenges.len(), 1);
    rejects(
        complete(&cleaned, time(60, 0)).1,
        Reason::challenge_unknown,
        "CompleteSessionRequest.challenge",
    );
}

#[test]
fn full_store_preserves_open_challenges_and_begin_does_not_disclose_revocation_or_idle_state() {
    let state = begun();
    let (next, result) = state.begin(time(1, 0), [2; 32]);
    rejects(result, Reason::rate_limited, "");
    assert_eq!(next.challenges.len(), 1);
    assert!(!next.challenges[0].consumed);
    assert!(complete(&next, time(2, 0)).1.is_ok());
    let mut state = sessions();
    state.device.revoked = true;
    assert!(state.begin(time(200, 0), [1; 32]).1.is_ok());
    state.device.registered = false;
    state.capacity = 0;
    rejects(
        state.begin(time(200, 0), [1; 32]).1,
        Reason::unknown_device,
        "BeginSessionRequest.device_id",
    );
}

#[test]
fn restart_can_preserve_or_forget_but_cannot_reopen_consumed_challenges() {
    for consume in [false, true] {
        let state = begun();
        let state = if consume {
            complete(&state, time(1, 0)).0
        } else {
            state
        };
        let preserved = state.restart(session::Restart::Preserve);
        let result = complete(&preserved, time(2, 0)).1;
        if consume {
            rejects(
                result,
                Reason::challenge_consumed,
                "CompleteSessionRequest.challenge",
            );
        } else {
            assert!(result.is_ok());
        }
        let forgotten = state.restart(session::Restart::Forget);
        rejects(
            complete(&forgotten, time(2, 0)).1,
            Reason::challenge_unknown,
            "CompleteSessionRequest.challenge",
        );
    }
}

#[test]
fn invalid_signature_and_binding_never_reach_the_atomic_step() {
    let key = SigningKey::from_seed(&[7; 32]);
    let context = SessionContext {
        instance_id: INSTANCE,
        device_id: DEVICE,
        challenge: &[1; 32],
    };
    let valid = proof::issue_session(&context, &key).unwrap();
    let mut invalid: Envelope =
        decode::decode_binary(&valid, InputLimit::envelope(envelope::SESSION_PROOF)).unwrap();
    // Keep the trusted key ID and break S's encoding, so authentication returns
    // invalid_signature rather than the unrelated no_trusted_signature reason.
    invalid.signatures[0].sig[63] ^= 0x80;
    let invalid = invalid.encode_to_vec();
    rejects(
        proof::verify_session(&invalid, &trusted(&key), &context),
        Reason::invalid_signature,
        "Signature.sig",
    );
    let observations = [
        proof::verify_session(&invalid, &trusted(&key), &context).map(|_| ()),
        proof::verify_session(
            &valid,
            &trusted(&key),
            &SessionContext {
                instance_id: OTHER,
                ..context
            },
        )
        .map(|_| ()),
    ];
    for observation in observations {
        let (state, result) = begun().complete(&[1; 32], observation, time(1, 0), |_| {
            panic!("invalid proof reached atomic step")
        });
        assert!(result.is_err());
        assert!(!state.challenges[0].consumed);
        assert_eq!(state.device.activity_at, time(0, 0));
        assert!(complete(&state, time(2, 0)).1.is_ok());
    }
}

#[test]
fn session_atomic_rechecks_expiry_revocation_idle_and_the_winning_completion() {
    let state = begun();
    let (expired, result) = state.complete(&[1; 32], Ok(()), time(59, 999_999_999), |current| {
        (current, time(60, 0))
    });
    rejects(
        result,
        Reason::challenge_expired,
        "CompleteSessionRequest.challenge",
    );
    assert!(!expired.challenges[0].consumed);
    let (revoked, result) = state.complete(&[1; 32], Ok(()), time(1, 0), |mut current| {
        current.device.revoked = true;
        (current, time(1, 0))
    });
    rejects(
        result,
        Reason::device_revoked,
        "CompleteSessionRequest.device_id",
    );
    assert!(revoked.device.revoked);
    assert!(!revoked.challenges[0].consumed);
    assert_eq!(revoked.device.activity_at, time(0, 0));
    let mut state = begun();
    state.device.idle_seconds = 10;
    let (idle, result) = state.complete(&[1; 32], Ok(()), time(9, 999_999_999), |current| {
        (current, time(10, 0))
    });
    rejects(
        result,
        Reason::device_expired,
        "CompleteSessionRequest.device_id",
    );
    assert!(!idle.challenges[0].consumed);
    assert_eq!(idle.device.activity_at, time(0, 0));
    let (loser, result) = state.complete(&[1; 32], Ok(()), time(1, 0), |current| {
        let (winner, result) = complete(&current, time(1, 0));
        assert!(result.is_ok());
        (winner, time(1, 0))
    });
    rejects(
        result,
        Reason::challenge_consumed,
        "CompleteSessionRequest.challenge",
    );
    assert!(loser.challenges[0].consumed);
    assert_eq!(loser.device.activity_at, time(1, 0));
}

fn poll(state: &pairing::State, now: Time) -> (pairing::State, Result<(), Diagnostic>) {
    state.poll(Ok(()), now, |current| (current, now))
}
#[test]
fn pending_polling_remains_repeatable_and_slow_down_uses_and_retains_the_previous_interval() {
    let mut state = pairing::State::start(time(0, 0)).unwrap();
    assert_eq!(state.expires_at, time(600, 0));
    for seconds in [0, 5, 10] {
        let (next, result) = poll(&state, time(seconds, 0));
        rejects(
            result,
            Reason::pairing_pending,
            "PollPairingRequest.pairing_id",
        );
        assert_eq!(next.last_accepted_poll, Some(time(seconds, 0)));
        assert_eq!(next.interval_seconds, 5);
        assert!(!next.device_registered);
        state = next;
    }
    for (seconds, interval) in [(11, 10), (20, 15)] {
        let (next, result) = poll(&state, time(seconds, 0));
        rejects(result, Reason::slow_down, "PollPairingRequest.pairing_id");
        assert_eq!(next.interval_seconds, interval);
        assert_eq!(next.last_accepted_poll, Some(time(seconds, 0)));
        state = next;
    }
    let (state, result) = poll(&state, time(35, 0));
    rejects(
        result,
        Reason::pairing_pending,
        "PollPairingRequest.pairing_id",
    );
    assert_eq!(state.interval_seconds, 15);
}

#[test]
fn pairing_rejections_before_poll_acceptance_do_not_change_poll_time() {
    let initial = pairing::State::start(time(0, 0)).unwrap();
    let invalid = Err(diagnostic(Reason::invalid_signature, "Signature.sig"));
    let (state, result) = initial.poll(invalid, time(1, 0), |_| {
        panic!("invalid proof reached completion")
    });
    assert_eq!(result, invalid);
    assert!(state.last_accepted_poll.is_none());
    assert_eq!(state.interval_seconds, 5);
    for (mut state, expected) in [
        (initial.clone(), Reason::pairing_expired),
        (initial.clone(), Reason::pairing_denied),
        (initial, Reason::pairing_unknown),
    ] {
        let now = if expected == Reason::pairing_expired {
            time(600, 0)
        } else {
            time(1, 0)
        };
        if expected == Reason::pairing_denied {
            state.denied = true;
        }
        if expected == Reason::pairing_unknown {
            state.exists = false;
        }
        let (next, result) = poll(&state, now);
        rejects(result, expected, "PollPairingRequest.pairing_id");
        assert!(next.last_accepted_poll.is_none());
        assert!(!next.device_registered);
    }
}

#[test]
fn pairing_completion_rechecks_state_and_retains_accepted_poll_time_when_it_loses() {
    for reason in [
        Reason::pairing_expired,
        Reason::pairing_unknown,
        Reason::pairing_denied,
        Reason::pairing_pending,
    ] {
        let mut state = pairing::State::start(time(0, 0)).unwrap();
        state.approved = true;
        let now = time(599, 999_999_999);
        let (next, result) = state.poll(Ok(()), now, |mut current| {
            if reason == Reason::pairing_unknown {
                current.completed = true;
            }
            if reason == Reason::pairing_denied {
                current.denied = true;
            }
            if reason == Reason::pairing_pending {
                current.approved = false;
            }
            let atomic_time = if reason == Reason::pairing_expired {
                time(600, 0)
            } else {
                now
            };
            (current, atomic_time)
        });
        rejects(result, reason, "PollPairingRequest.pairing_id");
        assert_eq!(next.last_accepted_poll, Some(now));
        assert!(!next.device_registered);
        assert_eq!(next.completed, reason == Reason::pairing_unknown);
    }
    let mut state = pairing::State::start(time(0, 0)).unwrap();
    state.approved = true;
    let (state, result) = poll(&state, time(599, 999_999_999));
    assert!(result.is_ok());
    assert!(state.completed && state.device_registered);
    rejects(
        poll(&state, time(600, 0)).1,
        Reason::pairing_unknown,
        "PollPairingRequest.pairing_id",
    );
}

#[test]
fn competing_pairing_completions_register_exactly_once() {
    let mut state = pairing::State::start(time(0, 0)).unwrap();
    state.approved = true;
    let (state, result) = state.poll(Ok(()), time(0, 0), |current| {
        // Another poll reaches its completion after this poll's step 11.
        // Its accepted poll runs at the next allowed interval.
        let (winner, result) = poll(&current, time(5, 0));
        assert!(result.is_ok());
        (winner, time(5, 0))
    });
    assert_eq!(
        result,
        Err(diagnostic(
            Reason::pairing_unknown,
            "PollPairingRequest.pairing_id",
        ))
    );
    assert!(state.completed && state.device_registered);
    assert_eq!(state.last_accepted_poll, Some(time(5, 0)));
}

#[test]
fn separate_challenges_and_devices_do_not_share_consumption() {
    let mut state = begun();
    state.capacity = 2;
    let (state, result) = state.begin(time(1, 0), [2; 32]);
    assert!(result.is_ok());
    let (state, result) = complete(&state, time(2, 0));
    assert!(result.is_ok());
    assert!(state.challenges[0].consumed);
    assert!(!state.challenges[1].consumed);
    let (mut state, result) = state.complete(&[2; 32], Ok(()), time(3, 0), |s| (s, time(3, 0)));
    assert!(result.is_ok());
    state.challenges[0].device_id = OTHER.into();
    assert_eq!(
        complete(&state, time(4, 0)).1,
        Err(diagnostic(
            Reason::challenge_unknown,
            "CompleteSessionRequest.challenge",
        ))
    );
}
