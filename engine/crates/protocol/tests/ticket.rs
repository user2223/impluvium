// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: 0BSD

use buffa::Message;
use impluvium_protocol::{
    decode::{Diagnostic, Reason},
    envelope,
    keys::{DepotTicket, KeyInput, SigningKey, TicketStatus},
    proto::impluvium::signed::v1::{Envelope, KeyRole},
    ticket::{
        self, AcceptanceSet, Context, DepotTicketClaims, Issue, IssueError, Time,
        acceptance::{Change, ChangeError, Outcome, StateError},
        cache::{self, Cache},
    },
};
const AUD: &str = "11111111-1111-1111-1111-111111111111";
const SUB: &str = "22222222-2222-2222-2222-222222222222";
const USER: &str = "33333333-3333-3333-3333-333333333333";
fn time(seconds: i64, nanos: u32) -> Time {
    Time { seconds, nanos }
}
fn context(now: Time) -> Context<'static> {
    Context {
        now,
        audience: AUD,
        subject: Some(SUB),
        user_id: Some(USER),
    }
}
fn key(n: u8) -> SigningKey<DepotTicket> {
    SigningKey::from_seed(&[n; 32])
}
fn set(key: &SigningKey<DepotTicket>) -> AcceptanceSet {
    AcceptanceSet::new(
        1,
        &[KeyInput {
            public_key: &key.public_key(),
            role: KeyRole::DepotTicket,
            ticket_status: Some(TicketStatus::Current),
        }],
    )
    .unwrap()
}
fn claims() -> DepotTicketClaims {
    let mut c = DepotTicketClaims {
        aud: AUD.into(),
        sub: SUB.into(),
        user_id: USER.into(),
        manifest_ids: vec![vec![1; 32]],
        ..Default::default()
    };
    c.iat.modify(|t| {
        t.seconds = 100;
        t.nanos = 5;
    });
    c.exp.modify(|t| {
        t.seconds = 3700;
        t.nanos = 5;
    });
    c
}
fn signed(c: &DepotTicketClaims, keys: &[&SigningKey<DepotTicket>]) -> Vec<u8> {
    let payload = c.encode_to_vec();
    Envelope {
        payload_type: envelope::DEPOT_TICKET.as_str().into(),
        signatures: keys
            .iter()
            .map(|k| envelope::sign_payload(envelope::DEPOT_TICKET, &payload, k).unwrap())
            .collect(),
        payload,
        ..Default::default()
    }
    .encode_to_vec()
}
fn verify(c: &DepotTicketClaims, ctx: &Context<'_>) -> Result<(), Diagnostic> {
    let key = key(1);
    let set = set(&key);
    ticket::verify(&signed(c, &[&key]), set.trusted(), ctx, &[1; 32]).map(|_| ())
}
fn err(reason: Reason, field: &'static str) -> Diagnostic {
    Diagnostic { reason, field }
}

#[test]
fn exact_time_boundaries_and_joint_window_diagnostics() {
    let c = claims();
    assert!(verify(&c, &context(time(100, 5))).is_ok());
    assert!(verify(&c, &context(time(3700, 4))).is_ok());
    assert_eq!(
        verify(&c, &context(time(3700, 5))),
        Err(err(Reason::ticket_expired, "DepotTicketClaims.exp"))
    );
    assert_eq!(
        verify(&c, &context(time(100, 4))),
        Err(err(Reason::ticket_not_yet_valid, "DepotTicketClaims.iat"))
    );
    for (seconds, nanos) in [(100, 5), (99, 999_999_999), (3700, 6)] {
        let mut c = claims();
        c.exp.modify(|t| {
            t.seconds = seconds;
            t.nanos = nanos;
        });
        assert_eq!(
            verify(&c, &context(time(100, 5))),
            Err(err(Reason::invalid_validity_window, ""))
        );
    }
    let mut c = claims();
    c.exp.modify(|t| {
        t.seconds = 100;
        t.nanos = 6;
    });
    assert!(verify(&c, &context(time(100, 5))).is_ok());
    // Extreme injected times must compare without overflowing.
    assert_eq!(
        verify(&claims(), &context(time(i64::MAX, 999_999_999))),
        Err(err(Reason::ticket_expired, "DepotTicketClaims.exp"))
    );
    assert_eq!(
        verify(&claims(), &context(time(i64::MIN, 0))),
        Err(err(Reason::ticket_not_yet_valid, "DepotTicketClaims.iat"))
    );
}
#[test]
fn scan_time_semantics_binding_and_scope_precedence() {
    let mut c = claims();
    c.aud = "invalid".into();
    c.sub = "invalid".into();
    c.manifest_ids = vec![vec![1; 31]];
    assert_eq!(
        verify(&c, &context(time(3700, 5))),
        Err(err(Reason::ticket_expired, "DepotTicketClaims.exp"))
    );
    c.exp.modify(|t| t.nanos = -1);
    assert_eq!(
        verify(&c, &context(time(3700, 5))),
        Err(err(Reason::invalid_timestamp, "DepotTicketClaims.exp"))
    );
    let mut c = claims();
    c.aud = SUB.into();
    assert_eq!(
        verify(&c, &context(time(100, 5))),
        Err(err(Reason::wrong_audience, "DepotTicketClaims.aud"))
    );
    c = claims();
    c.user_id = AUD.into();
    assert_eq!(
        verify(&c, &context(time(100, 5))),
        Err(err(Reason::wrong_subject, ""))
    );
    c = claims();
    c.manifest_ids = vec![vec![2; 32], vec![1; 32]];
    assert_eq!(
        verify(&c, &context(time(100, 5))),
        Err(err(
            Reason::unsorted_manifest_ids,
            "DepotTicketClaims.manifest_ids"
        ))
    );
    c.manifest_ids = (1..=9).map(|n| vec![n; 32]).collect();
    // Element limits are a scan failure, even when the ticket is expired.
    assert_eq!(
        verify(&c, &context(time(3700, 5))),
        Err(err(
            Reason::element_limit_exceeded,
            "DepotTicketClaims.manifest_ids"
        ))
    );
    c = claims();
    c.manifest_ids = vec![vec![2; 32]];
    assert_eq!(
        verify(&c, &context(time(100, 5))),
        Err(err(Reason::ticket_scope, "DepotTicketClaims.manifest_ids"))
    );
    let key = key(1);
    let set = set(&key);
    let bytes = signed(&c, &[&key]);
    let checked = ticket::verify_bindings(&bytes, set.trusted(), &context(time(100, 5))).unwrap();
    assert_eq!(
        checked.check_scope(&[1; 32]),
        Err(err(Reason::ticket_scope, "DepotTicketClaims.manifest_ids"))
    );
}
#[test]
fn altered_claims_require_authentication_before_semantics() {
    let key = key(1);
    let set = set(&key);
    let c = claims();
    let payload = c.encode_to_vec();
    let signature = envelope::sign_payload(envelope::DEPOT_TICKET, &payload, &key).unwrap();
    let mut altered = c;
    altered.aud = "invalid".into();
    let bytes = Envelope {
        payload_type: envelope::DEPOT_TICKET.as_str().into(),
        payload: altered.encode_to_vec(),
        signatures: vec![signature],
        ..Default::default()
    }
    .encode_to_vec();
    assert_eq!(
        ticket::verify(&bytes, set.trusted(), &context(time(100, 5)), &[1; 32]).unwrap_err(),
        err(Reason::invalid_signature, "Signature.sig")
    );
}
#[test]
fn rotation_overlap_retirement_and_hard_revocation() {
    let old = key(1);
    let new = key(2);
    let initial = set(&old);
    let bytes = signed(&claims(), &[&old]);
    assert!(ticket::verify(&bytes, initial.trusted(), &context(time(100, 5)), &[1; 32]).is_ok());
    let (overlap, outcome) = initial
        .apply(Change::Switch(new.public_key()), time(101, 0), 1)
        .unwrap();
    assert_eq!(outcome, Outcome::Switched);
    assert_eq!(overlap.generation(), 2);
    assert!(ticket::verify(&bytes, overlap.trusted(), &context(time(101, 0)), &[1; 32]).is_ok());
    let (unchanged, outcome) = overlap
        .apply(Change::Remove(old.public_key()), time(3700, 999_999_999), 1)
        .unwrap();
    assert_eq!(outcome, Outcome::Wait);
    assert_eq!(unchanged.generation(), 2);
    let (after, outcome) = overlap
        .apply(Change::Remove(old.public_key()), time(3701, 0), 1)
        .unwrap();
    assert_eq!(outcome, Outcome::Removed);
    assert_eq!(
        ticket::verify(&bytes, after.trusted(), &context(time(3701, 0)), &[1; 32]).unwrap_err(),
        err(Reason::no_trusted_signature, "")
    );
    // An old signature with iat == switch is rejected, including when a valid
    // current signature is first or last in the envelope.
    let mut late = claims();
    late.iat.modify(|t| {
        t.seconds = 101;
        t.nanos = 0;
    });
    for keys in [[&old, &new], [&new, &old]] {
        let bytes = signed(&late, &keys);
        assert_eq!(
            ticket::verify(&bytes, overlap.trusted(), &context(time(101, 0)), &[1; 32])
                .unwrap_err(),
            err(Reason::ticket_key_retired, "DepotTicketClaims.iat")
        );
    }
    late.iat.modify(|t| t.nanos = 1);
    assert_eq!(
        ticket::verify(
            &signed(&late, &[&old]),
            overlap.trusted(),
            &context(time(101, 1)),
            &[1; 32]
        )
        .unwrap_err(),
        err(Reason::ticket_key_retired, "DepotTicketClaims.iat")
    );
    let (revoked, outcome) = overlap
        .apply(Change::HardRevoke(old.public_key()), time(102, 0), 1)
        .unwrap();
    assert_eq!(outcome, Outcome::Revoked);
    assert_eq!(
        ticket::verify(&bytes, revoked.trusted(), &context(time(102, 0)), &[1; 32]).unwrap_err(),
        err(Reason::no_trusted_signature, "")
    );
    assert!(
        ticket::verify(
            &signed(&claims(), &[&old, &new]),
            revoked.trusted(),
            &context(time(102, 0)),
            &[1; 32]
        )
        .is_ok()
    );
    assert_eq!(
        ticket::verify(&bytes, overlap.trusted(), &context(time(3700, 5)), &[1; 32]).unwrap_err(),
        err(Reason::ticket_expired, "DepotTicketClaims.exp")
    );
}
#[test]
fn issuance_uses_only_current_key_and_enforces_producer_lifetime() {
    let old = key(1);
    let new = key(2);
    let initial = set(&old);
    let (set, _) = initial
        .apply(Change::Switch(new.public_key()), time(100, 0), 1)
        .unwrap();
    let ids = [[1; 32], [2; 32]];
    let input = |lifetime| Issue {
        audience: AUD,
        subject: SUB,
        user_id: USER,
        manifest_ids: &ids,
        now: time(100, 5),
        lifetime_seconds: lifetime,
    };
    assert_eq!(
        ticket::issue(input(3600), &set, &old),
        Err(IssueError::NotCurrentKey)
    );
    for lifetime in [300, 3600] {
        let bytes = ticket::issue(input(lifetime), &set, &new).unwrap();
        let ticket =
            ticket::verify(&bytes, set.trusted(), &context(time(100, 5)), &ids[1]).unwrap();
        assert_eq!(ticket.claims().exp.seconds, 100 + i64::from(lifetime));
        assert_eq!(ticket.claims().exp.nanos, 5);
    }
    for lifetime in [0, 299, 3601, u32::MAX] {
        assert_eq!(
            ticket::issue(input(lifetime), &set, &new),
            Err(IssueError::InvalidLifetime)
        );
    }
    let ids = [[1; 32]; 9];
    assert_eq!(
        ticket::issue(
            Issue {
                manifest_ids: &ids,
                ..input(300)
            },
            &set,
            &new
        ),
        Err(IssueError::Claims(err(
            Reason::element_limit_exceeded,
            "DepotTicketClaims.manifest_ids"
        )))
    );
    assert_eq!(
        ticket::issue(
            Issue {
                now: time(253_402_300_799, 0),
                ..input(300)
            },
            &set,
            &new
        ),
        Err(IssueError::InvalidTime)
    );
}
#[test]
fn cache_hits_recheck_time_audience_subject_scope_and_every_retiring_key() {
    let old = key(1);
    let new = key(2);
    let initial = set(&old);
    let bytes = signed(&claims(), &[&old]);
    let (cached, hit, result) = cache::verify_cached(
        &Cache::default(),
        &bytes,
        &initial,
        &context(time(100, 5)),
        &[1; 32],
    );
    assert!(!hit);
    result.unwrap();
    let (_, hit, result) =
        cache::verify_cached(&cached, &bytes, &initial, &context(time(100, 6)), &[1; 32]);
    assert!(hit);
    result.unwrap();
    for (ctx, id, reason, field) in [
        (
            Context {
                audience: SUB,
                ..context(time(100, 6))
            },
            [1; 32],
            Reason::wrong_audience,
            "DepotTicketClaims.aud",
        ),
        (
            Context {
                subject: Some(AUD),
                ..context(time(100, 6))
            },
            [1; 32],
            Reason::wrong_subject,
            "",
        ),
        (
            Context {
                user_id: Some(AUD),
                ..context(time(100, 6))
            },
            [1; 32],
            Reason::wrong_subject,
            "",
        ),
        (
            context(time(100, 6)),
            [2; 32],
            Reason::ticket_scope,
            "DepotTicketClaims.manifest_ids",
        ),
        (
            context(time(100, 4)),
            [1; 32],
            Reason::ticket_not_yet_valid,
            "DepotTicketClaims.iat",
        ),
    ] {
        let (_, hit, result) = cache::verify_cached(&cached, &bytes, &initial, &ctx, &id);
        assert!(hit);
        assert_eq!(result.unwrap_err(), err(reason, field));
    }
    let (_, hit, result) =
        cache::verify_cached(&cached, &bytes, &initial, &context(time(3700, 5)), &[1; 32]);
    assert!(!hit);
    assert_eq!(
        result.unwrap_err(),
        err(Reason::ticket_expired, "DepotTicketClaims.exp")
    );
    let (overlap, _) = initial
        .apply(Change::Switch(new.public_key()), time(101, 0), 1)
        .unwrap();
    let (cached, hit, result) =
        cache::verify_cached(&cached, &bytes, &overlap, &context(time(101, 0)), &[1; 32]);
    assert!(!hit);
    result.unwrap();
    let (_, hit, result) =
        cache::verify_cached(&cached, &bytes, &overlap, &context(time(101, 1)), &[1; 32]);
    assert!(hit);
    result.unwrap();
    let (revoked, _) = overlap
        .apply(Change::HardRevoke(old.public_key()), time(102, 0), 1)
        .unwrap();
    let (_, hit, result) =
        cache::verify_cached(&cached, &bytes, &revoked, &context(time(102, 0)), &[1; 32]);
    assert!(!hit);
    assert_eq!(result.unwrap_err(), err(Reason::no_trusted_signature, ""));
    let mut changed = bytes.clone();
    changed[0] ^= 1;
    let (_, hit, result) = cache::verify_cached(
        &cached,
        &changed,
        &overlap,
        &context(time(102, 0)),
        &[1; 32],
    );
    assert!(!hit);
    assert!(result.is_err());
}
#[test]
fn cache_state_role_is_part_of_identity() {
    let initial = cache::State::<u8>::default();
    let identity = |role| cache::Identity {
        ticket: b"opaque",
        role,
        generation: 1,
    };
    let (state, hit, result) = initial.step(
        identity(KeyRole::DepotTicket),
        time(0, 0),
        || Ok::<_, ()>((7, time(60, 0))),
        |_| Ok(()),
    );
    assert!(!hit);
    assert_eq!(result, Ok(7));
    let (_, hit, result) = state.step(
        identity(KeyRole::Device),
        time(0, 0),
        || Err(()),
        |_| Ok(()),
    );
    assert!(!hit);
    assert_eq!(result, Err(()));
}
#[test]
fn counter_exhaustion_and_overlap_boundary_are_atomic() {
    let old = key(1);
    let new = key(2);
    let initial = AcceptanceSet::new(
        u64::MAX,
        &[KeyInput {
            public_key: &old.public_key(),
            role: KeyRole::DepotTicket,
            ticket_status: Some(TicketStatus::Current),
        }],
    )
    .unwrap();
    assert_eq!(
        initial
            .apply(Change::Switch(new.public_key()), time(i64::MAX, 0), 1)
            .unwrap_err(),
        ChangeError::State(StateError::GenerationExhausted)
    );
    assert_eq!(initial.state().keys.len(), 1);
    let state = ticket::acceptance::State {
        generation: 1,
        keys: vec![
            ticket::acceptance::Entry {
                key: 1,
                status: TicketStatus::Retiring {
                    switch_time: Some(time(100, 5)),
                },
            },
            ticket::acceptance::Entry {
                key: 2,
                status: TicketStatus::Current,
            },
        ],
    };
    assert_eq!(
        state.apply(Change::Remove(1), time(3700, 4), 1).unwrap().1,
        Outcome::Wait
    );
    assert_eq!(
        state.apply(Change::Remove(1), time(3700, 5), 1).unwrap().1,
        Outcome::Removed
    );
    assert_eq!(
        state
            .apply(Change::Remove(1), time(i64::MIN, 0), 1)
            .unwrap()
            .1,
        Outcome::Wait
    );
    assert_eq!(
        state
            .apply(Change::Remove(1), time(i64::MAX, 0), 1)
            .unwrap()
            .1,
        Outcome::Removed
    );
}
#[test]
fn binary_limit_precedes_decoding_and_base64url_expansion_is_accounted_for() {
    assert_eq!(ticket::MAX_BINARY_BYTES, 4096);
    assert_eq!(ticket::MAX_TOKEN_CHARACTERS, 5462);
    assert_eq!(ticket::MAX_AUTHORIZATION_BYTES, 5469);
    let key = key(1);
    let set = set(&key);
    let original = signed(&claims(), &[&key]);
    for length in [4096, 4097] {
        let mut padded = original.clone();
        // Unknown Envelope field 100, length-delimited, with a two-byte length.
        // It changes envelope size without changing the signed payload bytes.
        let padding = length - original.len() - 4;
        assert!((128..16_384).contains(&padding));
        padded.extend_from_slice(&[
            0xa2,
            0x06,
            (padding as u8 & 0x7f) | 0x80,
            (padding >> 7) as u8,
        ]);
        padded.resize(length, 0);
        let result = ticket::verify(&padded, set.trusted(), &context(time(100, 5)), &[1; 32]);
        if length == 4096 {
            assert!(result.is_ok());
        } else {
            assert_eq!(result.unwrap_err(), err(Reason::envelope_too_large, ""));
        }
    }
}

#[test]
fn semantic_checks_keep_the_specified_joint_and_element_order() {
    let ctx = context(time(100, 5));
    for field in ["aud", "sub", "user_id"] {
        let mut c = claims();
        match field {
            "aud" => c.aud = "invalid".into(),
            "sub" => c.sub = "invalid".into(),
            "user_id" => c.user_id = "invalid".into(),
            _ => unreachable!(),
        }
        let diagnostic_field = match field {
            "aud" => "DepotTicketClaims.aud",
            "sub" => "DepotTicketClaims.sub",
            _ => "DepotTicketClaims.user_id",
        };
        assert_eq!(
            verify(&c, &ctx),
            Err(err(Reason::invalid_uuid, diagnostic_field))
        );
    }
    let mut c = claims();
    c.sub = "invalid".into();
    c.user_id = "invalid".into();
    assert_eq!(
        verify(&c, &ctx),
        Err(err(Reason::invalid_uuid, "DepotTicketClaims.sub"))
    );
    let mut c = claims();
    c.iat = Default::default();
    assert_eq!(verify(&c, &ctx), Err(err(Reason::missing_field, "")));
    c.exp.modify(|t| t.nanos = 1_000_000_000);
    assert_eq!(
        verify(&c, &ctx),
        Err(err(Reason::invalid_timestamp, "DepotTicketClaims.exp"))
    );
    let mut c = claims();
    c.manifest_ids = vec![vec![3; 32], vec![2; 32], vec![1; 31]];
    assert_eq!(
        verify(&c, &ctx),
        Err(err(
            Reason::unsorted_manifest_ids,
            "DepotTicketClaims.manifest_ids"
        ))
    );
    c.manifest_ids = vec![vec![3; 32], vec![2; 31]];
    assert_eq!(
        verify(&c, &ctx),
        Err(err(
            Reason::invalid_manifest_id,
            "DepotTicketClaims.manifest_ids"
        ))
    );
    let mut c = claims();
    c.sub = AUD.into();
    c.manifest_ids = vec![vec![2; 32]];
    assert_eq!(verify(&c, &ctx), Err(err(Reason::wrong_subject, "")));
    c.manifest_ids = vec![vec![1; 32]];
    assert!(
        verify(
            &c,
            &Context {
                subject: None,
                user_id: None,
                ..ctx
            }
        )
        .is_ok()
    );
}
