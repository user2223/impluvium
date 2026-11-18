// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: 0BSD

use buffa::Message;
use impluvium_protocol::{
    decode::{Diagnostic, Reason},
    envelope, fastcdc,
    hash::Algorithm,
    keys::{Content, KeyInput, SigningKey, TrustedKeySet},
    manifest::{
        self, Chunk, Entry, EntryType, Manifest, Profile, SourceEntry, SourceKind, VersionMetadata,
    },
    path,
    proto::impluvium::signed::v1::{Envelope, KeyRole},
};

fn metadata() -> VersionMetadata<'static> {
    VersionMetadata {
        game_id: "11111111-1111-1111-1111-111111111111",
        version_id: "22222222-2222-2222-2222-222222222222",
        created_at: buffa_types::google::protobuf::Timestamp {
            seconds: 123,
            nanos: 0,
            ..Default::default()
        },
    }
}
fn empty() -> Manifest {
    manifest::produce(metadata(), &[]).unwrap().manifest
}
fn regular(path: &str, data: &[u8], indexes: Vec<u32>) -> Entry {
    Entry {
        path: path.into(),
        r#type: EntryType::Regular.into(),
        size: data.len() as u64,
        hash: Algorithm::Sha256.digest(data).to_vec(),
        chunk_indexes: indexes,
        ..Default::default()
    }
}
fn rejected(model: &Manifest, reason: Reason, field: &'static str) {
    assert_eq!(
        manifest::validate(model, Profile::Base),
        Err(Diagnostic { reason, field })
    );
}

#[test]
fn producer_is_deterministic_and_every_file_reconstructs_exactly() {
    let large: Vec<u8> = (0..3_145_857u64)
        .map(|i| (i.wrapping_mul(73) ^ (i >> 13)) as u8)
        .collect();
    let mut source = vec![
        SourceEntry {
            path: "z/large",
            mode: 0o100755,
            kind: SourceKind::File(&large),
        },
        SourceEntry {
            path: "dir/file",
            mode: 0o100644,
            kind: SourceKind::File(b"abc"),
        },
        SourceEntry {
            path: "empty",
            mode: 0o644,
            kind: SourceKind::File(b""),
        },
        SourceEntry {
            path: "copy",
            mode: 0o755,
            kind: SourceKind::File(&large),
        },
        SourceEntry {
            path: "unused",
            mode: 0o40755,
            kind: SourceKind::Directory,
        },
        SourceEntry {
            path: "dir",
            mode: 0o40755,
            kind: SourceKind::Directory,
        },
        SourceEntry {
            path: "dir-aux",
            mode: 0o600,
            kind: SourceKind::File(b"a"),
        },
        SourceEntry {
            path: "link",
            mode: 0o777,
            kind: SourceKind::Symlink("dir/file"),
        },
    ];
    let first = manifest::produce(metadata(), &source).unwrap();
    source.reverse();
    let second = manifest::produce(metadata(), &source).unwrap();
    assert_eq!(
        first.manifest.encode_to_vec(),
        second.manifest.encode_to_vec()
    );
    assert_eq!(first.chunk_bytes, second.chunk_bytes);
    let model = &first.manifest;
    assert!(model.entries.iter().all(|e| e.path != "dir"));
    assert!(
        model
            .entries
            .iter()
            .any(|e| e.path == "unused" && e.r#type.to_i32() == 3)
    );
    assert_eq!(model.created_at.seconds, 123);
    assert_eq!(model.header.hash_algorithm.to_i32(), 1);
    assert_eq!(model.header.chunking.min_size, fastcdc::MIN as u64);
    assert_eq!(model.header.chunking.avg_size, fastcdc::AVG as u64);
    assert_eq!(model.header.chunking.max_size, fastcdc::MAX as u64);
    assert!(model.required_features.is_empty());
    assert!(model.chunks.windows(2).all(|w| w[0].id < w[1].id));
    for (chunk, bytes) in model.chunks.iter().zip(&first.chunk_bytes) {
        assert_eq!(chunk.id, Algorithm::Sha256.digest(bytes));
        assert_eq!(chunk.size, bytes.len() as u64);
    }
    for entry in &model.entries {
        assert!(entry.mode <= 0o777);
        if entry.r#type.to_i32() == 2 {
            assert_eq!(entry.mode, 0);
        }
        if entry.r#type.to_i32() != 1 {
            continue;
        }
        let data: Vec<_> = entry
            .chunk_indexes
            .iter()
            .flat_map(|&i| first.chunk_bytes[i as usize].iter().copied())
            .collect();
        let expected: &[u8] = match entry.path.as_str() {
            "copy" | "z/large" => &large,
            "empty" => b"",
            "dir/file" => b"abc",
            "dir-aux" => b"a",
            other => panic!("unexpected file {other}"),
        };
        assert_eq!(data, expected);
        assert_eq!(entry.hash, Algorithm::Sha256.digest(&data));
        match entry.path.as_str() {
            "empty" => assert!(entry.chunk_indexes.is_empty()),
            "dir/file" | "dir-aux" => assert_eq!(entry.chunk_indexes.len(), 1),
            _ => assert!(entry.chunk_indexes.len() > 1),
        }
    }
    let copy = model.entries.iter().find(|e| e.path == "copy").unwrap();
    let large = model.entries.iter().find(|e| e.path == "z/large").unwrap();
    assert_eq!(copy.chunk_indexes, large.chunk_indexes);
    assert_eq!(
        manifest::decode_payload(&model.encode_to_vec(), Profile::Base)
            .unwrap()
            .0,
        *model
    );
}

#[test]
fn explicit_consumer_chunks_allow_repeated_references_and_arbitrary_boundaries() {
    for algorithm in [Algorithm::Sha256, Algorithm::Blake3] {
        let mut model = empty();
        model.header.get_or_insert_default().hash_algorithm =
            (if algorithm == Algorithm::Sha256 { 1 } else { 2 }).into();
        model
            .header
            .get_or_insert_default()
            .chunking
            .get_or_insert_default()
            .algorithm = 901.into();
        model
            .header
            .get_or_insert_default()
            .chunking
            .get_or_insert_default()
            .min_size = u64::MAX;
        let id = algorithm.digest(b"ab");
        model.chunks = vec![Chunk {
            id: id.to_vec(),
            size: 2,
            ..Default::default()
        }];
        model.entries = vec![
            regular("duplicate", b"abab", vec![0, 0]),
            regular("empty", b"", vec![]),
            regular("single", b"ab", vec![0]),
        ];
        for entry in &mut model.entries {
            entry.hash = algorithm
                .digest(match entry.path.as_str() {
                    "duplicate" => b"abab",
                    "single" => b"ab",
                    _ => b"",
                })
                .to_vec();
        }
        model.total_size = 6;
        assert!(manifest::validate(&model, Profile::Base).is_ok());
        for entry in &model.entries {
            let reconstructed = b"ab".repeat(entry.chunk_indexes.len());
            assert_eq!(entry.size, reconstructed.len() as u64);
            assert_eq!(entry.hash, algorithm.digest(&reconstructed));
        }
        model.entries[0].chunk_indexes = vec![0, 1];
        rejected(
            &model,
            Reason::chunk_index_out_of_range,
            "Entry.chunk_indexes",
        );
    }
}

#[test]
fn header_features_and_chunk_table_priorities_are_stable() {
    let valid = empty();
    type Case = (fn(&mut Manifest), Reason, &'static str);
    let cases: &[Case] = &[
        (
            |m| {
                m.format = 0;
                m.game_id.clear();
            },
            Reason::missing_field,
            "Manifest.format",
        ),
        (
            |m| {
                m.format = 4;
                m.game_id.clear();
            },
            Reason::unsupported_manifest_format,
            "Manifest.format",
        ),
        (
            |m| m.game_id = "AAAAAAAA-1111-1111-1111-111111111111".into(),
            Reason::invalid_uuid,
            "Manifest.game_id",
        ),
        (
            |m| m.version_id = "00000000-0000-0000-0000-000000000000".into(),
            Reason::invalid_uuid,
            "Manifest.version_id",
        ),
        (
            |m| m.header = Default::default(),
            Reason::missing_header,
            "Manifest.header",
        ),
        (
            |m| {
                m.header.get_or_insert_default().hash_algorithm = 0.into();
                m.header.get_or_insert_default().chunking = Default::default();
            },
            Reason::missing_field,
            "Header.hash_algorithm",
        ),
        (
            |m| {
                m.header.get_or_insert_default().hash_algorithm = 77.into();
                m.header.get_or_insert_default().chunking = Default::default();
            },
            Reason::unsupported_hash_algorithm,
            "Header.hash_algorithm",
        ),
        (
            |m| m.header.get_or_insert_default().chunking = Default::default(),
            Reason::missing_chunking,
            "Header.chunking",
        ),
        (
            |m| {
                m.header
                    .get_or_insert_default()
                    .chunking
                    .get_or_insert_default()
                    .algorithm = 0.into()
            },
            Reason::missing_field,
            "Chunking.algorithm",
        ),
        (
            |m| m.required_features = vec!["z.v1".into(), "INVALID".into()],
            Reason::invalid_name,
            "Manifest.required_features",
        ),
        (
            |m| m.required_features = vec!["z.v1".into(), "a.v1".into()],
            Reason::unsorted_required_features,
            "Manifest.required_features",
        ),
        (
            |m| m.required_features = vec!["a.v1".into(), "a.v1".into()],
            Reason::unsorted_required_features,
            "Manifest.required_features",
        ),
        (
            |m| m.required_features = vec!["a.v1".into()],
            Reason::unknown_required_feature,
            "Manifest.required_features",
        ),
        (
            |m| m.chunks = vec![Chunk::default()],
            Reason::invalid_chunk_id,
            "Chunk.id",
        ),
        (
            |m| {
                m.chunks = vec![Chunk {
                    id: vec![0; 32],
                    ..Default::default()
                }]
            },
            Reason::chunk_size_zero,
            "Chunk.size",
        ),
        (
            |m| {
                m.chunks = vec![Chunk {
                    id: vec![0; 32],
                    size: 16_777_217,
                    ..Default::default()
                }]
            },
            Reason::chunk_size_too_large,
            "Chunk.size",
        ),
        (
            |m| {
                m.chunks = vec![Chunk {
                    id: vec![0; 32],
                    size: 1,
                    ..Default::default()
                }]
            },
            Reason::unreferenced_chunk,
            "Manifest.chunks",
        ),
    ];
    for &(mutate, reason, field) in cases {
        let mut model = valid.clone();
        mutate(&mut model);
        rejected(&model, reason, field);
    }
    for ids in [[1, 0], [0, 0]] {
        let mut model = valid.clone();
        model.chunks = ids
            .into_iter()
            .map(|n| Chunk {
                id: vec![n; 32],
                size: 1,
                ..Default::default()
            })
            .collect();
        rejected(&model, Reason::unsorted_chunks, "Manifest.chunks");
    }
    let mut model = valid.clone();
    model.required_features = vec!["a".repeat(129)];
    model.format = 0;
    rejected(
        &model,
        Reason::byte_limit_exceeded,
        "Manifest.required_features",
    );
}

#[test]
fn entry_failures_precede_totals_and_windows_checks() {
    let mut model = empty();
    model.entries = vec![regular("NUL", b"", vec![]), regular("../bad", b"", vec![])];
    assert_eq!(
        manifest::validate(&model, Profile::WindowsInstallation)
            .unwrap_err()
            .reason,
        Reason::path_dot_component
    );
    model.entries = vec![regular("a", b"", vec![]), regular("a/b", b"", vec![])];
    rejected(&model, Reason::path_parent_conflict, "Entry.path");
    model.entries[0].r#type = EntryType::Directory.into();
    model.entries[0].hash.clear();
    rejected(&model, Reason::path_parent_conflict, "Entry.path");
    model.entries = vec![regular("a", b"", vec![]), regular("a", b"", vec![])];
    rejected(&model, Reason::unsorted_entries, "Manifest.entries");
    model.entries = vec![regular("b", b"", vec![]), regular("a", b"", vec![])];
    rejected(&model, Reason::unsorted_entries, "Manifest.entries");
    let directory = Entry {
        path: "a".into(),
        r#type: EntryType::Directory.into(),
        ..Default::default()
    };
    for (mutated, field) in [
        (
            Entry {
                size: 1,
                hash: vec![1],
                ..directory.clone()
            },
            "Entry.size",
        ),
        (
            Entry {
                hash: vec![1],
                symlink_target: "b".into(),
                ..directory.clone()
            },
            "Entry.hash",
        ),
        (
            Entry {
                symlink_target: "b".into(),
                chunk_indexes: vec![0],
                ..directory.clone()
            },
            "Entry.symlink_target",
        ),
        (
            Entry {
                chunk_indexes: vec![0],
                ..directory.clone()
            },
            "Entry.chunk_indexes",
        ),
    ] {
        model.entries = vec![mutated];
        rejected(&model, Reason::invalid_entry_fields, field);
    }
    model.entries = vec![regular("a", b"", vec![])];
    model.entries[0].hash[0] ^= 1;
    rejected(&model, Reason::empty_file_hash_mismatch, "Entry.hash");
    model.entries[0].size = u64::MAX;
    model.entries[0].chunk_indexes = vec![0];
    model.chunks = vec![Chunk {
        id: vec![0; 32],
        size: 1,
        ..Default::default()
    }];
    rejected(&model, Reason::chunk_size_mismatch, "Entry.size");
    model.entries[0].size = 1;
    rejected(&model, Reason::single_chunk_hash_mismatch, "Entry.hash");
}

#[test]
fn linux_profile_keeps_linux_names_and_windows_checks_closure_on_any_host() {
    let mut model = empty();
    model.entries = vec![regular("saves:local", b"", vec![])];
    assert!(manifest::validate(&model, Profile::Base).is_ok());
    assert_eq!(
        manifest::validate(&model, Profile::WindowsInstallation).unwrap_err(),
        Diagnostic {
            reason: Reason::windows_forbidden_character,
            field: "Entry.path",
        }
    );
    for (left, right) in [
        ("A/x", "a/y"),
        ("k/x", "K/y"),
        ("I/x", "ı/y"),
        ("ẞ/x", "ß/y"),
        ("Ᲊ/x", "ᲊ/y"),
    ] {
        model.entries = vec![regular(left, b"", vec![]), regular(right, b"", vec![])];
        model.entries.sort_unstable_by(|a, b| a.path.cmp(&b.path));
        assert!(manifest::validate(&model, Profile::Base).is_ok());
        assert_eq!(
            manifest::validate(&model, Profile::WindowsInstallation)
                .unwrap_err()
                .reason,
            Reason::windows_case_collision
        );
    }
    model.entries = vec![regular("e\u{301}", b"", vec![]), regular("é", b"", vec![])];
    assert!(manifest::validate(&model, Profile::WindowsInstallation).is_ok());
    model.entries = vec![
        regular("shared/a", b"", vec![]),
        regular("shared/b", b"", vec![]),
    ];
    assert!(manifest::validate(&model, Profile::WindowsInstallation).is_ok());
    model.entries = vec![Entry {
        path: "link".into(),
        r#type: EntryType::Symlink.into(),
        symlink_target: "target:stream".into(),
        ..Default::default()
    }];
    assert!(manifest::validate(&model, Profile::Base).is_ok());
    assert_eq!(
        manifest::validate(&model, Profile::WindowsPublication).unwrap(),
        vec![Diagnostic {
            reason: Reason::windows_symlink,
            field: "Entry.type"
        }]
    );
    assert_eq!(
        manifest::validate(&model, Profile::WindowsInstallation).unwrap_err(),
        Diagnostic {
            reason: Reason::unsupported_symlink,
            field: "Entry.type"
        }
    );
    model.entries[0].symlink_target = "../escape".into();
    rejected(&model, Reason::symlink_escape, "Entry.symlink_target");
}

#[test]
fn forbidden_characters_and_priority_apply_to_linux_paths_and_link_targets() {
    for c in ['\\', '\u{0}', '\u{1f}', '\u{7f}', '\u{80}', '\u{9f}'] {
        let value = format!("a{c}b");
        assert_eq!(
            path::validate_base(&value, "Entry.path"),
            Err(Diagnostic {
                reason: Reason::path_forbidden_character,
                field: "Entry.path"
            })
        );
        assert_eq!(
            path::validate_symlink("a/link", &value),
            Err(Diagnostic {
                reason: Reason::path_forbidden_character,
                field: "Entry.symlink_target"
            })
        );
    }
    assert_eq!(
        path::validate_base("/a\\b", "").unwrap_err().reason,
        Reason::path_forbidden_character
    );
    assert_eq!(
        path::validate_base(&format!("{}/..", "a".repeat(256)), "")
            .unwrap_err()
            .reason,
        Reason::path_dot_component
    );
    assert_eq!(
        path::validate_symlink("link", &format!("../{}", "a".repeat(256)))
            .unwrap_err()
            .reason,
        Reason::path_component_too_long
    );
    assert!(path::validate_symlink("a/b/link", "../..").is_ok());
    assert!(path::validate_symlink("link", ".").is_ok());
    assert!(path::validate_base("a\u{a0}b", "").is_ok());
}

#[test]
fn authenticated_payload_errors_keep_their_own_fields_and_priority() {
    let key = SigningKey::<Content>::from_seed(&[9; 32]);
    let public = key.public_key();
    let trusted = TrustedKeySet::<Content>::new(&[KeyInput {
        public_key: &public,
        role: KeyRole::Content,
        ticket_status: None,
    }])
    .unwrap();
    let mut model = empty();
    model.format = 4;
    let payload = model.encode_to_vec();
    let signature = envelope::sign_payload(envelope::MANIFEST, &payload, &key).unwrap();
    let mut outer = Envelope {
        payload_type: envelope::MANIFEST.as_str().into(),
        payload,
        signatures: vec![signature],
        ..Default::default()
    };
    assert_eq!(
        manifest::verify(&outer.encode_to_vec(), &trusted, Profile::Base).unwrap_err(),
        Diagnostic {
            reason: Reason::unsupported_manifest_format,
            field: "Manifest.format"
        }
    );
    outer.signatures[0].sig[0] ^= 1;
    assert_eq!(
        manifest::verify(&outer.encode_to_vec(), &trusted, Profile::Base)
            .unwrap_err()
            .reason,
        Reason::invalid_signature
    );
    // Timestamp validation is a payload-profile rule, before semantic format checks.
    model.created_at.get_or_insert_default().nanos = -1;
    let bytes = model.encode_to_vec();
    assert_eq!(
        manifest::decode_payload(&bytes, Profile::Base).unwrap_err(),
        Diagnostic {
            reason: Reason::invalid_timestamp,
            field: "Manifest.created_at"
        }
    );
}

#[test]
fn producer_timestamp_has_no_unknown_fields_and_omits_zero_nanos() {
    let mut version = metadata();
    // Field 3 is unknown to Timestamp; a producer emits known metadata only.
    version.created_at =
        buffa_types::google::protobuf::Timestamp::decode(&mut &[8, 123, 24, 1][..]).unwrap();
    let built = manifest::produce(version, &[]).unwrap();
    assert_eq!(built.manifest.created_at.encode_to_vec(), [8, 123]);
}

#[test]
fn decoded_model_limits_apply_before_semantic_work() {
    let mut components = vec!["a".repeat(255); 16];
    components[0].pop();
    components.push("b".into());
    let mut value = components.join("/");
    assert_eq!(value.len(), 4096);
    assert!(path::validate_base(&value, "Entry.path").is_ok());
    value.push('c');
    assert_eq!(
        path::validate_base(&value, "Entry.path").unwrap_err(),
        Diagnostic {
            reason: Reason::byte_limit_exceeded,
            field: "Entry.path",
        }
    );
    let mut model = empty();
    model.format = 0;
    model.entries = vec![regular(&value, b"", vec![])];
    rejected(&model, Reason::byte_limit_exceeded, "Entry.path");
    model.entries.clear();
    model.required_features = vec!["a.v1".into(); 65];
    rejected(
        &model,
        Reason::element_limit_exceeded,
        "Manifest.required_features",
    );
}
