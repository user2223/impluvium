// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: 0BSD

//! Manifest-purpose signed cases execute authentication, parsing and semantics.
//! Other signed purposes and transport/model operations have explicit exclusions.

use impluvium_protocol::{
    decode::{Diagnostic, Reason},
    fastcdc,
    hash::Algorithm,
    keys::{Content, KeyInput, TrustedKeySet},
    manifest::{self, Entry, Profile},
    path::{self, WindowsVariant},
    proto::impluvium::signed::v1::KeyRole,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    panic::{AssertUnwindSafe, catch_unwind},
};
// Each integration target uses a subset of the shared corpus helpers.
#[allow(dead_code)]
#[path = "support/corpus.rs"]
mod corpus;
use corpus::{Corpus, array, expected, text};

const SHARDS: &[&str] = &[
    "behavior-000.json",
    "behavior-blake3.json",
    "fastcdc-000.json",
    "signed-000.json",
    "signed-ed25519.json",
    "signed-priority.json",
    "signed-witnesses.json",
    "signed-conformance-000.json",
];
const SKIPPED: &[&str] = &[
    "generic-api",
    "version-decision",
    "range",
    "http-response",
    "transport",
    "redirect",
    "event-receive",
    "resume",
    "session-model",
    "pairing-model",
    "library-model",
    "discovery",
    "download-response",
    "trust-model",
    "size-precheck",
    "publication",
    "blake3-http-response",
];
fn failure(d: Diagnostic) -> Value {
    json!({"reason": format!("{:?}", d.reason), "field": d.field})
}
fn warnings(warnings: Vec<Diagnostic>, accepted: bool) -> Value {
    let mut value =
        json!({"warnings": warnings.iter().map(|w| format!("{:?}", w.reason)).collect::<Vec<_>>()});
    if accepted {
        value["accepted"] = json!(true);
    }
    if !warnings.is_empty() {
        value["warningFields"] = json!(warnings.iter().map(|w| w.field).collect::<Vec<_>>());
    }
    value
}
fn utf8(corpus: &Corpus, value: &Value) -> Result<String, Diagnostic> {
    String::from_utf8(corpus.bytes(value)).map_err(|_| Diagnostic {
        reason: Reason::invalid_utf8,
        field: "",
    })
}
fn reconstruct(corpus: &Corpus, input: &Value) -> Value {
    let bytes = corpus.bytes(&input["manifestPayloadHex"]);
    let (model, _) = match manifest::decode_payload(&bytes, Profile::Base) {
        Ok(v) => v,
        Err(d) => return failure(d),
    };
    let algorithm = Algorithm::from_wire(model.header.hash_algorithm.to_i32()).unwrap();
    let mut supplied = BTreeMap::new();
    for chunk in array(&input["chunks"]) {
        supplied.insert(
            corpus.bytes(&chunk["idHex"]),
            corpus.bytes(&chunk["bodyHex"]),
        );
    }
    for chunk in &model.chunks {
        let Some(body) = supplied.get(&chunk.id) else {
            return json!({"decision":"discard-chunk"});
        };
        if body.len() as u64 != chunk.size || algorithm.digest(body).as_slice() != chunk.id {
            return json!({"decision":"discard-chunk"});
        }
    }
    let mut files = Vec::new();
    for entry in &model.entries {
        if entry.r#type.to_i32() != 1 {
            continue;
        }
        let mut body = Vec::new();
        let mut hash = algorithm.hasher();
        for &index in &entry.chunk_indexes {
            let chunk = &supplied[&model.chunks[index as usize].id];
            body.extend_from_slice(chunk);
            hash.update(chunk);
        }
        assert_eq!(body.len() as u64, entry.size, "exact reconstruction length");
        assert_eq!(
            hash.finish().as_slice(),
            entry.hash,
            "whole reconstructed file hash"
        );
        files.push(json!({"path": entry.path, "bodyHex": hex::encode(body)}));
    }
    json!({"decision":"reconstructed", "files": files})
}
fn signed(corpus: &Corpus, input: &Value) -> Value {
    let entries = array(&input["keys"]);
    let raw: Vec<_> = entries
        .iter()
        .map(|e| corpus.bytes(&e["publicKeyHex"]))
        .collect();
    let inputs: Vec<_> = entries
        .iter()
        .zip(&raw)
        .map(|(e, bytes)| KeyInput {
            public_key: bytes,
            role: match text(&e["role"]) {
                "content" => KeyRole::Content,
                "device" => KeyRole::Device,
                "depot-ticket" => KeyRole::DepotTicket,
                "wrong-role" => KeyRole::Unspecified,
                other => panic!("unknown key role {other}"),
            },
            ticket_status: None,
        })
        .collect();
    let trusted = match TrustedKeySet::<Content>::new(&inputs) {
        Ok(v) => v,
        Err(d) => return json!({"reason": format!("{:?}", d.reason), "field": d.field}),
    };
    let profile = match input["context"]["windows"].as_str() {
        None => Profile::Base,
        Some("publication") => Profile::WindowsPublication,
        Some("installation") => Profile::WindowsInstallation,
        other => panic!("unknown Windows context {other:?}"),
    };
    match manifest::verify(&corpus.bytes(&input["envelopeHex"]), &trusted, profile) {
        Ok((_, w)) => warnings(w, true),
        Err(d) => failure(d),
    }
}
fn evaluate(corpus: &Corpus, operation: &str, input: &Value) -> Value {
    match operation {
        "base-path" => {
            let result = utf8(corpus, &input["pathHex"]).and_then(|value| match input["targetOf"]
                .as_str()
            {
                Some(link) => path::validate_symlink(link, &value),
                None => path::validate_base(&value, ""),
            });
            match result {
                Ok(()) => json!({"accepted":true}),
                Err(d) => failure(d),
            }
        }
        "collision" => {
            let left = utf8(corpus, &input["leftHex"]).unwrap();
            let right = utf8(corpus, &input["rightHex"]).unwrap();
            json!({"collision":path::collision_key(&left) == path::collision_key(&right)})
        }
        "windows-tree" => {
            let entries: Vec<_> = array(&input["entries"])
                .iter()
                .map(|e| Entry {
                    path: text(&e["path"]).into(),
                    r#type: (e["type"].as_i64().unwrap() as i32).into(),
                    ..Default::default()
                })
                .collect();
            let variant = match text(&input["variant"]) {
                "publication" => WindowsVariant::Publication,
                "installation" => WindowsVariant::Installation,
                other => panic!("unknown Windows variant {other}"),
            };
            match path::validate_windows(&entries, variant) {
                Ok(w) => warnings(w, false),
                Err(d) => failure(d),
            }
        }
        "reconstruct" | "blake3-reconstruct" => reconstruct(corpus, input),
        "fastcdc" => {
            let bytes = corpus.bytes(&input["messageHex"]);
            json!({"chunks":fastcdc::spans(&bytes).map(|span| json!({
                "offsetU64":span.start.to_string(), "lengthU64":span.len().to_string()
            })).collect::<Vec<_>>()})
        }
        "signed-document" | "blake3-signed-document" => signed(corpus, input),
        other => panic!("unknown operation {other}"),
    }
}

#[test]
fn manifest_paths_and_chunking_corpus() {
    let corpus = Corpus::new();
    // Pin numerical assets even though product code uses compiled constants.
    corpus.pinned("unicode16.json");
    corpus.pinned("fastcdc-constants.json");
    let mut counts: BTreeMap<(String, String), (usize, usize)> = BTreeMap::new();
    let mut mismatches = Vec::new();
    for file in SHARDS {
        let shard = corpus.pinned(file);
        for case in array(&shard["cases"]) {
            let operation = text(&case["operation"]);
            let input = &case["input"];
            let signed = matches!(operation, "signed-document" | "blake3-signed-document");
            let label = if signed {
                format!("{operation}/{}", text(&input["purpose"]))
            } else {
                operation.to_owned()
            };
            let count = counts.entry(((*file).to_owned(), label)).or_default();
            let skip = if signed {
                match text(&input["purpose"]) {
                    "manifest" => false,
                    "descriptor" | "ticket" | "session" | "pairing" => true,
                    other => panic!("unknown signed purpose {other}"),
                }
            } else {
                SKIPPED.contains(&operation)
            };
            if skip {
                count.1 += 1;
                continue;
            }
            count.0 += 1;
            let evaluated = catch_unwind(AssertUnwindSafe(|| {
                (
                    expected(&corpus, &shard, case),
                    evaluate(&corpus, operation, input),
                )
            }));
            match evaluated {
                Ok((fixed, actual)) if fixed == actual => (),
                Ok((fixed, actual)) => mismatches.push(format!(
                    "{file}: {}: expected {fixed}, received {actual}",
                    text(&case["id"])
                )),
                Err(_) => mismatches.push(format!(
                    "{file}: {}: evaluation panicked",
                    text(&case["id"])
                )),
            }
        }
    }
    for ((file, operation), (executed, skipped)) in counts {
        eprintln!("{file}: {operation}: executed={executed} skipped={skipped}");
    }
    assert!(
        mismatches.is_empty(),
        "{} mismatches:\n{}",
        mismatches.len(),
        mismatches.join("\n")
    );
}
