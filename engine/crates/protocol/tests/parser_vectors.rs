// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: 0BSD

//! Profile operations compare their raw substage, never later semantic validity.

use impluvium_protocol::{
    decode::{self, InputLimit, Projection, Value as Projected},
    limits::{Kind, MessageKind, Occurrence},
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    panic::{AssertUnwindSafe, catch_unwind},
};
#[path = "support/corpus.rs"]
mod corpus;
use corpus::{Corpus, array, expected, text};

// Complete exclusions for behavioral operations outside the parser/size layer.
// A newly introduced operation must be classified explicitly or fail the test.
const SKIPPED: &[&str] = &[
    "acceptance-set-model",
    "auth-rpc-flow",
    "base-path",
    "blake3-http-response",
    "blake3-reconstruct",
    "collision",
    "command-budget",
    "discovery",
    "domain-separation",
    "download-response",
    "event-open",
    "event-open-model",
    "event-receive",
    "generic-api",
    "generic-api-codec",
    "http-response",
    "library-model",
    "normalize-user-code",
    "page-size",
    "page-size-codec",
    "pairing-model",
    "publication",
    "range",
    "reconstruct",
    "redirect",
    "response-head-size",
    "resume",
    "rpc-exchange",
    "server-get",
    "session-model",
    "supersession",
    "ticket-cache-model",
    "transport",
    "trust-model",
    "u64-accumulator-probe",
    "user-code-input",
    "version-decision",
    "windows-tree",
];

fn preserve_enum(field: &str) -> bool {
    matches!(
        field,
        "Header.hash_algorithm"
            | "Chunking.algorithm"
            | "Entry.type"
            | "ReleaseDescriptor.kind"
            | "LaunchOption.platform"
            | "SetupStep.platform"
            | "VersionSummary.kind"
    )
}
fn observation(projection: &Projection) -> Value {
    let mut object = serde_json::Map::new();
    for rule in projection.message.fields() {
        let values: Vec<_> = projection
            .fields
            .iter()
            .filter(|(n, _)| *n == rule.number)
            .filter_map(|(_, v)| {
                if let Kind::Enum(values) = rule.kind {
                    let unknown = match v {
                        Projected::Enum(n) => !values.iter().any(|(_, v)| v == n),
                        Projected::UnknownEnumName(_) => true,
                        _ => false,
                    };
                    if unknown && rule.occurrence == Occurrence::Repeated {
                        return None;
                    }
                    if unknown && !preserve_enum(rule.diagnostic) {
                        return Some(json!(0));
                    }
                }
                Some(match v {
                    Projected::Uint32(n) => json!(n),
                    Projected::Uint64(n) => json!(n.to_string()),
                    Projected::Int32(n) | Projected::Enum(n) => json!(n),
                    Projected::Int64(n) => json!(n.to_string()),
                    Projected::Bool(b) => json!(b),
                    Projected::String(s) => json!(s),
                    Projected::Bytes(b) => json!(hex::encode(b)),
                    Projected::UnknownEnumName(n) => json!({"unknownName": n}),
                    Projected::Message(m) => observation(m),
                    Projected::MapEntry(k, v) => json!([k, v]),
                })
            })
            .collect();
        let value = if matches!(rule.occurrence, Occurrence::Repeated | Occurrence::Map) {
            Value::Array(values)
        } else if let Some(value) = values.into_iter().next() {
            value
        } else {
            match rule.kind {
                Kind::Message(_) => continue,
                Kind::String | Kind::Bytes => json!(""),
                Kind::Bool => json!(false),
                Kind::Uint64 | Kind::Int64 => json!("0"),
                _ => json!(0),
            }
        };
        object.insert(rule.name.to_owned(), value);
    }
    Value::Object(object)
}
fn failure(diagnostic: decode::Diagnostic) -> Value {
    json!({"reason": format!("{:?}", diagnostic.reason), "field": diagnostic.field})
}
fn size_case(corpus: &Corpus, input: &Value) -> Value {
    let limit = match text(&input["surface"]) {
        "manifest-envelope" => InputLimit::envelope(impluvium_protocol::envelope::MANIFEST),
        "descriptor-envelope" => InputLimit::envelope(impluvium_protocol::envelope::RELEASE),
        "small-envelope" => InputLimit::envelope(impluvium_protocol::envelope::SESSION_PROOF),
        "rpc-request" => InputLimit::body(65_536),
        "auth-success" => InputLimit::response(65_536),
        "device-success" => InputLimit::response(1_048_576),
        "library-success" => InputLimit::response(4_194_304),
        "download-success" => InputLimit::response(2_097_152),
        "envelope-manifest-payload" => InputLimit {
            field: "Envelope.payload",
            ..InputLimit::payload(536_870_912)
        },
        "envelope-descriptor-payload" => InputLimit {
            field: "Envelope.payload",
            ..InputLimit::payload(1_048_576)
        },
        other => panic!("unknown size surface {other}"),
    };
    let mut received = 0u64;
    let mut rejected = None;
    let length = corpus.stream(text(&input["streamRecipe"]), |block| {
        if rejected.is_none() {
            if let Err(error) = limit.check_add(received, block.len() as u64) {
                rejected = Some(error);
            } else {
                received += block.len() as u64;
            }
        }
    });
    match rejected {
        None => {
            assert_eq!(received, length);
            json!({"sizeAccepted": true, "receivedU64": received.to_string()})
        }
        Some(error) => {
            let mut value = json!({"sizeAccepted": false, "reason": format!("{:?}", error.reason),
                "firstRejectedByteU64": (limit.bytes as u64 + 1).to_string()});
            if !error.field.is_empty() {
                value["field"] = json!(error.field);
            }
            value
        }
    }
}
fn evaluate(corpus: &Corpus, operation: &str, input: &Value) -> Result<Value, String> {
    if operation == "size-precheck" {
        return Ok(size_case(corpus, input));
    }
    let message =
        MessageKind::from_name(text(&input["message"])).ok_or("unknown schema message")?;
    // The vector's explicit limit selects its precheck substage; otherwise it
    // uses the broad profile input budget, independent of route semantics.
    let limit = InputLimit::body(input["limit"].as_u64().unwrap_or(536_887_296) as usize);
    let bytes = corpus.bytes(
        if operation.ends_with("binary") || operation == "binary-precheck" {
            &input["wireHex"]
        } else {
            &input["rawJsonHex"]
        },
    );
    Ok(match operation {
        "scan-binary" => match decode::project_binary(message, &bytes, limit) {
            Ok(p) => json!({"value": observation(&p)}),
            Err(d) => failure(d),
        },
        "scan-json" => match decode::project_json(message, &bytes, limit) {
            Ok(p) => json!({"value": observation(&p)}),
            Err(d) => failure(d),
        },
        "binary-precheck" => match decode::scan_binary(message, &bytes, limit) {
            Ok(()) => json!({"accepted": true}),
            Err(d) => failure(d),
        },
        "json-precheck" => match decode::scan_json(message, &bytes, limit) {
            Ok(()) => json!({"accepted": true}),
            Err(d) => failure(d),
        },
        other => return Err(format!("unknown operation {other}")),
    })
}

#[test]
fn parser_and_size_corpus() {
    let corpus = Corpus::new();
    let mut counts: BTreeMap<(String, String), (usize, usize)> = BTreeMap::new();
    let mut mismatches = Vec::new();
    for file in corpus.groups().into_iter().filter(|f| {
        f.starts_with("profile-")
            || f == "diagnostic-profile.json"
            || f == "large-limits-000.json"
            || f.starts_with("behavior-")
    }) {
        let shard = corpus.pinned(&file);
        for case in array(&shard["cases"]) {
            let operation = text(&case["operation"]);
            let count = counts.entry((file.clone(), operation.into())).or_default();
            if file.starts_with("behavior-") && SKIPPED.contains(&operation) {
                count.1 += 1;
                continue;
            }
            count.0 += 1;
            let (fixed, actual, evaluation_failed) = match catch_unwind(AssertUnwindSafe(|| {
                let fixed = expected(&corpus, &shard, case);
                evaluate(&corpus, operation, &case["input"]).map(|actual| (fixed, actual))
            })) {
                Ok(Ok((fixed, actual))) => (fixed, actual, false),
                Ok(Err(error)) => (
                    case["expected"].clone(),
                    json!({"evaluationError": error}),
                    true,
                ),
                Err(error) => (
                    case["expected"].clone(),
                    json!({"evaluationPanic": error.downcast_ref::<String>().map(String::as_str).or_else(|| error.downcast_ref::<&str>().copied()).unwrap_or("non-string panic")}),
                    true,
                ),
            };
            if evaluation_failed || actual != fixed {
                mismatches.push(format!(
                    "{}: {}\n  expected: {}\n  received: {}",
                    file,
                    text(&case["id"]),
                    fixed,
                    actual
                ));
            }
        }
    }
    for ((file, operation), (executed, skipped)) in counts {
        eprintln!("{file}: {operation}: executed={executed} skipped={skipped}");
    }
    for mismatch in &mismatches {
        eprintln!("{mismatch}");
    }
    assert!(
        mismatches.is_empty(),
        "{} parser cases differ; all observations and counts are listed above",
        mismatches.len()
    );
}

fn proto_fields(source: &str) -> BTreeMap<String, (u32, String, Occurrence)> {
    let source = source
        .lines()
        .map(|line| line.split("//").next().unwrap())
        .collect::<Vec<_>>()
        .join("\n")
        .replace('{', "{\n")
        .replace('}', "\n}\n")
        .replace(';', ";\n");
    let mut message: Option<String> = None;
    let mut oneof = false;
    let mut fields = BTreeMap::new();
    for line in source.lines().map(str::trim).filter(|s| !s.is_empty()) {
        if let Some(rest) = line.strip_prefix("message ") {
            assert!(
                message.is_none(),
                "nested schema declarations need an explicit inventory rule"
            );
            message = Some(rest.split_whitespace().next().unwrap().to_owned());
            continue;
        }
        let Some(owner) = &message else {
            continue;
        };
        if line.starts_with("oneof ") {
            oneof = true;
            continue;
        }
        if line == "}" {
            if oneof {
                oneof = false;
            } else {
                message = None;
            }
            continue;
        }
        let Some((declaration, number)) = line.strip_suffix(';').and_then(|s| s.split_once('='))
        else {
            continue;
        };
        let number = number.trim().parse().unwrap();
        let mut declaration = declaration.trim();
        let mut occurrence = if oneof {
            Occurrence::Oneof
        } else {
            Occurrence::Scalar
        };
        if let Some(rest) = declaration.strip_prefix("repeated ") {
            declaration = rest;
            occurrence = Occurrence::Repeated;
        }
        let (kind, name) = if let Some(end) = declaration.find('>') {
            occurrence = Occurrence::Map;
            ("map", declaration[end + 1..].trim())
        } else {
            let (kind, name) = declaration.split_once(char::is_whitespace).unwrap();
            (kind.rsplit('.').next().unwrap(), name.trim())
        };
        if occurrence == Occurrence::Scalar && MessageKind::from_name(kind).is_some() {
            occurrence = Occurrence::Message;
        }
        assert!(
            fields
                .insert(
                    format!("{owner}.{name}"),
                    (number, kind.to_owned(), occurrence)
                )
                .is_none()
        );
    }
    fields
}
fn kind_matches(kind: Kind, name: &str, schema: &Value) -> bool {
    match kind {
        Kind::Uint32 => name == "uint32",
        Kind::Uint64 => name == "uint64",
        Kind::Int32 => name == "int32",
        Kind::Int64 => name == "int64",
        Kind::Bool => name == "bool",
        Kind::String => name == "string",
        Kind::Bytes => name == "bytes",
        Kind::Map { .. } => name == "map",
        Kind::Message(m) => name == m.name(),
        Kind::Enum(values) => {
            let Some(expected) = schema["enums"][name].as_object() else {
                return false;
            };
            expected.len() == values.len()
                && values
                    .iter()
                    .all(|(n, v)| expected.get(*n).and_then(Value::as_i64) == Some(i64::from(*v)))
        }
    }
}

#[test]
fn schema_and_field_inventory_require_explicit_guards() {
    let corpus = Corpus::new();
    let schema = corpus.pinned("schema.json");
    let inventory = corpus.pinned("field-inventory.json");
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../protocol/proto");
    fn visit(path: &std::path::Path, sources: &mut Vec<String>) {
        for entry in std::fs::read_dir(path).unwrap() {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_dir() {
                visit(&entry.path(), sources);
            } else if entry.path().extension().is_some_and(|e| e == "proto") {
                sources.push(std::fs::read_to_string(entry.path()).unwrap());
            }
        }
    }
    let mut sources = Vec::new();
    visit(&root, &mut sources);
    let actual: BTreeMap<_, _> = sources
        .iter()
        .flat_map(|source| proto_fields(source))
        .collect();
    let mut guarded = BTreeMap::new();
    let mut positions = BTreeMap::new();
    for message in MessageKind::ALL {
        assert!(
            message.fields().len() <= 16,
            "fixed scanner scratch capacity"
        );
        for rule in message.fields() {
            assert!(positions.insert(rule.diagnostic.to_owned(), rule).is_none());
            if *message != MessageKind::Timestamp {
                let declaration = actual
                    .get(rule.diagnostic)
                    .expect("guard has a schema field");
                assert!(
                    kind_matches(rule.kind, &declaration.1, &schema),
                    "{} type",
                    rule.diagnostic
                );
                guarded.insert(
                    rule.diagnostic.to_owned(),
                    (rule.number, declaration.1.clone(), rule.occurrence),
                );
            }
        }
    }
    assert_eq!(
        actual, guarded,
        "every actual schema position needs an explicit field guard, including presence and repetition"
    );
    let mut inventoried = BTreeMap::new();
    for row in array(&inventory["fields"]) {
        if let Some(parent) = row["parentMessage"].as_str() {
            let message = MessageKind::from_name(parent).unwrap();
            let rule = message
                .fields()
                .iter()
                .find(|r| r.number as u64 == row["parentNo"].as_u64().unwrap())
                .unwrap();
            let Kind::Map { key, value } = rule.kind else {
                panic!("map-entry guard missing");
            };
            assert_eq!(
                row["maxBytes"].as_u64().unwrap() as usize,
                if row["no"] == 1 { key } else { value }
            );
            continue;
        }
        let name = text(&row["fieldId"]);
        let rule = positions[name];
        assert_eq!(
            u64::from(rule.number),
            row["no"].as_u64().unwrap(),
            "{name}"
        );
        assert!(
            kind_matches(rule.kind, text(&row["type"]), &schema),
            "{name} type"
        );
        assert_eq!(
            rule.value_limit,
            row["maxBytes"].as_u64().unwrap_or(0) as usize,
            "{name} value guard"
        );
        assert_eq!(
            rule.element_limit,
            row["maxElements"].as_u64().unwrap_or(0) as usize,
            "{name} element guard"
        );
        let occurrence = if !row["oneof"].is_null() {
            Occurrence::Oneof
        } else if row["map"] == true {
            Occurrence::Map
        } else if row["repeated"] == true {
            Occurrence::Repeated
        } else if matches!(rule.kind, Kind::Message(_)) {
            Occurrence::Message
        } else {
            Occurrence::Scalar
        };
        assert_eq!(
            rule.occurrence, occurrence,
            "{name} presence/repetition guard"
        );
        assert!(inventoried.insert(name.to_owned(), ()).is_none());
    }
    assert_eq!(
        positions.keys().collect::<Vec<_>>(),
        inventoried.keys().collect::<Vec<_>>()
    );
    // The independent schema asset must agree too; it cannot silently lag a
    // newly added field in the actual .proto sources.
    for (name, message) in schema["messages"].as_object().unwrap() {
        let kind = MessageKind::from_name(name).expect("schema message guard");
        assert_eq!(
            kind.fields().len(),
            array(&message["fields"]).len(),
            "{name}"
        );
        for field in array(&message["fields"]) {
            let rule = kind
                .fields()
                .iter()
                .find(|r| r.number as u64 == field["no"].as_u64().unwrap())
                .unwrap();
            assert_eq!(rule.name, text(&field["name"]));
        }
    }
}

#[test]
fn adding_a_schema_field_breaks_the_inventory_comparison() {
    let actual = proto_fields("message Entry { string path = 1; bytes future = 42; }");
    let rules = MessageKind::Entry.fields();
    assert!(
        actual
            .keys()
            .any(|name| !rules.iter().any(|r| r.diagnostic == name.as_str()))
    );
}
