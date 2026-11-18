// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: 0BSD

use buffa::Message;
use impluvium_protocol::{
    envelope::{self, Diagnostic, Reason},
    ids,
    keys::{
        self, ConfigurationReason, Content, DepotTicket, Device, KeyInput, SigningKey,
        TicketStatus, TrustedKeySet,
    },
    proto::impluvium::signed::v1::{Envelope, KeyRole, Signature},
};

fn device_trust(public_key: &[u8]) -> TrustedKeySet<Device> {
    TrustedKeySet::new(&[KeyInput {
        public_key,
        role: KeyRole::Device,
        ticket_status: None,
    }])
    .unwrap()
}

fn session(payload: &[u8], key: &SigningKey<Device>) -> Envelope {
    Envelope {
        payload_type: envelope::SESSION_PROOF.as_str().into(),
        payload: payload.to_vec(),
        signatures: vec![envelope::sign_payload(envelope::SESSION_PROOF, payload, key).unwrap()],
        ..Default::default()
    }
}

fn check(input: &Envelope, trust: &TrustedKeySet<Device>) -> Result<(), Diagnostic> {
    envelope::authenticate(&input.encode_to_vec(), envelope::SESSION_PROOF, trust).map(|_| ())
}

fn failure(reason: Reason, field: &'static str) -> Result<(), Diagnostic> {
    Err(Diagnostic { reason, field })
}

#[test]
fn pae_counts_bytes_and_writes_decimal_lengths() {
    assert_eq!(envelope::pae(b"", b"").unwrap(), b"DSSEv1 0  0 ");
    assert_eq!(
        envelope::pae("π".as_bytes(), "€".as_bytes()).unwrap(),
        "DSSEv1 2 π 3 €".as_bytes()
    );
    let body = vec![0xff; 1234];
    let ty = vec![b'a'; 128];
    let encoded = envelope::pae(&ty, &body).unwrap();
    assert!(encoded.starts_with(b"DSSEv1 128 "));
    assert_eq!(&encoded[11 + 128..11 + 128 + 6], b" 1234 ");
    assert!(encoded.ends_with(&body));
    assert_eq!(
        envelope::pae(&[0; 129], b""),
        Err(Diagnostic {
            reason: Reason::byte_limit_exceeded,
            field: "Envelope.payload_type",
        })
    );
}

#[test]
fn signature_counts_and_unknown_ids_do_not_change_trust() {
    let key = SigningKey::<Device>::from_seed(&[1; 32]);
    let trust = device_trust(&key.public_key());
    let original = session(b"exact bytes", &key);
    assert_eq!(check(&original, &trust), Ok(()));
    let mut input = original.clone();
    input.signatures.clear();
    assert_eq!(
        check(&input, &trust),
        failure(Reason::no_signatures, "Envelope.signatures")
    );
    for count in [1, 8, 9] {
        input = original.clone();
        for id in 1..count {
            input.signatures.push(Signature {
                key_id: vec![id as u8; 8],
                sig: vec![0; 64],
                ..Default::default()
            });
        }
        let expected = if count <= 8 {
            Ok(())
        } else {
            failure(Reason::element_limit_exceeded, "Envelope.signatures")
        };
        assert_eq!(check(&input, &trust), expected, "count {count}");
        assert_eq!(trust.keys().len(), 1);
    }
    input.signatures = vec![Signature {
        key_id: vec![0; 8],
        sig: vec![0; 64],
        ..Default::default()
    }];
    assert_eq!(
        check(&input, &trust),
        failure(Reason::no_trusted_signature, "")
    );
    // A well-shaped unknown signature does not need to pass the point profile.
    input = original.clone();
    input.signatures.insert(
        0,
        Signature {
            key_id: vec![0; 8],
            sig: vec![0; 64],
            ..Default::default()
        },
    );
    assert_eq!(check(&input, &trust), Ok(()));
    input.signatures[0].sig.pop();
    assert_eq!(
        check(&input, &trust),
        failure(Reason::malformed_signature_entry, "Envelope.signatures")
    );
    input = original.clone();
    input.signatures.push(input.signatures[0].clone());
    assert_eq!(
        check(&input, &trust),
        failure(Reason::duplicate_signature_key_id, "Envelope.signatures")
    );
    input.signatures[1].sig[0] ^= 1;
    assert_eq!(
        check(&input, &trust),
        failure(Reason::duplicate_signature_key_id, "Envelope.signatures")
    );
}

#[test]
fn every_known_signature_must_verify_in_both_orders() {
    let first = SigningKey::<DepotTicket>::from_seed(&[2; 32]);
    let second = SigningKey::<DepotTicket>::from_seed(&[3; 32]);
    let a = first.public_key();
    let b = second.public_key();
    let trust = TrustedKeySet::<DepotTicket>::new(&[
        KeyInput {
            public_key: &a,
            role: KeyRole::DepotTicket,
            ticket_status: Some(TicketStatus::Current),
        },
        KeyInput {
            public_key: &b,
            role: KeyRole::DepotTicket,
            ticket_status: Some(TicketStatus::Retiring {
                switch_time: Some(keys::SwitchTime {
                    seconds: 1,
                    nanos: 0,
                }),
            }),
        },
    ])
    .unwrap();
    let good = envelope::sign_payload(envelope::DEPOT_TICKET, b"body", &first).unwrap();
    let other = envelope::sign_payload(envelope::DEPOT_TICKET, b"body", &second).unwrap();
    let mut input = Envelope {
        payload_type: envelope::DEPOT_TICKET.as_str().into(),
        payload: b"body".to_vec(),
        signatures: vec![good.clone(), other.clone()],
        ..Default::default()
    };
    let bytes = input.encode_to_vec();
    let authenticated = envelope::authenticate(&bytes, envelope::DEPOT_TICKET, &trust).unwrap();
    assert_eq!(authenticated.valid_keys().len(), 2);
    assert_eq!(authenticated.valid_keys()[0].key_id(), ids::key_id(&a));
    assert_eq!(
        authenticated.valid_keys()[1].ticket_status(),
        trust.keys()[1].ticket_status()
    );
    let mut bad = other;
    bad.sig[40] ^= 1;
    for signatures in [vec![good.clone(), bad.clone()], vec![bad, good]] {
        input.signatures = signatures;
        assert_eq!(
            envelope::authenticate(&input.encode_to_vec(), envelope::DEPOT_TICKET, &trust)
                .unwrap_err(),
            Diagnostic {
                reason: Reason::invalid_signature,
                field: "Signature.sig"
            }
        );
    }
}

#[test]
fn bits_wrong_keys_and_truncation_fail_without_reencoding() {
    let key = SigningKey::<Device>::from_seed(&[4; 32]);
    let trust = device_trust(&key.public_key());
    // Non-minimal protobuf varints and unknown payload fields remain signed bytes.
    let payload = [0x08, 0x81, 0, 0xa0, 0x06, 7];
    let original = session(&payload, &key);
    let bytes = original.encode_to_vec();
    let authenticated = envelope::authenticate(&bytes, envelope::SESSION_PROOF, &trust).unwrap();
    assert_eq!(authenticated.payload(), payload);
    assert!(authenticated.payload().as_ptr() >= bytes.as_ptr());
    assert!(authenticated.payload().as_ptr() < bytes.as_ptr().wrapping_add(bytes.len()));
    let id = ids::manifest_id(authenticated.payload());
    assert_ne!(id, ids::manifest_id(&[0x08, 1, 0xa0, 0x06, 7]));
    for index in 0..payload.len() {
        let mut changed = original.clone();
        changed.payload[index] ^= 1;
        assert_eq!(
            check(&changed, &trust),
            failure(Reason::invalid_signature, "Signature.sig")
        );
    }
    let wrong = SigningKey::<Device>::from_seed(&[5; 32]);
    let mut changed = original.clone();
    changed.signatures[0].sig = wrong
        .sign(&envelope::pae(envelope::SESSION_PROOF.as_str().as_bytes(), &payload).unwrap())
        .to_vec();
    assert_eq!(
        check(&changed, &trust),
        failure(Reason::invalid_signature, "Signature.sig")
    );
    assert_eq!(
        check(&original, &device_trust(&wrong.public_key())),
        failure(Reason::no_trusted_signature, "")
    );
    for length in [0, 1, 31, 32, 63, 65] {
        changed = original.clone();
        changed.signatures[0].sig.resize(length, 0);
        assert_eq!(
            check(&changed, &trust),
            failure(Reason::malformed_signature_entry, "Envelope.signatures")
        );
    }
    // Signature changes are irrelevant to a manifest ID.
    assert_eq!(ids::manifest_id(&changed.payload), id);
}

#[test]
fn trust_configuration_checks_all_points_before_roles_and_collisions() {
    let key = SigningKey::<Device>::from_seed(&[6; 32]);
    let public = key.public_key();
    let wrong_role = KeyInput {
        public_key: &public,
        role: KeyRole::Content,
        ticket_status: None,
    };
    assert_eq!(
        TrustedKeySet::<Device>::new(&[wrong_role])
            .unwrap_err()
            .reason,
        ConfigurationReason::role_mismatch
    );
    assert_eq!(
        TrustedKeySet::<Device>::new(&[]).unwrap_err().reason,
        ConfigurationReason::trusted_set_size
    );
    assert_eq!(
        TrustedKeySet::<Device>::new(&[wrong_role, wrong_role])
            .unwrap_err()
            .reason,
        ConfigurationReason::trusted_set_size
    );
    let weak = [0; 32];
    let invalid = KeyInput {
        public_key: &weak,
        role: KeyRole::DepotTicket,
        ticket_status: None,
    };
    assert_eq!(
        TrustedKeySet::<DepotTicket>::new(&[wrong_role, invalid])
            .unwrap_err()
            .reason,
        ConfigurationReason::invalid_trusted_key
    );
    let duplicate = KeyInput {
        public_key: &public,
        role: KeyRole::DepotTicket,
        ticket_status: None,
    };
    assert_eq!(
        TrustedKeySet::<DepotTicket>::new(&[duplicate, duplicate])
            .unwrap_err()
            .reason,
        ConfigurationReason::trusted_key_id_collision
    );
    assert_eq!(
        TrustedKeySet::<DepotTicket>::new(&[duplicate])
            .unwrap_err()
            .reason,
        ConfigurationReason::invalid_ticket_key_set
    );
    let current = KeyInput {
        ticket_status: Some(TicketStatus::Current),
        ..duplicate
    };
    assert_eq!(
        TrustedKeySet::<DepotTicket>::new(&[current])
            .unwrap()
            .keys()
            .len(),
        1
    );
    assert_eq!(
        TrustedKeySet::<Content>::new(&[KeyInput {
            role: KeyRole::Content,
            ..duplicate
        }])
        .unwrap()
        .keys()
        .len(),
        1
    );
}

#[test]
fn hkdf_limits_and_empty_salt_equivalence() {
    let prk = keys::hkdf_sha256_extract(b"", b"instance");
    assert_eq!(prk, keys::hkdf_sha256_extract(&[0; 32], b"instance"));
    let mut output = [0; 8161];
    assert!(keys::hkdf_sha256_expand(&prk, b"info", &mut output).is_err());
    assert_eq!(output, [0; 8161]);
    keys::hkdf_sha256_expand(&prk, b"info", &mut output[..8160]).unwrap();
    keys::hkdf_sha256_expand(&prk, b"info", &mut []).unwrap();
}

#[test]
fn framing_and_normative_priorities() {
    let key = SigningKey::<Device>::from_seed(&[7; 32]);
    let trust = device_trust(&key.public_key());
    for (bytes, reason, field) in [
        (vec![0x0b], Reason::malformed_wire, "Envelope.payload_type"),
        (
            vec![0x08, 0],
            Reason::wrong_wire_type,
            "Envelope.payload_type",
        ),
        (
            vec![0x0a, 1],
            Reason::malformed_wire,
            "Envelope.payload_type",
        ),
        (
            vec![0x0a, 1, 0xff],
            Reason::invalid_utf8,
            "Envelope.payload_type",
        ),
        (
            vec![0x0a, 0, 0x0a, 0],
            Reason::duplicate_field,
            "Envelope.payload_type",
        ),
        (
            vec![0x0a, 0, 0x0a, 1],
            Reason::malformed_wire,
            "Envelope.payload_type",
        ),
        (vec![0x1a, 1, 0], Reason::malformed_wire, ""),
        (vec![0; 4097], Reason::envelope_too_large, ""),
    ] {
        assert_eq!(
            envelope::authenticate(&bytes, envelope::SESSION_PROOF, &trust).unwrap_err(),
            Diagnostic { reason, field }
        );
    }
    let mut input = session(b"", &key);
    input.payload_type = "invalid type".into();
    input.signatures[0].sig.clear();
    assert_eq!(
        check(&input, &trust),
        failure(Reason::malformed_signature_entry, "Envelope.signatures")
    );
    input = session(b"", &key);
    input.payload_type = "unknown".into();
    input.signatures.push(input.signatures[0].clone());
    assert_eq!(
        check(&input, &trust),
        failure(Reason::duplicate_signature_key_id, "Envelope.signatures")
    );
}

#[test]
fn point_and_scalar_prechecks_precede_the_equation() {
    use envelope::{Ed25519Error, PointError};
    let key = SigningKey::<Device>::from_seed(&[8; 32]);
    let public = key.public_key();
    let signature = key.sign(b"message");
    let mut p = [0xff; 32];
    p[0] = 0xed;
    p[31] = 0x7f;
    let mut negative_zero = [0; 32];
    negative_zero[0] = 1;
    negative_zero[31] = 0x80;
    let mut off_curve = [0; 32];
    off_curve[0] = 2;
    for (bad, failed) in [
        (p, PointError::CanonicalY),
        (negative_zero, PointError::NegativeZero),
        (off_curve, PointError::NotOnCurve),
        ([0; 32], PointError::SmallOrder),
    ] {
        assert_eq!(envelope::check_point(&bad), Err(failed));
        assert_eq!(
            envelope::verify_ed25519(&bad, b"wrong message", &signature),
            Err(Ed25519Error::PublicKey(failed))
        );
        let mut changed = signature;
        changed[..32].copy_from_slice(&bad);
        assert_eq!(
            envelope::verify_ed25519(&public, b"wrong message", &changed),
            Err(Ed25519Error::R(failed))
        );
    }
    let order =
        hex::decode("edd3f55c1a631258d69cf7a2def9de1400000000000000000000000000000010").unwrap();
    let mut changed = signature;
    changed[32..].copy_from_slice(&order);
    assert_eq!(
        envelope::verify_ed25519(&public, b"wrong message", &changed),
        Err(Ed25519Error::SBound)
    );
    assert_eq!(
        envelope::verify_ed25519(&public[..31], b"wrong message", &changed[..63]),
        Err(Ed25519Error::SignatureLength)
    );
}

#[test]
fn trust_and_signature_counts_are_independent() {
    let signing: Vec<_> = (30..39)
        .map(|byte| SigningKey::<DepotTicket>::from_seed(&[byte; 32]))
        .collect();
    let public: Vec<_> = signing.iter().map(SigningKey::public_key).collect();
    let inputs: Vec<_> = public
        .iter()
        .map(|key| KeyInput {
            public_key: key,
            role: KeyRole::DepotTicket,
            ticket_status: Some(TicketStatus::Retiring {
                switch_time: Some(keys::SwitchTime {
                    seconds: 100,
                    nanos: 0,
                }),
            }),
        })
        .collect();
    assert_eq!(
        TrustedKeySet::<DepotTicket>::new(&inputs)
            .unwrap_err()
            .reason,
        ConfigurationReason::trusted_set_size
    );
    let trust = TrustedKeySet::<DepotTicket>::new(&inputs[..8]).unwrap();
    let mut input = Envelope {
        payload_type: envelope::DEPOT_TICKET.as_str().into(),
        payload: vec![42],
        signatures: signing[..8]
            .iter()
            .map(|key| envelope::sign_payload(envelope::DEPOT_TICKET, &[42], key).unwrap())
            .collect(),
        ..Default::default()
    };
    let bytes = input.encode_to_vec();
    assert_eq!(
        envelope::authenticate(&bytes, envelope::DEPOT_TICKET, &trust)
            .unwrap()
            .valid_keys()
            .len(),
        8
    );
    input.signatures.truncate(1);
    let bytes = input.encode_to_vec();
    assert_eq!(
        envelope::authenticate(&bytes, envelope::DEPOT_TICKET, &trust)
            .unwrap()
            .valid_keys()
            .len(),
        1
    );
    assert_eq!(trust.keys().len(), 8);
}
