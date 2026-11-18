// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: 0BSD

use buffa::Message;
use impluvium_protocol::{
    api::{
        self, Codec,
        errors::{self, Code, Context, Source},
        pagination::{self, Binding, Failure, Method, Token},
        trust::{self, CheckedConnection, ClientVersion, Connections, Pin, VerificationError},
    },
    decode::{self, Diagnostic, InputLimit, Reason},
    keys::{Content, KeyInput, SigningKey, SwitchTime, TrustedKeySet},
    limits::MessageKind,
    proto::impluvium::{
        signed::v1::{Envelope, KeyRole},
        v2::*,
    },
};
use sha2::{Digest, Sha256};

const INSTANCE: &str = "33333333-3333-3333-3333-333333333333";
const OTHER: &str = "44444444-4444-4444-4444-444444444444";
fn discovery() -> Discovery {
    let mut d = Discovery {
        discovery: 1,
        product: "impluvium".into(),
        server_name: "Server".into(),
        server_version: "display version".into(),
        instance_id: INSTANCE.into(),
        manifest_formats: vec![3],
        source_url: "https://server.invalid/source".into(),
        ..Default::default()
    };
    d.protocol.modify(|p| {
        p.major = 2;
        p.revision = 1;
        p.min_client_revision = 1;
    });
    d
}
fn key() -> SigningKey<Content> {
    SigningKey::from_seed(&[19; 32])
}
fn pin() -> Pin {
    Pin::from_stored(INSTANCE.into(), key().public_key()).unwrap()
}
fn check(d: &Discovery, p: &Pin) -> Result<CheckedConnection, Diagnostic> {
    trust::check_discovery(
        "connection",
        d,
        Some(p),
        ClientVersion::default(),
        &["events.v1".into()],
    )
}
fn assert_failure<T>(result: Result<T, Diagnostic>, reason: Reason, field: &'static str) {
    assert_eq!(result.err(), Some(Diagnostic { reason, field }));
}

#[test]
fn pin_order_and_failures_preserve_the_pin_and_block_credentials() {
    let pin = pin();
    let original = pin.clone();
    let mut d = discovery();
    d.instance_id = OTHER.into();
    d.protocol.modify(|p| p.major = 1);
    d.key_fingerprints.push(KeyFingerprint {
        role: KeyRole::Content.into(),
        sha256: vec![0; 32],
        ..Default::default()
    });
    assert_failure(
        check(&d, &pin),
        Reason::instance_mismatch,
        "Discovery.instance_id",
    );
    d.instance_id = INSTANCE.into();
    assert_failure(
        check(&d, &pin),
        Reason::content_key_changed,
        "Discovery.key_fingerprints",
    );
    d.key_fingerprints[0].sha256 = Sha256::digest(pin.content_public_key()).to_vec();
    assert_failure(
        check(&d, &pin),
        Reason::protocol_unsupported,
        "ProtocolVersion.major",
    );
    let (connections, result) =
        Connections::new(&pin).rejected("connection", check(&d, &pin).unwrap_err());
    assert!(result.is_err());
    assert!(!connections.credential_allowed("connection"));
    assert_eq!(pin, original);
    // Informational fingerprints never replace the content key or extend trust.
    d.protocol.modify(|p| p.major = 2);
    assert!(check(&d, &pin).is_ok());
    d.key_fingerprints[0].role = 999.into();
    d.key_fingerprints[0].sha256 = vec![1; 31];
    assert!(check(&d, &pin).is_ok());
    assert_eq!(pin, original);
}

#[test]
fn chapter_order_checks_fields_before_pins_and_latches_trust_stops() {
    let pin = pin();
    let mut d = discovery();
    d.instance_id = OTHER.into();
    d.server_name.clear();
    assert_failure(
        check(&d, &pin),
        Reason::missing_field,
        "Discovery.server_name",
    );
    d.server_name = "Server".into();
    let (connections, _) =
        Connections::new(&pin).rejected("connection", check(&d, &pin).unwrap_err());
    let (connections, result) =
        connections.checked("connection", check(&discovery(), &pin).unwrap());
    assert_eq!(
        result,
        Err(VerificationError::Stopped(Diagnostic {
            reason: Reason::instance_mismatch,
            field: "",
        }))
    );
    assert!(!connections.credential_allowed("connection"));
    assert!(!connections.credential_allowed("b"));
    let (connections, result) =
        Connections::new(&pin).checked("connection", check(&discovery(), &pin).unwrap());
    assert!(result.is_ok());
    assert!(connections.credential_allowed("connection"));
    assert!(
        !connections
            .open("connection")
            .credential_allowed("connection")
    );
    assert!(!connections.credential_allowed("b"));
}

#[test]
fn discovery_evidence_cannot_verify_another_connection() {
    let pin = pin();
    let proof =
        trust::check_discovery("a", &discovery(), Some(&pin), ClientVersion::default(), &[])
            .unwrap();
    let (connections, result) = Connections::new(&pin).checked("b", proof);
    assert_eq!(result, Err(VerificationError::ConnectionMismatch));
    assert!(!connections.credential_allowed("a"));
    assert!(!connections.credential_allowed("b"));

    let proof =
        trust::check_discovery("a", &discovery(), Some(&pin), ClientVersion::default(), &[])
            .unwrap();
    let (connections, result) = connections.checked("a", proof);
    assert!(result.is_ok());
    assert!(connections.credential_allowed("a"));
    assert!(!connections.credential_allowed("b"));
}

#[test]
fn discovery_evidence_cannot_outlive_a_pin_change() {
    let pin = pin();
    // Reject both parts of the pin independently: the content key can change
    // while the instance stays the same, and the instance can change alone.
    let replacement_key = SigningKey::<Content>::from_seed(&[20; 32]).public_key();
    for replacement in [
        Pin::from_stored(INSTANCE.into(), replacement_key).unwrap(),
        Pin::from_stored(OTHER.into(), *pin.content_public_key()).unwrap(),
    ] {
        let proof = check(&discovery(), &pin).unwrap();
        let (connections, result) = Connections::new(&replacement).checked("connection", proof);
        assert_eq!(result, Err(VerificationError::ContextMismatch));
        assert!(!connections.credential_allowed("connection"));
    }

    // A pairing discovery has no pinned context and cannot authorize access.
    let proof = trust::check_discovery(
        "connection",
        &discovery(),
        None,
        ClientVersion::default(),
        &[],
    )
    .unwrap();
    let (connections, result) = Connections::new(&pin).checked("connection", proof);
    assert_eq!(result, Err(VerificationError::ContextMismatch));
    assert!(!connections.credential_allowed("connection"));
}

#[test]
fn version_order_and_unknown_capabilities() {
    let mut d = discovery();
    let client = ClientVersion {
        major: 2,
        revision: 7,
        min_server_revision: 4,
    };
    d.protocol.modify(|p| {
        p.major = 1;
        p.revision = 3;
        p.min_client_revision = 8;
    });
    assert_failure(
        trust::check_version(&d.protocol, client),
        Reason::protocol_unsupported,
        "ProtocolVersion.major",
    );
    d.protocol.modify(|p| p.major = 2);
    assert_failure(
        trust::check_version(&d.protocol, client),
        Reason::server_too_old,
        "ProtocolVersion.revision",
    );
    d.protocol.modify(|p| p.revision = 99);
    assert_failure(
        trust::check_version(&d.protocol, client),
        Reason::client_too_old,
        "ProtocolVersion.min_client_revision",
    );
    d.protocol.modify(|p| p.min_client_revision = 1);
    assert!(trust::check_version(&d.protocol, client).is_ok());
    // A server revision smaller than the client's revision can be compatible.
    d.protocol.modify(|p| p.revision = 4);
    assert!(trust::check_version(&d.protocol, client).is_ok());
    d = discovery();
    d.capabilities = vec!["future.v91".into()];
    assert!(check(&d, &pin()).is_ok());
    d.required_client_capabilities = vec!["future.v91".into()];
    assert_failure(
        check(&d, &pin()),
        Reason::client_too_old,
        "Discovery.required_client_capabilities",
    );
    assert_eq!(
        trust::accepted_capabilities(
            &["future.v91".into(), "events.v1".into()],
            &["events.v1".into()]
        ),
        vec!["events.v1"]
    );
}

#[test]
fn pairing_key_and_fingerprint_checks_follow_instance_binding() {
    let d = discovery();
    let mut response = PollPairingResponse {
        device_id: OTHER.into(),
        instance_id: OTHER.into(),
        content_public_key: vec![0; 32],
        ..Default::default()
    };
    assert_failure(
        trust::decode_pairing_result(Codec::Binary, &response.encode_to_vec(), &d),
        Reason::instance_mismatch,
        "PollPairingResponse.instance_id",
    );
    response.instance_id = INSTANCE.into();
    assert_failure(
        trust::decode_pairing_result(Codec::Binary, &response.encode_to_vec(), &d),
        Reason::invalid_trusted_key,
        "PollPairingResponse.content_public_key",
    );
    response.content_public_key = key().public_key().to_vec();
    let mut d = d;
    d.key_fingerprints.push(KeyFingerprint {
        role: KeyRole::Content.into(),
        sha256: vec![7; 32],
        ..Default::default()
    });
    assert_failure(
        trust::decode_pairing_result(Codec::Binary, &response.encode_to_vec(), &d),
        Reason::content_key_changed,
        "PollPairingResponse.content_public_key",
    );
    d.key_fingerprints[0].sha256 = Sha256::digest(&response.content_public_key).to_vec();
    let (_, paired) =
        trust::decode_pairing_result(Codec::Binary, &response.encode_to_vec(), &d).unwrap();
    assert_eq!(paired, pin());
}

#[test]
fn all_fields_precede_response_context_comparisons() {
    let json = format!(
        r#"{{"versions":[{{"versionId":"{OTHER}","gameId":"{OTHER}","kind":99,"manifestSize":"1","manifestId":"AA==","descriptorRevision":"1"}},{{"versionId":"{OTHER}"}}]}}"#
    );
    // The first manifest has the wrong length, before either context comparison.
    assert_failure(
        api::decode_generic::<ListVersionsResponse>(Codec::Json, json.as_bytes(), INSTANCE),
        Reason::invalid_length,
        "VersionSummary.manifest_id",
    );
    let mut response = ListVersionsResponse::default();
    response.versions.push(VersionSummary {
        version_id: OTHER.into(),
        game_id: OTHER.into(),
        kind: 99.into(),
        manifest_size: 1,
        manifest_id: vec![0; 32],
        descriptor_revision: 1,
        ..Default::default()
    });
    response.versions.push(VersionSummary {
        version_id: OTHER.into(),
        ..Default::default()
    });
    assert_failure(
        api::decode_generic::<ListVersionsResponse>(
            Codec::Binary,
            &response.encode_to_vec(),
            INSTANCE,
        ),
        Reason::missing_field,
        "VersionSummary.game_id",
    );
}

#[test]
fn enums_wrappers_and_oneof_null_keep_the_existing_decode_boundary() {
    let json =
        format!(r#"{{"gameId":"{INSTANCE}","name":"Game","platforms":[1,99,"FUTURE_PLATFORM"]}}"#);
    let game = api::decode_generic::<GameSummary>(Codec::Json, json.as_bytes(), ()).unwrap();
    assert_eq!(
        game.platforms
            .iter()
            .map(|p| p.to_i32())
            .collect::<Vec<_>>(),
        vec![1]
    );
    for value in ["true", "{}", "[]", "null"] {
        let json = format!(r#"{{"platforms":[{value}]}}"#);
        assert_failure(
            api::decode_generic::<GameSummary>(Codec::Json, json.as_bytes(), ()),
            Reason::json_type_mismatch,
            "GameSummary.platforms",
        );
    }
    for value in ["99", "\"FUTURE_SCOPE\""] {
        let json = format!(
            r#"{{"accessToken":"{}","expiresAt":"2026-01-01T00:00:00Z","scope":{value}}}"#,
            "A".repeat(43)
        );
        assert_failure(
            api::decode_generic::<CompleteSessionResponse>(Codec::Json, json.as_bytes(), ()),
            Reason::unsupported_session_scope,
            "CompleteSessionResponse.scope",
        );
    }
    assert_failure(
        api::decode_generic::<GetGameRequest>(
            Codec::Json,
            br#"{"message":{"gameId":"ignored"}}"#,
            (),
        ),
        Reason::missing_field,
        "GetGameRequest.game_id",
    );
    let json = format!(r#"{{"game":{{"message":{{"gameId":"{INSTANCE}","name":"Game"}}}}}}"#);
    assert_failure(
        api::decode_generic::<GetGameResponse>(Codec::Json, json.as_bytes(), INSTANCE),
        Reason::missing_field,
        "GameSummary.game_id",
    );
    assert_failure(
        api::decode_generic::<BeginSessionResponse>(
            Codec::Json,
            br#"{"challenge":"AA==","expiresAt":{"seconds":0}}"#,
            (),
        ),
        Reason::json_type_mismatch,
        "BeginSessionResponse.expires_at",
    );
    assert_failure(
        api::decode_generic::<GetGameRequest>(Codec::Binary, &[0, 0, 0, 0, 0], ()),
        Reason::malformed_wire,
        "",
    );
    assert_failure(
        decode::scan_json(
            MessageKind::ServerEvent,
            br#"{"connected":null,"heartbeat":{}}"#,
            InputLimit::body(65_536),
        ),
        Reason::oneof_conflict,
        "",
    );
}

#[test]
fn timestamp_bounds_are_checked_in_binary_and_json_before_semantics() {
    let mut response = BeginSessionResponse {
        challenge: vec![1; 32],
        ..Default::default()
    };
    for (seconds, nanos, accepted) in [
        (-62_135_596_800, 0, true),
        (253_402_300_799, 999_999_999, true),
        (-62_135_596_801, 0, false),
        (253_402_300_800, 0, false),
        (0, -1, false),
        (0, 1_000_000_000, false),
    ] {
        response.expires_at.modify(|t| {
            t.seconds = seconds;
            t.nanos = nanos;
        });
        let result = api::decode_generic::<BeginSessionResponse>(
            Codec::Binary,
            &response.encode_to_vec(),
            (),
        );
        if accepted {
            assert!(result.is_ok());
        } else {
            assert_failure(
                result,
                Reason::invalid_timestamp,
                "BeginSessionResponse.expires_at",
            );
        }
    }
    for (timestamp, accepted) in [
        ("0001-01-01T00:00:00Z", true),
        ("9999-12-31T23:59:59.999999999Z", true),
        ("0001-01-01T00:00:00+00:01", false),
        ("9999-12-31T23:59:59-00:01", false),
    ] {
        let json = format!(
            r#"{{"challenge":"{}","expiresAt":"{timestamp}"}}"#,
            "AQEB".repeat(10) + "AQE="
        );
        let result = api::decode_generic::<BeginSessionResponse>(Codec::Json, json.as_bytes(), ());
        if accepted {
            assert!(result.is_ok());
        } else {
            assert_failure(
                result,
                Reason::invalid_timestamp,
                "BeginSessionResponse.expires_at",
            );
        }
    }
}

#[test]
fn token_binding_precedes_expiry_with_exact_nanosecond_boundary() {
    for method in [
        Method::ListDevices,
        Method::ListLibrary,
        Method::ListVersions,
    ] {
        let binding = Binding {
            method,
            page_size: 0,
            game_id: (method == Method::ListVersions).then_some(INSTANCE),
        };
        let token = Token {
            binding: binding.clone(),
            issued: SwitchTime {
                seconds: 1,
                nanos: 3,
            },
        };
        let now = SwitchTime {
            seconds: 3601,
            nanos: 2,
        };
        assert!(pagination::validate("opaque", Some(&token), &binding, now).is_ok());
        let expired = SwitchTime {
            seconds: 3601,
            nanos: 3,
        };
        assert!(matches!(
            pagination::validate("opaque", Some(&token), &binding, expired),
            Err(Failure::Expired { .. })
        ));
        let changed = Binding {
            page_size: 50,
            ..binding.clone()
        };
        assert!(matches!(
            pagination::validate("opaque", Some(&token), &changed, expired),
            Err(Failure::Invalid(Diagnostic {
                reason: Reason::invalid_page_token,
                ..
            }))
        ));
        assert!(pagination::validate("", None, &binding, expired).is_ok());
        assert!(matches!(
            pagination::validate("bad", None, &binding, now),
            Err(Failure::Invalid(_))
        ));
        let error = pagination::validate("opaque", Some(&token), &binding, expired)
            .unwrap_err()
            .wire();
        assert_eq!(error.code, Code::failed_precondition);
        assert_eq!(error.reason, Some(ErrorReason::PageTokenExpired));
    }
    assert_eq!(
        [0, 1, 200, 201, u32::MAX].map(pagination::page_size),
        [50, 1, 200, 200, 200]
    );
}

#[test]
fn error_info_metadata_and_contextual_catalog_are_independent_of_text() {
    let mut info = ErrorInfo {
        reason: 999.into(),
        ..Default::default()
    };
    info.metadata.insert("future".into(), "allowed".into());
    let decoded = api::decode_error_info(&info.encode_to_vec()).unwrap();
    assert_eq!(decoded.reason.to_i32(), 0);
    info.metadata.insert("bad key".into(), "value".into());
    assert_failure(
        api::decode_error_info(&info.encode_to_vec()),
        Reason::invalid_text,
        "ErrorInfo.metadata",
    );
    info.metadata.clear();
    info.metadata.insert("request_id".into(), "".into());
    info.retry_after_seconds = 86_400;
    assert!(api::decode_error_info(&info.encode_to_vec()).is_ok());
    info.retry_after_seconds += 1;
    assert_failure(
        api::decode_error_info(&info.encode_to_vec()),
        Reason::value_out_of_range,
        "ErrorInfo.retry_after_seconds",
    );
    for reason in [
        Reason::invalid_signature,
        Reason::invalid_json,
        Reason::element_limit_exceeded,
    ] {
        let proof = errors::to_wire(reason, Context::Proof).unwrap();
        assert_eq!(
            (proof.code, proof.reason),
            (Code::unauthenticated, Some(ErrorReason::ChallengeInvalid))
        );
        let ticket = errors::to_wire(reason, Context::Ticket).unwrap();
        assert_eq!((ticket.code, ticket.reason), (Code::unauthenticated, None));
    }
    assert_eq!(
        errors::to_wire(Reason::unknown_device, Context::BeginSession)
            .unwrap()
            .reason,
        None
    );
    assert_eq!(
        errors::to_wire(Reason::unknown_device, Context::CompleteSession)
            .unwrap()
            .reason,
        Some(ErrorReason::ChallengeInvalid)
    );
    assert_eq!(
        errors::to_wire(Reason::client_too_old, Context::CompleteSession)
            .unwrap()
            .reason,
        Some(ErrorReason::ClientTooOld)
    );
    for reason in [
        Reason::protocol_unsupported,
        Reason::client_too_old,
        Reason::server_too_old,
        Reason::instance_mismatch,
        Reason::content_key_changed,
    ] {
        assert!(errors::to_wire(reason, Context::General).is_none());
    }
    assert!(errors::to_wire(Reason::unsorted_manifest_ids, Context::Response).is_none());
    assert_eq!(
        errors::to_wire(Reason::unsorted_manifest_ids, Context::Ticket)
            .unwrap()
            .code,
        Code::unauthenticated
    );
    assert_eq!(
        errors::to_wire(Reason::body_too_large, Context::General)
            .unwrap()
            .code,
        Code::resource_exhausted
    );
    assert_eq!(
        errors::to_wire(Reason::invalid_name, Context::General)
            .unwrap()
            .code,
        Code::invalid_argument
    );
    assert_eq!(
        errors::to_wire(Reason::stale_revision, Context::General)
            .unwrap()
            .code,
        Code::failed_precondition
    );
    assert_eq!(errors::Code::parse("future_code"), Code::unknown);
    for n in 1..=21 {
        assert!(errors::wire_catalog(n).is_some());
    }
    for n in [i32::MIN, -1, 0, 22, i32::MAX] {
        assert!(errors::wire_catalog(n).is_none());
    }
    assert_eq!(
        errors::client_action(Code::unauthenticated, 5),
        Some(errors::Action::DeviceRevoked)
    );
    assert_eq!(errors::client_action(Code::internal, 5), None);
}

#[test]
fn only_a_valid_revocation_from_the_contexts_server_can_delete_credentials() {
    let base = "https://server.invalid/prefix";
    let source = Source::Server {
        response_url: "https://server.invalid/api/v2/method",
        transport_verified: true,
    };
    assert!(errors::delete_credentials(
        base,
        source,
        true,
        Code::unauthenticated,
        5
    ));
    assert!(errors::delete_credentials(
        base,
        source,
        true,
        Code::unauthenticated,
        6
    ));
    for reason in [0, 1, 2, 3, 4, 7, 12, 999] {
        assert!(!errors::delete_credentials(
            base,
            source,
            true,
            Code::unauthenticated,
            reason
        ));
    }
    assert!(!errors::delete_credentials(
        base,
        source,
        false,
        Code::unauthenticated,
        5
    ));
    assert!(!errors::delete_credentials(
        base,
        source,
        true,
        Code::internal,
        5
    ));
    for bad in [
        Source::Other,
        Source::Server {
            response_url: "https://other.invalid/api/v2/method",
            transport_verified: true,
        },
        Source::Server {
            response_url: "https://server.invalid:444/api/v2/method",
            transport_verified: true,
        },
        Source::Server {
            response_url: "http://server.invalid/api/v2/method",
            transport_verified: true,
        },
        Source::Server {
            response_url: "https://server.invalid/api/v2/method",
            transport_verified: false,
        },
    ] {
        assert!(!errors::delete_credentials(
            base,
            bad,
            true,
            Code::unauthenticated,
            5
        ));
    }
}

#[test]
fn download_fields_precede_descriptor_then_manifest_set_then_endpoint_syntax() {
    let key = key();
    let public_key = key.public_key();
    let trusted = TrustedKeySet::<Content>::new(&[KeyInput {
        public_key: &public_key,
        role: KeyRole::Content,
        ticket_status: None,
    }])
    .unwrap();
    let mut response = OpenDownloadResponse {
        descriptor: vec![0],
        manifest_ids: vec![vec![0; 32]],
        ticket: vec![0],
        ..Default::default()
    };
    response.depot_endpoints.push(DepotEndpoint {
        base_url: "bad".into(),
        ..Default::default()
    });
    response.manifest_ids.push(vec![0; 32]);
    assert_failure(
        api::decode_download(
            Codec::Binary,
            &response.encode_to_vec(),
            &trusted,
            OTHER,
            None,
        ),
        Reason::unsorted_manifest_ids,
        "OpenDownloadResponse.manifest_ids",
    );
    response.manifest_ids.pop();
    assert_failure(
        api::decode_download(
            Codec::Binary,
            &response.encode_to_vec(),
            &trusted,
            OTHER,
            None,
        ),
        Reason::malformed_wire,
        "",
    );
    // A structurally valid signed descriptor permits the later context and set checks.
    let d = impluvium_protocol::descriptor::ReleaseDescriptor {
        game_id: INSTANCE.into(),
        version_id: OTHER.into(),
        revision: 1,
        manifest_id: vec![7; 32],
        manifest_size: 1,
        kind: 1.into(),
        ..Default::default()
    };
    let payload = d.encode_to_vec();
    let signature = impluvium_protocol::envelope::sign_payload(
        impluvium_protocol::envelope::RELEASE,
        &payload,
        &key,
    )
    .unwrap();
    response.descriptor = Envelope {
        payload_type: impluvium_protocol::envelope::RELEASE.as_str().into(),
        payload,
        signatures: vec![signature],
        ..Default::default()
    }
    .encode_to_vec();
    assert_failure(
        api::decode_download(
            Codec::Binary,
            &response.encode_to_vec(),
            &trusted,
            INSTANCE,
            None,
        ),
        Reason::descriptor_version_mismatch,
        "ReleaseDescriptor.version_id",
    );
    assert_failure(
        api::decode_download(
            Codec::Binary,
            &response.encode_to_vec(),
            &trusted,
            OTHER,
            None,
        ),
        Reason::manifest_set_mismatch,
        "OpenDownloadResponse.manifest_ids",
    );
    response.manifest_ids = vec![vec![7; 32]];
    assert_failure(
        api::decode_download(
            Codec::Binary,
            &response.encode_to_vec(),
            &trusted,
            OTHER,
            None,
        ),
        Reason::invalid_url,
        "DepotEndpoint.base_url",
    );
    response.depot_endpoints[0].base_url = "https://depot.invalid".into();
    assert!(
        api::decode_download(
            Codec::Binary,
            &response.encode_to_vec(),
            &trusted,
            OTHER,
            None
        )
        .is_ok()
    );
}

#[test]
fn enum_field_table_covers_every_schema_enum_container() {
    use impluvium_protocol::limits::{Kind, Occurrence};
    use std::collections::BTreeSet;
    let schema: BTreeSet<_> = MessageKind::ALL
        .iter()
        .flat_map(|m| m.fields())
        .filter(|r| matches!(r.kind, Kind::Enum(_)))
        .map(|r| r.diagnostic)
        .collect();
    let table: BTreeSet<_> = api::ENUM_FIELDS.iter().map(|r| r.field).collect();
    assert_eq!(table, schema);
    assert_eq!(table.len(), api::ENUM_FIELDS.len());
    for policy in api::ENUM_FIELDS {
        let rule = MessageKind::ALL
            .iter()
            .flat_map(|m| m.fields())
            .find(|r| r.diagnostic == policy.field)
            .unwrap();
        assert_eq!(policy.container, rule.occurrence);
        if policy.usage == api::EnumUse::StateChanging {
            assert_eq!(policy.container, Occurrence::Scalar);
        }
    }
}

#[test]
fn depot_base_urls_use_whatwg_paths_and_reject_empty_queries_and_fragments() {
    for base_url in [
        "https://depot.invalid",
        "https://depot.invalid/prefix",
        "https://bücher.invalid/prefix",
    ] {
        let endpoint = DepotEndpoint {
            base_url: base_url.into(),
            ..Default::default()
        };
        assert!(api::decode_depot_endpoint(Codec::Binary, &endpoint.encode_to_vec()).is_ok());
    }
    for base_url in [
        "https://depot.invalid/",
        "https://depot.invalid/prefix/",
        "https://depot.invalid/prefix/.",
        "https://depot.invalid/.",
        "https://depot.invalid\\",
        "https://depot.invalid?",
        "https://depot.invalid#",
        "https://user:password@depot.invalid/prefix",
        "ftp://depot.invalid/prefix",
    ] {
        let endpoint = DepotEndpoint {
            base_url: base_url.into(),
            ..Default::default()
        };
        assert_failure(
            api::decode_depot_endpoint(Codec::Binary, &endpoint.encode_to_vec()),
            Reason::invalid_url,
            "DepotEndpoint.base_url",
        );
    }
}
