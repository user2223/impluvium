// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: 0BSD

//! Full descriptor consumers and publication/command-budget substages. Other
//! document purposes and behavioral operations are explicitly counted as skipped.

use impluvium_protocol::{
    decode::Diagnostic,
    descriptor::{self, Context, KnownRevision, PublishedRelease, ReleaseDescriptor},
    envelope,
    keys::{Content, KeyInput, TrustedKeySet},
    manifest::{self, Profile},
    path::WindowsVariant,
    proto::impluvium::signed::v1::KeyRole,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    panic::{AssertUnwindSafe, catch_unwind},
};
#[allow(dead_code)]
#[path = "support/corpus.rs"]
mod corpus;
use corpus::{Corpus, array, expected, text};

const SHARDS: &[&str] = &[
    "signed-000.json",
    "signed-priority.json",
    "signed-witnesses.json",
    "signed-conformance-000.json",
    "behavior-000.json",
    "behavior-001.json",
    "behavior-conformance-000.json",
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
    "base-path",
    "collision",
    "windows-tree",
    "discovery",
    "download-response",
    "reconstruct",
    "trust-model",
    "size-precheck",
    "ticket-cache-model",
    "rpc-exchange",
    "event-open",
    "supersession",
    "server-get",
    "generic-api-codec",
    "page-size",
    "normalize-user-code",
    "user-code-input",
    "page-size-codec",
    "acceptance-set-model",
    "event-open-model",
    "blake3-http-response",
    "domain-separation",
];
fn failure(d: Diagnostic) -> Value {
    json!({"reason":format!("{:?}", d.reason), "field":d.field})
}
fn signed(corpus: &Corpus, input: &Value) -> Value {
    let raw: Vec<_> = array(&input["keys"])
        .iter()
        .map(|k| corpus.bytes(&k["publicKeyHex"]))
        .collect();
    let keys: Vec<_> = array(&input["keys"])
        .iter()
        .zip(&raw)
        .map(|(k, bytes)| KeyInput {
            public_key: bytes,
            role: match text(&k["role"]) {
                "content" => KeyRole::Content,
                "device" => KeyRole::Device,
                "depot-ticket" => KeyRole::DepotTicket,
                "wrong-role" => KeyRole::Unspecified,
                other => panic!("unknown key role {other}"),
            },
            ticket_status: None,
        })
        .collect();
    let trusted = match TrustedKeySet::<Content>::new(&keys) {
        Ok(v) => v,
        Err(d) => return json!({"reason":format!("{:?}",d.reason),"field":d.field}),
    };
    let bytes = corpus.bytes(&input["envelopeHex"]);
    let authenticated = match envelope::authenticate(&bytes, envelope::RELEASE, &trusted) {
        Ok(v) => v,
        Err(d) => return failure(d),
    };
    let descriptor = match descriptor::decode_payload(authenticated.payload()) {
        Ok(v) => v,
        Err(d) => return failure(d),
    };
    let ctx = &input["context"];
    let manifest_bytes = corpus.bytes(&ctx["manifestPayloadHex"]);
    let (manifest, _) = match manifest::decode_payload(&manifest_bytes, Profile::Base) {
        Ok(v) => v,
        Err(d) => return failure(d),
    };
    let known_id = ctx.get("knownManifestIdHex").map(|v| {
        let id: [u8; 32] = corpus.bytes(v).try_into().unwrap();
        id
    });
    let known_payload = ctx
        .get("knownPayloadHex")
        .map(|v| corpus.bytes(v))
        .unwrap_or_default();
    let known_revision = ctx.get("knownRevisionU64").map(|v| KnownRevision {
        revision: text(v).parse().unwrap(),
        payload: &known_payload,
    });
    let windows = match ctx["windows"].as_str() {
        None | Some("installation") => WindowsVariant::Installation,
        Some("publication") => WindowsVariant::Publication,
        other => panic!("unknown Windows variant {other:?}"),
    };
    let context = Context {
        expected_version_id: text(&ctx["versionId"]),
        expected_game_id: ctx["gameId"].as_str(),
        manifest: &manifest,
        manifest_payload: &manifest_bytes,
        known_manifest_id: known_id.as_ref(),
        known_revision,
        windows,
    };
    match descriptor::validate_context(&descriptor, &context) {
        Err(d) => failure(d),
        Ok(w) => {
            let mut v = json!({"accepted":true, "warnings":w.iter().map(|d| format!("{:?}",d.reason)).collect::<Vec<_>>()});
            if !w.is_empty() {
                v["warningFields"] = json!(w.iter().map(|d| d.field).collect::<Vec<_>>());
            }
            v
        }
    }
}
fn publication(corpus: &Corpus, input: &Value) -> Value {
    let releases: Vec<_> = array(&input["published"])
        .iter()
        .chain(std::iter::once(&input["next"]))
        .map(|r| {
            (
                text(&r["versionId"]),
                corpus.bytes(&r["manifestIdHex"]).try_into().unwrap(),
                array(&r["requires"])
                    .iter()
                    .map(|r| text(r).to_owned())
                    .collect::<Vec<_>>(),
            )
        })
        .collect();
    let (next, published) = releases.split_last().unwrap();
    let published = published
        .iter()
        .map(|(id, manifest_id, requires)| {
            (
                *id,
                PublishedRelease {
                    manifest_id: *manifest_id,
                    requires,
                },
            )
        })
        .collect();
    match descriptor::validate_publication(
        next.0,
        PublishedRelease {
            manifest_id: next.1,
            requires: &next.2,
        },
        &published,
    ) {
        Ok(checked) => json!({"accepted":true,"checkedVersions":checked}),
        Err(e) => {
            assert_eq!(e.diagnostic.field, "");
            json!({"reason":format!("{:?}",e.diagnostic.reason),"versionId":e.version_id})
        }
    }
}
fn command_budget(corpus: &Corpus, input: &Value) -> Value {
    // Resolve byte recipes through the same bounded helper used for expectations.
    let shard = json!({});
    let case = json!({"expected":input["model"]});
    let mut resolved = expected(corpus, &shard, &case);
    for field in ["launch_options", "setup_steps"] {
        for item in resolved[field].as_array_mut().unwrap() {
            let env = &mut item["command"]["env"];
            if env.is_array() {
                let entries = env
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|e| (text(&e[0]).to_owned(), e[1].clone()))
                    .collect::<serde_json::Map<_, _>>();
                *env = Value::Object(entries);
            }
        }
    }
    let model: ReleaseDescriptor = serde_json::from_value(resolved).unwrap();
    match descriptor::validate_command_budget(&model) {
        Ok(()) => json!({"accepted":true}),
        Err(d) => failure(d),
    }
}
#[test]
fn descriptor_and_publication_corpus() {
    let corpus = Corpus::new();
    let mut counts: BTreeMap<(String, String), (usize, usize)> = BTreeMap::new();
    let mut mismatches = Vec::new();
    for file in SHARDS {
        let shard = corpus.pinned(file);
        for case in array(&shard["cases"]) {
            let operation = text(&case["operation"]);
            let input = &case["input"];
            let signed_operation =
                matches!(operation, "signed-document" | "blake3-signed-document");
            let label = if signed_operation {
                format!("{operation}/{}", text(&input["purpose"]))
            } else {
                operation.to_owned()
            };
            let count = counts.entry(((*file).to_owned(), label)).or_default();
            let skip = if signed_operation {
                match text(&input["purpose"]) {
                    "descriptor" => false,
                    "manifest" | "ticket" | "session" | "pairing" => true,
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
            let result = catch_unwind(AssertUnwindSafe(|| {
                let actual = match operation {
                    "signed-document" | "blake3-signed-document" => signed(&corpus, input),
                    "publication" => publication(&corpus, input),
                    "command-budget" => command_budget(&corpus, input),
                    other => panic!("unknown operation {other}"),
                };
                (expected(&corpus, &shard, case), actual)
            }));
            match result {
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
