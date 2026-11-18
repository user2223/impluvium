// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: 0BSD

//! Fixed corpus expectations. Signed-document cases execute only the envelope
//! prefix; their payload-profile, time and semantic operations are counted as
//! skipped, including when the complete document's expected result is success.

use buffa::Message;
use impluvium_protocol::{
    envelope::{self, PayloadType},
    ids,
    keys::{self, Device, KeyInput, Role, SigningKey, SwitchTime, TicketStatus, TrustedKeySet},
    proto::impluvium::signed::v1::{Envelope, KeyRole},
};
use serde_json::Value;
use sha2::{Digest, Sha256, Sha512};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    num::NonZeroU32,
    panic::{UnwindSafe, catch_unwind},
    path::PathBuf,
};

const SHARDS: &[&str] = &[
    "pae-000.json",
    "signed-000.json",
    "signed-ed25519.json",
    "signed-priority.json",
    "signed-witnesses.json",
    "ed25519-profile.json",
    "keys-000.json",
    "primitives-000.json",
];

// This is the complete operation exclusion list for this implementation step.
// The slash suffix explicitly names the unimplemented part of a mixed operation.
const SKIPPED_OPERATIONS: &[&str] = &[
    "blake3",
    "signed-document/payload",
    "blake3-signed-document/payload",
];

// Payload-only reasons in envelope.md §7 steps 11–13, defined by the payload
// chapters and catalogued in errors.md §5.3–5.6. Shared profile/limit reasons
// are classified separately below; an unknown reason is always a test failure.
const PAYLOAD_REASONS: &[&str] = &[
    "chunk_index_out_of_range",
    "chunk_size_mismatch",
    "chunk_size_too_large",
    "chunk_size_zero",
    "descriptor_game_mismatch",
    "descriptor_version_mismatch",
    "duplicate_env_name",
    "duplicate_launch_key",
    "duplicate_requirement",
    "duplicate_setup_id",
    "empty_file_hash_mismatch",
    "executable_not_found",
    "executable_not_regular",
    "install_size_mismatch",
    "invalid_argument_text",
    "invalid_challenge_length",
    "invalid_chunk_id",
    "invalid_entry_fields",
    "invalid_entry_type",
    "invalid_env_name",
    "invalid_env_value",
    "invalid_hash_length",
    "invalid_hint_key",
    "invalid_key",
    "invalid_manifest_id",
    "invalid_manifest_size",
    "invalid_mode",
    "invalid_name",
    "invalid_pairing_id_length",
    "invalid_revision",
    "invalid_timestamp",
    "invalid_uuid",
    "invalid_validity_window",
    "manifest_binding_mismatch",
    "manifest_changed_for_version",
    "manifest_id_mismatch",
    "manifest_size_mismatch",
    "missing_chunking",
    "missing_field",
    "missing_header",
    "path_parent_conflict",
    "proof_challenge_mismatch",
    "proof_device_mismatch",
    "proof_instance_mismatch",
    "proof_pairing_mismatch",
    "revision_conflict",
    "self_requirement",
    "setup_only_with_launch_options",
    "single_chunk_hash_mismatch",
    "stale_revision",
    "symlink_escape",
    "ticket_expired",
    "ticket_key_retired",
    "ticket_not_yet_valid",
    "ticket_scope",
    "total_size_mismatch",
    "unknown_required_feature",
    "unreferenced_chunk",
    "unsorted_chunks",
    "unsorted_entries",
    "unsorted_manifest_ids",
    "unsorted_required_features",
    "unsupported_hash_algorithm",
    "unsupported_manifest_format",
    "unsupported_release_kind",
    "unsupported_symlink",
    "windows_case_collision",
    "working_dir_not_found",
    "wrong_audience",
    "wrong_subject",
];
// Complete reason inventory through envelope.md §7 step 10: the binary profile
// of Envelope and Signature (encoding.md §2), envelope checks, and the local
// configuration checks in keys.md §3. These messages contain neither singular
// message fields, maps nor oneofs, and their fixed depth cannot exceed 64.
const ENVELOPE_REASONS: &[&str] = &[
    "malformed_wire",
    "wrong_wire_type",
    "invalid_utf8",
    "duplicate_field",
    "element_limit_exceeded",
    "byte_limit_exceeded",
    "envelope_too_large",
    "no_signatures",
    "malformed_signature_entry",
    "invalid_payload_type",
    "duplicate_signature_key_id",
    "unknown_payload_type",
    "unexpected_payload_type",
    "invalid_signature",
    "no_trusted_signature",
    "trusted_set_size",
    "invalid_trusted_key",
    "role_mismatch",
    "trusted_key_id_collision",
    "invalid_ticket_key_set",
];

// These reasons can also arise in the payload profile (step 11), or from a
// semantic aggregate limit (step 13). Their name alone does not identify a layer.
const SHARED_PROFILE_REASONS: &[&str] = &[
    "malformed_wire",
    "wrong_wire_type",
    "invalid_utf8",
    "duplicate_field",
    "element_limit_exceeded",
    "byte_limit_exceeded",
];

struct Corpus {
    root: PathBuf,
    index: Value,
    recipes: BTreeMap<String, Value>,
}

fn text(value: &Value) -> &str {
    value.as_str().expect("corpus string")
}
fn number(value: &Value) -> usize {
    value.as_u64().expect("corpus unsigned integer") as usize
}
fn decimal(value: &Value) -> usize {
    text(value).parse().expect("corpus decimal")
}
fn array(value: &Value) -> &Vec<Value> {
    value.as_array().expect("corpus array")
}

impl Corpus {
    fn new() -> Self {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../protocol/testdata");
        let index: Value =
            serde_json::from_slice(&fs::read(root.join("INDEX.json")).unwrap()).unwrap();
        assert_eq!(index["format"], "corpus-index-v2");
        let mut corpus = Self {
            root,
            index,
            recipes: BTreeMap::new(),
        };
        let files: Vec<String> = array(&corpus.index["assets"])
            .iter()
            .map(|asset| text(&asset["file"]))
            .filter(|name| name.starts_with("recipes-") && name.ends_with(".json"))
            .map(str::to_owned)
            .collect();
        for file in files {
            let asset = corpus.pinned(&file);
            assert_eq!(asset["format"], "byte-recipes-v1");
            for recipe in array(&asset["recipes"]) {
                assert!(
                    corpus
                        .recipes
                        .insert(text(&recipe["id"]).to_owned(), recipe.clone())
                        .is_none()
                );
            }
        }
        assert!(corpus.recipes.len() <= 8192);
        corpus
    }

    fn pinned(&self, file: &str) -> Value {
        assert!(!file.contains('/') && !file.contains('\\'));
        let pin = array(&self.index["groups"])
            .iter()
            .chain(array(&self.index["assets"]))
            .find(|entry| entry["file"] == file)
            .expect("indexed asset");
        let path = self.root.join(file);
        let metadata = fs::symlink_metadata(&path).unwrap();
        assert!(metadata.is_file() && metadata.len() <= 1_048_576);
        let bytes = fs::read(path).unwrap();
        assert_eq!(
            hex::encode(Sha256::digest(&bytes)),
            text(&pin["sha256"]),
            "{file} pin"
        );
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        if let Some(count) = pin["count"].as_u64() {
            assert_eq!(array(&value["cases"]).len() as u64, count);
        }
        value
    }

    fn bytes(&self, value: &Value) -> Vec<u8> {
        if let Some(hex) = value.as_str() {
            return hex::decode(hex).expect("corpus hex");
        }
        assert_eq!(value.as_object().expect("byte marker").len(), 1);
        self.recipe(
            text(&value["$bytes"]),
            1,
            &mut 0,
            &mut BTreeSet::new(),
            &mut BTreeSet::new(),
        )
    }

    fn recipe(
        &self,
        id: &str,
        depth: usize,
        visits: &mut usize,
        reachable: &mut BTreeSet<String>,
        active: &mut BTreeSet<String>,
    ) -> Vec<u8> {
        *visits += 1;
        reachable.insert(id.to_owned());
        assert!(depth <= 32 && *visits <= 4096 && reachable.len() <= 256);
        assert!(active.insert(id.to_owned()), "cyclic recipe");
        let recipe = self.recipes.get(id).expect("recipe exists");
        let length = decimal(&recipe["byteLengthU64"]);
        assert!(length <= 33_554_432, "materialization limit");
        let mut child = |id: &str| self.recipe(id, depth + 1, visits, reachable, active);
        let bytes = match text(&recipe["op"]) {
            "literal" => {
                let bytes = hex::decode(text(&recipe["hex"])).unwrap();
                assert!(bytes.len() <= 65536);
                bytes
            }
            "repeat" => {
                let source = child(text(&recipe["source"]));
                let count = decimal(&recipe["countU64"]);
                assert_eq!(source.len().checked_mul(count), Some(length));
                source.repeat(count)
            }
            "concat" => {
                let parts = array(&recipe["parts"]);
                assert!(parts.len() <= 128);
                let mut result = Vec::new();
                for id in parts {
                    let part = child(text(id));
                    assert!(result.len() + part.len() <= length);
                    result.extend_from_slice(&part);
                }
                result
            }
            "slice" => {
                let source = child(text(&recipe["source"]));
                let from = decimal(&recipe["fromU64"]);
                let take = decimal(&recipe["takeU64"]);
                assert_eq!(length, take);
                source[from..from.checked_add(take).unwrap()].to_vec()
            }
            "insert" => {
                let source = child(text(&recipe["source"]));
                let insert = child(text(&recipe["insert"]));
                let at = decimal(&recipe["atU64"]);
                assert_eq!(source.len() + insert.len(), length);
                [&source[..at], insert.as_slice(), &source[at..]].concat()
            }
            "counter" => {
                let start: u64 = text(&recipe["startU64"]).parse().unwrap();
                let step: u64 = text(&recipe["stepU64"]).parse().unwrap();
                let width = number(&recipe["width"]);
                assert!(matches!(width, 1 | 2 | 4 | 8));
                assert_eq!(decimal(&recipe["outputBytesU64"]), length);
                let mut result = Vec::with_capacity(length);
                let mut word = start;
                while result.len() < length {
                    let take = width.min(length - result.len());
                    match text(&recipe["endian"]) {
                        "little" => result.extend_from_slice(&word.to_le_bytes()[..take]),
                        "big" => result
                            .extend_from_slice(&word.to_be_bytes()[8 - width..8 - width + take]),
                        other => panic!("unknown counter endian {other}"),
                    }
                    word = word.wrapping_add(step);
                }
                result
            }
            other => panic!("unknown byte recipe operation {other}"),
        };
        assert_eq!(bytes.len(), length, "recipe {id} length");
        assert_eq!(
            hex::encode(Sha256::digest(&bytes)),
            text(&recipe["sha256Hex"]),
            "recipe {id} hash"
        );
        active.remove(id);
        bytes
    }
}

fn expected<'a>(shard: &'a Value, case: &'a Value) -> &'a Value {
    let expected = &case["expected"];
    if let Some(key) = expected["$value"].as_str() {
        &shard["values"][key]
    } else {
        expected
    }
}

#[derive(Debug, Eq, PartialEq)]
struct Failure {
    reason: String,
    field: String,
}

fn signed<R: Role>(corpus: &Corpus, input: &Value, purpose: PayloadType<R>) -> Result<(), Failure> {
    let entries = array(&input["keys"]);
    let raw_keys: Vec<_> = entries
        .iter()
        .map(|entry| corpus.bytes(&entry["publicKeyHex"]))
        .collect();
    let inputs: Vec<_> = entries
        .iter()
        .zip(&raw_keys)
        .map(|(entry, bytes)| KeyInput {
            public_key: bytes,
            role: match text(&entry["role"]) {
                "content" => KeyRole::Content,
                "device" => KeyRole::Device,
                "depot-ticket" => KeyRole::DepotTicket,
                "wrong-role" => KeyRole::Unspecified,
                other => panic!("unknown configured role {other}"),
            },
            ticket_status: match entry["status"].as_str() {
                None => None,
                Some("current") => Some(TicketStatus::Current),
                Some("retiring") => Some(TicketStatus::Retiring {
                    switch_time: entry.get("switchTime").map(|time| SwitchTime {
                        seconds: text(&time["seconds"]).parse().unwrap(),
                        nanos: number(&time["nanos"]) as u32,
                    }),
                }),
                other => panic!("unknown ticket status {other:?}"),
            },
        })
        .collect();
    let trusted = TrustedKeySet::<R>::new(&inputs).map_err(|error| Failure {
        reason: format!("{:?}", error.reason),
        field: error.field.to_owned(),
    })?;
    // Envelope bytes are not even expanded until configuration succeeds.
    let bytes = corpus.bytes(&input["envelopeHex"]);
    envelope::authenticate(&bytes, purpose, &trusted)
        .map(|_| ())
        .map_err(|error| Failure {
            reason: format!("{:?}", error.reason),
            field: error.field.to_owned(),
        })
}

fn signed_case(corpus: &Corpus, input: &Value) -> Result<(), Failure> {
    match text(&input["purpose"]) {
        "manifest" => signed(corpus, input, envelope::MANIFEST),
        "descriptor" => signed(corpus, input, envelope::RELEASE),
        "ticket" => signed(corpus, input, envelope::DEPOT_TICKET),
        "session" => signed(corpus, input, envelope::SESSION_PROOF),
        "pairing" => signed(corpus, input, envelope::PAIRING_PROOF),
        other => panic!("unknown signed purpose {other}"),
    }
}

#[derive(Default)]
struct Counts(BTreeMap<(String, String), (usize, usize)>);
impl Counts {
    fn executed(&mut self, file: &str, operation: &str) {
        self.0.entry((file.into(), operation.into())).or_default().0 += 1;
    }
    fn skipped(&mut self, file: &str, operation: &str) {
        assert!(
            SKIPPED_OPERATIONS.contains(&operation),
            "unlisted skipped operation {operation}"
        );
        self.0.entry((file.into(), operation.into())).or_default().1 += 1;
    }
    fn report(&self) {
        for ((file, operation), (executed, skipped)) in &self.0 {
            eprintln!("{file}: {operation}: executed={executed} skipped={skipped}");
        }
    }
}

/// Project a full document expectation onto envelope.md §7 steps 1–10.
/// This uses only the fixed expectation and independent input measurements,
/// never the authentication result that the test is about to compare.
fn envelope_expectation(corpus: &Corpus, input: &Value, expected: &Value) -> Result<Value, String> {
    let Some(reason) = expected["reason"].as_str() else {
        return if expected["accepted"] == true {
            Ok(serde_json::json!({"accepted": true}))
        } else {
            Err("signed-document expectation has no supported outcome".into())
        };
    };
    if PAYLOAD_REASONS.contains(&reason) {
        return Ok(serde_json::json!({"accepted": true}));
    }
    if !ENVELOPE_REASONS.contains(&reason) {
        return Err(format!("unknown signed-document reason {reason}"));
    }
    let field = expected["field"]
        .as_str()
        .ok_or_else(|| "signed-document rejection has no diagnostic field".to_owned())?;
    if SHARED_PROFILE_REASONS.contains(&reason) {
        if field.is_empty() {
            if reason != "byte_limit_exceeded" {
                // Empty fields also occur for unidentifiable wire tags in
                // either layer (errors.md §5). Do not guess from the reason.
                return Err(format!("shared reason {reason} with no layer evidence"));
            }
            // envelope.md §7 step 3 and descriptor.md §5 step 14 both use
            // byte_limit_exceeded with field "". The former measures raw
            // envelope metadata; the latter sums decoded command values.
            // Decode the outer container solely to measure payload length.
            // Do not decode, re-encode, sign or hash the payload here.
            let bytes = corpus.bytes(&input["envelopeHex"]);
            let envelope = Envelope::decode(&mut bytes.as_slice())
                .map_err(|error| format!("cannot measure envelope metadata: {error:?}"))?;
            if bytes.len() - envelope.payload.len() <= 16_384 {
                return Ok(serde_json::json!({"accepted": true}));
            }
        } else if !field.starts_with("Envelope.") && !field.starts_with("Signature.") {
            // Steps 11–13 retain their payload schema's own field, per
            // envelope.md §7 and errors.md §5; enclosing fields are not inherited.
            return Ok(serde_json::json!({"accepted": true}));
        }
    }
    Ok(serde_json::json!({"reason": reason, "field": field}))
}

fn signed_outcome(result: Result<(), Failure>) -> Value {
    match result {
        Ok(()) => serde_json::json!({"accepted": true}),
        Err(failure) => serde_json::json!({"reason": failure.reason, "field": failure.field}),
    }
}

/// Return complete observations so all differing fields of a case remain visible.
fn evaluate_case(
    corpus: &Corpus,
    operation: &str,
    input: &Value,
    expected: &Value,
) -> Result<(Value, Value), String> {
    let mut projected = expected.clone();
    let received = match operation {
        "pae" => {
            let message = envelope::pae(
                &corpus.bytes(&input["typeHex"]),
                &corpus.bytes(&input["bodyHex"]),
            )
            .map_err(|error| format!("PAE failed: {error:?}"))?;
            serde_json::json!({"paeHex": hex::encode(message)})
        }
        "sha256" => serde_json::json!({
            "digestHex": hex::encode(ids::manifest_id(&corpus.bytes(&input["messageHex"])))
        }),
        "sha512" => serde_json::json!({
            "digestHex": hex::encode(Sha512::digest(corpus.bytes(&input["messageHex"])))
        }),
        "hkdf-sha256" | "derive-key" => {
            let ikm = corpus.bytes(&input["ikmHex"]);
            let info = corpus.bytes(&input["infoHex"]);
            let prk = keys::hkdf_sha256_extract(&corpus.bytes(&input["saltHex"]), &ikm);
            let length = number(&input["length"]);
            if length > 8160 {
                return Err("HKDF output exceeds the fixed test allocation limit".into());
            }
            let mut output = vec![0; length];
            keys::hkdf_sha256_expand(&prk, &info, &mut output)
                .map_err(|error| format!("HKDF expand failed: {error:?}"))?;
            if operation == "derive-key" {
                let instance: [u8; 32] =
                    ikm.try_into().map_err(|_| "invalid instance key length")?;
                let public_key = if info == b"impluvium/content-signing-key/v1" {
                    keys::derive_content_key(&instance).public_key()
                } else {
                    let label = std::str::from_utf8(&info).map_err(|error| error.to_string())?;
                    let generation = label
                        .strip_prefix("impluvium/depot-ticket-signing-key/v1/")
                        .ok_or("unknown derivation label")?
                        .parse::<NonZeroU32>()
                        .map_err(|error| error.to_string())?;
                    keys::derive_ticket_key(&instance, generation).public_key()
                };
                serde_json::json!({
                    "prkHex": hex::encode(prk),
                    "seedHex": hex::encode(output),
                    "publicKeyHex": hex::encode(public_key),
                    "keyIdHex": hex::encode(ids::key_id(&public_key)),
                })
            } else {
                serde_json::json!({"prkHex": hex::encode(prk), "okmHex": hex::encode(output)})
            }
        }
        "ed25519-kat" => {
            let seed: [u8; 32] = corpus
                .bytes(&input["seedHex"])
                .try_into()
                .map_err(|_| "invalid Ed25519 seed length")?;
            let key = SigningKey::<Device>::from_seed(&seed);
            let message = corpus.bytes(&input["messageHex"]);
            let signature = corpus.bytes(&expected["signatureHex"]);
            // A known-answer signature must also pass the acceptance profile.
            projected["verificationAccepted"] = Value::from(true);
            serde_json::json!({
                "publicKeyHex": hex::encode(key.public_key()),
                "signatureHex": hex::encode(key.sign(&message)),
                "verificationAccepted": envelope::verify_ed25519(
                    &key.public_key(), &message, &signature,
                ).is_ok(),
            })
        }
        "ed25519-point-profile" => {
            let result = envelope::check_point(&corpus.bytes(&input["pointHex"]));
            serde_json::json!({
                "accepted": result.is_ok(),
                "failedCheck": result.err().map(|error| error.failed_check()).unwrap_or(""),
            })
        }
        "ed25519-verify-profile" | "ed25519-encoding-substage" => {
            let key = corpus.bytes(&input["publicKeyHex"]);
            let signature = corpus.bytes(&input["signatureHex"]);
            let (result, field) = if operation == "ed25519-verify-profile" {
                (
                    envelope::verify_ed25519(&key, &corpus.bytes(&input["messageHex"]), &signature),
                    "accepted",
                )
            } else {
                (
                    envelope::check_ed25519_encoding(&key, &signature),
                    "encodingAccepted",
                )
            };
            let mut outcome = serde_json::json!({
                "failedCheck": result.err().map(|error| error.failed_check()).unwrap_or(""),
            });
            outcome[field] = Value::from(result.is_ok());
            outcome
        }
        "signed-document" | "blake3-signed-document" => {
            projected = envelope_expectation(corpus, input, expected)?;
            signed_outcome(signed_case(corpus, input))
        }
        other => return Err(format!("unknown operation {other}")),
    };
    Ok((projected, received))
}

struct Mismatch {
    file: String,
    id: String,
    expected: Value,
    received: Value,
}

#[derive(Default)]
struct Report {
    mismatches: Vec<Mismatch>,
}

impl Report {
    fn case(
        &mut self,
        file: &str,
        id: &str,
        fixed_expected: &Value,
        evaluate: impl FnOnce() -> Result<(Value, Value), String> + UnwindSafe,
    ) {
        let (expected, received, evaluation_failed) = match catch_unwind(evaluate) {
            Ok(Ok((expected, received))) => (expected, received, false),
            Ok(Err(error)) => (
                fixed_expected.clone(),
                serde_json::json!({"evaluationError": error}),
                true,
            ),
            Err(panic) => {
                let message = panic
                    .downcast_ref::<String>()
                    .map(String::as_str)
                    .or_else(|| panic.downcast_ref::<&str>().copied())
                    .unwrap_or("non-string panic");
                (
                    fixed_expected.clone(),
                    serde_json::json!({"evaluationPanic": message}),
                    true,
                )
            }
        };
        if evaluation_failed || expected != received {
            self.mismatches.push(Mismatch {
                file: file.to_owned(),
                id: id.to_owned(),
                expected,
                received,
            });
        }
    }

    fn finish(&self, counts: &Counts) {
        counts.report();
        for mismatch in &self.mismatches {
            eprintln!(
                "{}: {}\n  expected: {}\n  received: {}",
                mismatch.file, mismatch.id, mismatch.expected, mismatch.received,
            );
        }
        assert!(
            self.mismatches.is_empty(),
            "{} vector cases differ; all observations and counts are listed above",
            self.mismatches.len(),
        );
    }
}

#[test]
fn signature_foundation_corpus() {
    let corpus = Corpus::new();
    let mut counts = Counts::default();
    let mut report = Report::default();
    let schema = corpus.pinned("schema.json");
    report.case("schema.json", "envelope-limits", &Value::from(8), || {
        Ok((
            Value::from(8),
            schema["messages"]["Envelope"]["fields"][2]["maxElements"].clone(),
        ))
    });
    report.case("schema.json", "signature-limits", &Value::from(128), || {
        Ok((
            Value::from(128),
            schema["messages"]["Signature"]["fields"][0]["maxBytes"].clone(),
        ))
    });
    for file in SHARDS {
        let shard = corpus.pinned(file);
        assert!(matches!(
            text(&shard["format"]),
            "reference-corpus-v1" | "reference-corpus-v2"
        ));
        for case in array(&shard["cases"]) {
            let id = text(&case["id"]);
            let operation = text(&case["operation"]);
            let input = &case["input"];
            let expected = expected(&shard, case);
            if operation == "blake3" {
                counts.skipped(file, operation);
                continue;
            }
            if matches!(operation, "signed-document" | "blake3-signed-document") {
                counts.skipped(file, &format!("{operation}/payload"));
            }
            // Count every attempted case, including a mismatch or evaluation
            // error, so a failing run still reports its complete coverage.
            counts.executed(file, operation);
            report.case(file, id, expected, || {
                evaluate_case(&corpus, operation, input, expected)
            });
        }
    }
    let fixtures = corpus.pinned("signed-inputs.json");
    report.case(
        "signed-inputs.json",
        "fixture-format",
        &Value::from("public-synthetic-fixtures-v1"),
        || {
            Ok((
                Value::from("public-synthetic-fixtures-v1"),
                fixtures["format"].clone(),
            ))
        },
    );
    counts.executed("signed-inputs.json", "manifest-id");
    report.case(
        "signed-inputs.json",
        "manifest-id",
        &fixtures["manifestIdHex"],
        || {
            Ok((
                fixtures["manifestIdHex"].clone(),
                Value::from(hex::encode(ids::manifest_id(
                    &corpus.bytes(&fixtures["manifestPayloadHex"]),
                ))),
            ))
        },
    );
    for (purpose, envelope) in fixtures["goldenEnvelopes"].as_object().unwrap() {
        counts.executed("signed-inputs.json", "fixture-envelope");
        report.case(
            "signed-inputs.json",
            purpose,
            &serde_json::json!({"accepted": true}),
            || {
                let (key_field, role) = match purpose.as_str() {
                    "manifest" | "descriptor" => ("contentPublicKeyHex", "content"),
                    "ticket" => ("ticketPublicKeyHex", "depot-ticket"),
                    "session" | "pairing" => ("devicePublicKeyHex", "device"),
                    other => return Err(format!("unknown golden purpose {other}")),
                };
                let mut key =
                    serde_json::json!({"publicKeyHex": fixtures[key_field], "role": role});
                if role == "depot-ticket" {
                    key["status"] = Value::from("current");
                }
                let input =
                    serde_json::json!({"purpose": purpose, "envelopeHex": envelope, "keys": [key]});
                Ok((
                    serde_json::json!({"accepted": true}),
                    signed_outcome(signed_case(&corpus, &input)),
                ))
            },
        );
    }
    report.finish(&counts);
}

#[test]
fn shared_budget_reason_uses_the_failing_layer() {
    let corpus = Corpus::new();
    let expected = serde_json::json!({"reason": "byte_limit_exceeded", "field": ""});
    let payload_budget = Envelope {
        payload_type: envelope::RELEASE.as_str().into(),
        payload: vec![0; 262_145],
        ..Default::default()
    }
    .encode_to_vec();
    let input = serde_json::json!({"envelopeHex": hex::encode(&payload_budget)});
    assert_eq!(
        envelope_expectation(&corpus, &input, &expected).unwrap(),
        serde_json::json!({"accepted": true}),
    );

    // An opaque unknown field contributes to the raw metadata budget. Test
    // both sides of its exact limit without consulting the verifier's result.
    for metadata in [16_384usize, 16_385] {
        let payload = [0x12, 0x01, 0x00];
        let mut bytes = payload.to_vec();
        let unknown_length = metadata - 5;
        bytes.extend_from_slice(&[
            0x22,
            0x80 | (unknown_length & 0x7f) as u8,
            (unknown_length >> 7) as u8,
        ]);
        bytes.resize(metadata + 1, 0);
        let input = serde_json::json!({"envelopeHex": hex::encode(bytes)});
        assert_eq!(
            envelope_expectation(&corpus, &input, &expected).unwrap(),
            if metadata == 16_384 {
                serde_json::json!({"accepted": true})
            } else {
                expected.clone()
            },
        );
    }
}

#[test]
fn reason_inventory_is_explicit_and_unknown_reasons_fail() {
    let corpus = Corpus::new();
    let input = serde_json::json!({});
    for reason in ENVELOPE_REASONS {
        let expected = serde_json::json!({"reason": reason, "field": "Envelope.signatures"});
        assert_eq!(
            envelope_expectation(&corpus, &input, &expected).unwrap(),
            expected,
        );
    }
    for reason in PAYLOAD_REASONS {
        let expected = serde_json::json!({"reason": reason, "field": ""});
        assert_eq!(
            envelope_expectation(&corpus, &input, &expected).unwrap(),
            serde_json::json!({"accepted": true}),
        );
    }
    let payload_scan = serde_json::json!({
        "reason": "byte_limit_exceeded", "field": "Command.args",
    });
    assert_eq!(
        envelope_expectation(&corpus, &input, &payload_scan).unwrap(),
        serde_json::json!({"accepted": true}),
    );
    let unknown = serde_json::json!({"reason": "unregistered_reason", "field": "Command.args"});
    assert!(envelope_expectation(&corpus, &input, &unknown).is_err());
}

#[test]
fn report_keeps_all_mismatches_errors_and_counts() {
    let mut report = Report::default();
    let mut counts = Counts::default();
    for (id, received) in [("first", 1), ("second", 2), ("matching", 0)] {
        counts.executed("synthetic", "sha256");
        report.case("synthetic", id, &Value::from(0), || {
            Ok((Value::from(0), Value::from(received)))
        });
    }
    counts.executed("synthetic", "unknown-operation");
    report.case("synthetic", "unknown", &Value::from(0), || {
        Err("unknown operation".into())
    });
    counts.executed("synthetic", "sha256");
    report.case("synthetic", "after-error", &Value::from(0), || {
        Ok((Value::from(0), Value::from(3)))
    });
    counts.skipped("synthetic", "blake3");
    assert_eq!(report.mismatches.len(), 4);
    assert_eq!(report.mismatches[0].id, "first");
    assert_eq!(report.mismatches[1].id, "second");
    assert_eq!(report.mismatches[2].id, "unknown");
    assert_eq!(report.mismatches[3].id, "after-error");
    assert_eq!(report.mismatches[0].expected, 0);
    assert_eq!(report.mismatches[0].received, 1);
    assert_eq!(counts.0[&("synthetic".into(), "sha256".into())], (4, 0));
    assert_eq!(counts.0[&("synthetic".into(), "blake3".into())], (0, 1));
}
