// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: 0BSD

use buffa::Message;
use impluvium_protocol::{
    decode::{self, Diagnostic, InputLimit, Reason},
    descriptor::{
        self, Command, Context, KnownRevision, LaunchOption, PublishedRelease, ReleaseDescriptor,
        SetupStep,
    },
    envelope, ids,
    keys::{Content, KeyInput, SigningKey, TrustedKeySet},
    limits::{self, MessageKind},
    manifest::{self, Manifest, SourceEntry, SourceKind, VersionMetadata},
    path::WindowsVariant,
    proto::impluvium::signed::v1::{Envelope, KeyRole},
};
use serde_json::json;
use std::collections::BTreeMap;

const GAME: &str = "11111111-1111-1111-1111-111111111111";
const VERSION: &str = "22222222-2222-2222-2222-222222222222";
fn manifest() -> Manifest {
    manifest::produce(
        VersionMetadata {
            game_id: GAME,
            version_id: VERSION,
            created_at: buffa_types::google::protobuf::Timestamp::default(),
        },
        &[
            SourceEntry {
                path: "bin/game",
                mode: 0o755,
                kind: SourceKind::File(b"game"),
            },
            SourceEntry {
                path: "empty",
                mode: 0,
                kind: SourceKind::Directory,
            },
            SourceEntry {
                path: "link",
                mode: 0,
                kind: SourceKind::Symlink("bin/game"),
            },
        ],
    )
    .unwrap()
    .manifest
}
fn model(manifest: &Manifest) -> ReleaseDescriptor {
    let bytes = manifest.encode_to_vec();
    ReleaseDescriptor {
        game_id: GAME.into(),
        version_id: VERSION.into(),
        revision: 1,
        manifest_id: ids::manifest_id(&bytes).to_vec(),
        manifest_size: bytes.len() as u64,
        install_size: manifest.total_size,
        kind: 1.into(),
        launch_options: vec![LaunchOption {
            key: "main".into(),
            name: "Game".into(),
            platform: 1.into(),
            command: Command {
                executable: "bin/game".into(),
                ..Default::default()
            }
            .into(),
            ..Default::default()
        }],
        ..Default::default()
    }
}
fn context<'a>(m: &'a Manifest, bytes: &'a [u8]) -> Context<'a> {
    Context {
        expected_version_id: VERSION,
        expected_game_id: Some(GAME),
        manifest: m,
        manifest_payload: bytes,
        known_manifest_id: None,
        known_revision: None,
        windows: WindowsVariant::Installation,
    }
}
fn check(d: &ReleaseDescriptor, c: &Context<'_>) -> Result<Vec<Diagnostic>, Diagnostic> {
    descriptor::validate_context(&descriptor::decode_payload(&d.encode_to_vec())?, c)
}
fn diagnostic(reason: Reason, field: &'static str) -> Diagnostic {
    Diagnostic { reason, field }
}

#[test]
fn metadata_correction_preserves_manifest_identity_and_revision_guards_use_original_bytes() {
    let m = manifest();
    let bytes = m.encode_to_vec();
    let first = model(&m);
    let first_payload = first.encode_to_vec();
    let id = ids::manifest_id(&bytes);
    let mut c = context(&m, &bytes);
    c.known_manifest_id = Some(&id);
    c.known_revision = Some(KnownRevision {
        revision: 1,
        payload: &first_payload,
    });
    assert!(check(&first, &c).is_ok());
    let mut edited = first.clone();
    edited.launch_options[0].name = "Corrected name".into();
    assert_eq!(
        check(&edited, &c),
        Err(diagnostic(Reason::revision_conflict, ""))
    );
    edited.revision = 2;
    assert!(check(&edited, &c).is_ok());
    assert_eq!(edited.manifest_id, first.manifest_id);
    assert_eq!(ids::manifest_id(&m.encode_to_vec()), id);
    c.known_revision = Some(KnownRevision {
        revision: 3,
        payload: &first_payload,
    });
    assert_eq!(
        check(&edited, &c),
        Err(diagnostic(
            Reason::stale_revision,
            "ReleaseDescriptor.revision"
        ))
    );
    let other_id = [0; 32];
    c.known_manifest_id = Some(&other_id);
    assert_eq!(
        check(&edited, &c),
        Err(diagnostic(
            Reason::manifest_changed_for_version,
            "ReleaseDescriptor.manifest_id"
        ))
    );
    // Unknown fields still distinguish two payloads at an equal revision.
    c.known_manifest_id = Some(&id);
    c.known_revision = Some(KnownRevision {
        revision: 1,
        payload: &first_payload,
    });
    let mut unknown = first_payload.clone();
    unknown.extend_from_slice(&[0x80, 1, 1]);
    let decoded = descriptor::decode_payload(&unknown).unwrap();
    assert_eq!(decoded.payload(), unknown);
    assert_eq!(
        descriptor::validate_context(&decoded, &c),
        Err(diagnostic(Reason::revision_conflict, ""))
    );
}

#[test]
fn context_order_and_exact_regular_executable_and_directory_lookup() {
    let m = manifest();
    let bytes = m.encode_to_vec();
    let c = context(&m, &bytes);
    for (executable, reason) in [
        ("missing", Reason::executable_not_found),
        ("link", Reason::executable_not_regular),
        ("empty", Reason::executable_not_regular),
    ] {
        let mut d = model(&m);
        d.launch_options[0]
            .command
            .modify(|command| command.executable = executable.into());
        assert_eq!(check(&d, &c), Err(diagnostic(reason, "Command.executable")));
    }
    for (dir, valid) in [
        ("", true),
        ("bin", true),
        ("empty", true),
        ("bi", false),
        ("bin/game", false),
        ("link", false),
    ] {
        let mut d = model(&m);
        d.launch_options[0]
            .command
            .modify(|command| command.working_dir = dir.into());
        if valid {
            assert!(check(&d, &c).is_ok());
        } else {
            assert_eq!(
                check(&d, &c),
                Err(diagnostic(
                    Reason::working_dir_not_found,
                    "Command.working_dir"
                ))
            );
        }
    }
    let mut d = model(&m);
    d.manifest_id = vec![0; 32];
    d.manifest_size += 1;
    let mut c = context(&m, &bytes);
    c.expected_version_id = GAME;
    assert_eq!(
        check(&d, &c),
        Err(diagnostic(
            Reason::descriptor_version_mismatch,
            "ReleaseDescriptor.version_id"
        ))
    );
    c.expected_version_id = VERSION;
    c.expected_game_id = Some(VERSION);
    assert_eq!(
        check(&d, &c),
        Err(diagnostic(
            Reason::descriptor_game_mismatch,
            "ReleaseDescriptor.game_id"
        ))
    );
    c.expected_game_id = None;
    assert_eq!(
        check(&d, &c),
        Err(diagnostic(
            Reason::manifest_id_mismatch,
            "ReleaseDescriptor.manifest_id"
        ))
    );
}

#[test]
fn all_launch_checks_precede_duplicate_keys_and_all_setup_checks_precede_duplicate_ids() {
    let m = manifest();
    let mut d = model(&m);
    d.launch_options.push(d.launch_options[0].clone());
    assert_eq!(
        descriptor::decode_payload(&d.encode_to_vec())
            .unwrap_err()
            .reason,
        Reason::duplicate_launch_key
    );
    d.launch_options[1].platform = 2.into();
    assert!(descriptor::decode_payload(&d.encode_to_vec()).is_ok());
    d.launch_options[1].platform = 1.into();
    d.launch_options[1]
        .command
        .modify(|command| command.args.push("bad\0argument".into()));
    assert_eq!(
        descriptor::decode_payload(&d.encode_to_vec()).unwrap_err(),
        diagnostic(Reason::invalid_argument_text, "Command.args")
    );
    d.launch_options.clear();
    d.kind = 2.into();
    let setup = SetupStep {
        id: "setup".into(),
        platform: 1.into(),
        command: Command {
            executable: "bin/game".into(),
            ..Default::default()
        }
        .into(),
        ..Default::default()
    };
    d.setup_steps = vec![setup.clone(), setup];
    assert_eq!(
        descriptor::decode_payload(&d.encode_to_vec()).unwrap_err(),
        diagnostic(Reason::duplicate_setup_id, "ReleaseDescriptor.setup_steps")
    );
    d.setup_steps[1]
        .command
        .modify(|command| command.executable.clear());
    assert_eq!(
        descriptor::decode_payload(&d.encode_to_vec()).unwrap_err(),
        diagnostic(Reason::path_empty, "Command.executable")
    );
}

#[test]
fn setup_only_and_unknown_platforms_and_host_independent_windows_profile() {
    let m = manifest();
    let bytes = m.encode_to_vec();
    let c = context(&m, &bytes);
    let mut d = model(&m);
    d.kind = 2.into();
    assert_eq!(
        check(&d, &c),
        Err(diagnostic(
            Reason::setup_only_with_launch_options,
            "ReleaseDescriptor.launch_options"
        ))
    );
    let command = d.launch_options[0].command.clone();
    d.launch_options.clear();
    d.setup_steps.push(SetupStep {
        id: "setup".into(),
        platform: 1.into(),
        command,
        ..Default::default()
    });
    assert!(check(&d, &c).is_ok());
    d.setup_steps[0].platform = 2.into();
    assert_eq!(
        check(&d, &c),
        Err(diagnostic(Reason::unsupported_symlink, "Entry.type"))
    );
    let mut c = context(&m, &bytes);
    c.windows = WindowsVariant::Publication;
    assert_eq!(
        check(&d, &c).unwrap(),
        vec![diagnostic(Reason::windows_symlink, "Entry.type")]
    );
    d.setup_steps[0].platform = 999.into();
    assert!(check(&d, &c).unwrap().is_empty());
}

#[test]
fn signed_descriptor_verification_and_authentication_precedence() {
    let m = manifest();
    let bytes = m.encode_to_vec();
    let c = context(&m, &bytes);
    let payload = model(&m).encode_to_vec();
    let key = SigningKey::<Content>::from_seed(&[17; 32]);
    let public = key.public_key();
    let trust = TrustedKeySet::new(&[KeyInput {
        public_key: &public,
        role: KeyRole::Content,
        ticket_status: None,
    }])
    .unwrap();
    let mut envelope = Envelope {
        payload_type: envelope::RELEASE.as_str().into(),
        payload: payload.clone(),
        signatures: vec![envelope::sign_payload(envelope::RELEASE, &payload, &key).unwrap()],
        ..Default::default()
    };
    assert!(descriptor::verify(&envelope.encode_to_vec(), &trust, &c).is_ok());
    envelope.payload = vec![0xff];
    assert_eq!(
        descriptor::verify(&envelope.encode_to_vec(), &trust, &c).unwrap_err(),
        diagnostic(Reason::invalid_signature, "Signature.sig")
    );
}

fn varint(out: &mut Vec<u8>, mut n: u64) {
    while n >= 128 {
        out.push(n as u8 | 128);
        n >>= 7;
    }
    out.push(n as u8);
}
fn field(out: &mut Vec<u8>, number: u64, bytes: &[u8]) {
    varint(out, number << 3 | 2);
    varint(out, bytes.len() as u64);
    out.extend_from_slice(bytes);
}
fn entry(k: &str, v: &str) -> Vec<u8> {
    let mut b = Vec::new();
    field(&mut b, 1, k.as_bytes());
    field(&mut b, 2, v.as_bytes());
    b
}
fn with_maps(env: &[(&str, &str)], hints: &[(&str, &str)], platform: i32) -> Vec<u8> {
    let mut d = model(&manifest());
    d.launch_options.clear();
    let mut command = Command {
        executable: "bin/game".into(),
        ..Default::default()
    }
    .encode_to_vec();
    for &(k, v) in env {
        field(&mut command, 4, &entry(k, v));
    }
    let mut option = LaunchOption {
        key: "main".into(),
        name: "Game".into(),
        platform: platform.into(),
        ..Default::default()
    }
    .encode_to_vec();
    field(&mut option, 4, &command);
    for &(k, v) in hints {
        field(&mut option, 6, &entry(k, v));
    }
    let mut bytes = d.encode_to_vec();
    field(&mut bytes, 9, &option);
    bytes
}
#[test]
fn map_semantics_preserve_wire_order_and_windows_collision_runs_after_all_env_values() {
    for env in [
        vec![("Z", "\0"), ("=", "ok")],
        vec![("=", "ok"), ("Z", "\0")],
    ] {
        let reason = if env[0].0 == "Z" {
            Reason::invalid_env_value
        } else {
            Reason::invalid_env_name
        };
        assert_eq!(
            descriptor::decode_payload(&with_maps(&env, &[], 1)).unwrap_err(),
            diagnostic(reason, "Command.env")
        );
    }
    assert_eq!(
        descriptor::decode_payload(&with_maps(&[("Path", ""), ("PATH", "")], &[], 2)).unwrap_err(),
        diagnostic(Reason::duplicate_env_name, "Command.env")
    );
    assert!(descriptor::decode_payload(&with_maps(&[("Path", ""), ("PATH", "")], &[], 1)).is_ok());
    assert_eq!(
        descriptor::decode_payload(&with_maps(
            &[("Path", ""), ("PATH", ""), ("Z", "\0")],
            &[],
            2
        ))
        .unwrap_err(),
        diagnostic(Reason::invalid_env_value, "Command.env")
    );
    for hints in [
        vec![("z", "\0"), ("bad key", "")],
        vec![("bad key", ""), ("z", "\0")],
    ] {
        let reason = if hints[0].0 == "z" {
            Reason::invalid_text
        } else {
            Reason::invalid_hint_key
        };
        assert_eq!(
            descriptor::decode_payload(&with_maps(&[], &hints, 1)).unwrap_err(),
            diagnostic(reason, "LaunchOption.hints")
        );
    }
    assert_eq!(
        descriptor::decode_payload(&with_maps(&[("x", "a"), ("x", "b")], &[], 1)).unwrap_err(),
        diagnostic(Reason::duplicate_map_key, "Command.env")
    );
    assert_eq!(
        descriptor::decode_payload(&with_maps(&[], &[("x", "a"), ("x", "b")], 1)).unwrap_err(),
        diagnostic(Reason::duplicate_map_key, "LaunchOption.hints")
    );
}

#[test]
fn descriptor_collections_are_bounded_before_decode_in_both_codecs() {
    let limit = InputLimit::payload(limits::DESCRIPTOR_BYTES);
    for (kind, number, name, max, diagnostic_field, is_map, message) in [
        (
            MessageKind::ReleaseDescriptor,
            8,
            "requires",
            8,
            "ReleaseDescriptor.requires",
            false,
            false,
        ),
        (
            MessageKind::ReleaseDescriptor,
            9,
            "launchOptions",
            128,
            "ReleaseDescriptor.launch_options",
            false,
            true,
        ),
        (
            MessageKind::ReleaseDescriptor,
            10,
            "setupSteps",
            128,
            "ReleaseDescriptor.setup_steps",
            false,
            true,
        ),
        (
            MessageKind::Command,
            2,
            "args",
            256,
            "Command.args",
            false,
            false,
        ),
        (
            MessageKind::Command,
            4,
            "env",
            128,
            "Command.env",
            true,
            false,
        ),
        (
            MessageKind::LaunchOption,
            6,
            "hints",
            64,
            "LaunchOption.hints",
            true,
            false,
        ),
        (
            MessageKind::EmulatorRef,
            3,
            "suggestions",
            32,
            "EmulatorRef.suggestions",
            false,
            false,
        ),
        (
            MessageKind::ReleaseDescriptor,
            11,
            "requiredFeatures",
            64,
            "ReleaseDescriptor.required_features",
            false,
            false,
        ),
    ] {
        for count in [max, max + 1] {
            let mut wire = Vec::new();
            let mut values = Vec::new();
            let mut object = serde_json::Map::new();
            for i in 0..count {
                if is_map {
                    let key = format!("k{i}");
                    field(&mut wire, number, &entry(&key, ""));
                    object.insert(key, json!(""));
                } else {
                    field(&mut wire, number, if message { b"" } else { b"x" });
                    values.push(if message { json!({}) } else { json!("x") });
                }
            }
            let mut json_body = json!({});
            json_body[name] = if is_map { json!(object) } else { json!(values) };
            let want = if count == max {
                Ok(())
            } else {
                Err(diagnostic(Reason::element_limit_exceeded, diagnostic_field))
            };
            assert_eq!(
                decode::scan_binary(kind, &wire, limit),
                want,
                "{name} binary count={count}"
            );
            assert_eq!(
                decode::scan_json(kind, &serde_json::to_vec(&json_body).unwrap(), limit),
                want,
                "{name} JSON count={count}"
            );
        }
    }
}

#[test]
fn descriptor_value_boundaries_and_json_aliases_precede_semantics() {
    let limit = InputLimit::payload(limits::DESCRIPTOR_BYTES);
    for (kind, number, name, max, diagnostic_field) in [
        (
            MessageKind::Command,
            1,
            "executable",
            4096,
            "Command.executable",
        ),
        (
            MessageKind::Command,
            3,
            "workingDir",
            4096,
            "Command.working_dir",
        ),
        (MessageKind::LaunchOption, 1, "key", 128, "LaunchOption.key"),
        (
            MessageKind::LaunchOption,
            2,
            "name",
            128,
            "LaunchOption.name",
        ),
    ] {
        for size in [max, max + 1] {
            let value = "x".repeat(size);
            let mut wire = Vec::new();
            field(&mut wire, number, value.as_bytes());
            let mut body = json!({});
            body[name] = json!(value);
            let want = if size == max {
                Ok(())
            } else {
                Err(diagnostic(Reason::byte_limit_exceeded, diagnostic_field))
            };
            assert_eq!(decode::scan_binary(kind, &wire, limit), want);
            assert_eq!(
                decode::scan_json(kind, &serde_json::to_vec(&body).unwrap(), limit),
                want
            );
        }
    }
    for (kind, raw, reason, field) in [
        (
            MessageKind::ReleaseDescriptor,
            r#"{"launchOptions":[],"launch_options":[]}"#,
            Reason::duplicate_field,
            "ReleaseDescriptor.launch_options",
        ),
        (
            MessageKind::ReleaseDescriptor,
            r#"{"gameId":"x","game_id":"y"}"#,
            Reason::duplicate_field,
            "ReleaseDescriptor.game_id",
        ),
        (
            MessageKind::Command,
            r#"{"workingDir":"","working_dir":""}"#,
            Reason::duplicate_field,
            "Command.working_dir",
        ),
        (
            MessageKind::Command,
            r#"{"env":{"x":"a","\u0078":"b"}}"#,
            Reason::duplicate_json_key,
            "",
        ),
        (
            MessageKind::LaunchOption,
            r#"{"hints":{"x":"a","x":"b"}}"#,
            Reason::duplicate_json_key,
            "",
        ),
    ] {
        assert_eq!(
            decode::scan_json(kind, raw.as_bytes(), limit),
            Err(diagnostic(reason, field))
        );
    }
}

#[test]
fn publication_priorities_affected_order_deduplication_and_deep_shared_manifests() {
    let names: Vec<_> = (0..10_000)
        .map(|i| format!("{i:08x}-1111-1111-1111-111111111111"))
        .collect();
    let requires: Vec<Vec<String>> = (0..names.len())
        .map(|i| names.get(i + 1).cloned().into_iter().collect())
        .collect();
    let snapshot: BTreeMap<_, _> = names
        .iter()
        .zip(&requires)
        .map(|(id, r)| {
            (
                id.as_str(),
                PublishedRelease {
                    manifest_id: [1; 32],
                    requires: r,
                },
            )
        })
        .collect();
    let next = snapshot[names[0].as_str()];
    assert_eq!(
        descriptor::validate_publication(&names[0], next, &snapshot).unwrap(),
        vec![names[0].as_str()]
    );
    let cycle = vec![names[0].clone()];
    let mut snapshot = snapshot.clone();
    snapshot.insert(
        &names[9999],
        PublishedRelease {
            manifest_id: [2; 32],
            requires: &cycle,
        },
    );
    assert_eq!(
        descriptor::validate_publication(&names[0], next, &snapshot)
            .unwrap_err()
            .diagnostic,
        diagnostic(Reason::dependency_cycle, "")
    );
    let missing = vec![names[0].clone(), "missing".into()];
    snapshot.insert(
        &names[9999],
        PublishedRelease {
            manifest_id: [2; 32],
            requires: &missing,
        },
    );
    assert_eq!(
        descriptor::validate_publication(&names[0], next, &snapshot)
            .unwrap_err()
            .diagnostic,
        diagnostic(Reason::dependency_not_found, "")
    );
}

#[test]
fn publication_checks_new_version_first_then_sorted_affected_versions() {
    let ids = [
        "00000001-1111-1111-1111-111111111111",
        "00000002-1111-1111-1111-111111111111",
        "00000003-1111-1111-1111-111111111111",
        "00000004-1111-1111-1111-111111111111",
        "00000005-1111-1111-1111-111111111111",
    ];
    let requirements = [
        vec![ids[3].into()],
        vec![ids[0].into()],
        vec![ids[3].into()],
        vec![],
    ];
    let snapshot = BTreeMap::from([
        (
            ids[0],
            PublishedRelease {
                manifest_id: [1; 32],
                requires: &requirements[0],
            },
        ),
        (
            ids[1],
            PublishedRelease {
                manifest_id: [1; 32],
                requires: &requirements[1],
            },
        ),
        (
            ids[2],
            PublishedRelease {
                manifest_id: [1; 32],
                requires: &requirements[2],
            },
        ),
        (
            ids[4],
            PublishedRelease {
                manifest_id: [1; 32],
                requires: &requirements[3],
            },
        ),
    ]);
    let next = PublishedRelease {
        manifest_id: [1; 32],
        requires: &[],
    };
    assert_eq!(
        descriptor::validate_publication(ids[3], next, &snapshot).unwrap(),
        vec![ids[3], ids[0], ids[1], ids[2]]
    );
    assert!(!snapshot.contains_key(ids[3]));
    let cycle = vec![ids[1].into()];
    let next = PublishedRelease {
        manifest_id: [1; 32],
        requires: &cycle,
    };
    let error = descriptor::validate_publication(ids[3], next, &snapshot).unwrap_err();
    assert_eq!(error.version_id, ids[3]);
    assert_eq!(error.diagnostic, diagnostic(Reason::dependency_cycle, ""));
}

#[test]
fn command_budget_counts_utf8_values_and_every_command_source_but_excludes_hints() {
    let mut d = ReleaseDescriptor::default();
    let mut c = Command {
        executable: "é".into(),
        working_dir: "dir".into(),
        args: vec!["α".into()],
        ..Default::default()
    };
    c.env.insert("ENV".into(), "值".into());
    let option = LaunchOption {
        command: c.clone().into(),
        legacy_command: "legacy".into(),
        ..Default::default()
    };
    let mut step = SetupStep {
        command: c.into(),
        legacy_command: "s".into(),
        ..Default::default()
    };
    // 19 bytes in the launch, 14 bytes in the setup before this final argument.
    step.command.modify(|command| {
        command
            .args
            .push("x".repeat(descriptor::MAX_COMMAND_BYTES - 33));
    });
    d.launch_options.push(option);
    d.setup_steps.push(step);
    d.launch_options[0]
        .hints
        .insert("ignored".into(), "x".repeat(4096));
    assert_eq!(descriptor::validate_command_budget(&d), Ok(()));
    d.setup_steps[0].legacy_command.push('x');
    assert_eq!(
        descriptor::validate_command_budget(&d),
        Err(diagnostic(Reason::byte_limit_exceeded, ""))
    );
}

#[test]
fn repeated_text_and_map_value_limits_are_checked_before_projection() {
    let limit = InputLimit::payload(limits::DESCRIPTOR_BYTES);
    for (kind, number, name, max, diagnostic_field) in [
        (
            MessageKind::ReleaseDescriptor,
            8,
            "requires",
            128,
            "ReleaseDescriptor.requires",
        ),
        (MessageKind::Command, 2, "args", 4096, "Command.args"),
        (
            MessageKind::EmulatorRef,
            3,
            "suggestions",
            256,
            "EmulatorRef.suggestions",
        ),
    ] {
        for size in [max, max + 1] {
            let value = "x".repeat(size);
            let mut wire = Vec::new();
            field(&mut wire, number, value.as_bytes());
            let mut body = json!({});
            body[name] = json!([value]);
            let want = if size == max {
                Ok(())
            } else {
                Err(diagnostic(Reason::byte_limit_exceeded, diagnostic_field))
            };
            assert_eq!(decode::scan_binary(kind, &wire, limit), want);
            assert_eq!(
                decode::scan_json(kind, &serde_json::to_vec(&body).unwrap(), limit),
                want
            );
        }
    }
    for (kind, number, name, diagnostic_field) in [
        (MessageKind::Command, 4, "env", "Command.env"),
        (MessageKind::LaunchOption, 6, "hints", "LaunchOption.hints"),
    ] {
        for (key_size, value_size, valid) in [(128, 4096, true), (129, 0, false), (1, 4097, false)]
        {
            let key = "k".repeat(key_size);
            let value = "v".repeat(value_size);
            let mut wire = Vec::new();
            field(&mut wire, number, &entry(&key, &value));
            let mut object = serde_json::Map::new();
            object.insert(key, json!(value));
            let mut body = json!({});
            body[name] = json!(object);
            let want = if valid {
                Ok(())
            } else {
                Err(diagnostic(Reason::byte_limit_exceeded, diagnostic_field))
            };
            assert_eq!(decode::scan_binary(kind, &wire, limit), want);
            assert_eq!(
                decode::scan_json(kind, &serde_json::to_vec(&body).unwrap(), limit),
                want
            );
        }
    }
    let mut wire = Vec::new();
    for _ in 0..8 {
        field(&mut wire, 8, b"x");
    }
    field(&mut wire, 8, &[0xff]);
    assert_eq!(
        decode::scan_binary(MessageKind::ReleaseDescriptor, &wire, limit),
        Err(diagnostic(
            Reason::element_limit_exceeded,
            "ReleaseDescriptor.requires"
        ))
    );
}
