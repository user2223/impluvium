// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: 0BSD

//! Bare API contracts and explicit state/arithmetic substages; no HTTP or RPC
//! exchange is synthesized from these observations.

use impluvium_protocol::{
    api::{
        self, Codec, pagination,
        trust::{self, ClientVersion, Connections, Pin, VerificationError},
    },
    decode::Diagnostic,
    keys::{Content, KeyInput, TrustedKeySet},
    proto::impluvium::{signed::v1::KeyRole, v2::*},
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

const SHARDS: &[(&str, usize)] = &[
    ("behavior-000.json", 47),
    ("behavior-api-fields.json", 192),
    ("behavior-conformance-000.json", 36),
    ("behavior-arithmetic.json", 4),
];
const SKIPPED: &[&str] = &[
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
    "reconstruct",
    "size-precheck",
    "publication",
    "normalize-user-code",
    "user-code-input",
    "acceptance-set-model",
    "event-open-model",
    "blake3-http-response",
    "domain-separation",
    "command-budget",
    "server-get",
];
fn failure(d: Diagnostic) -> Value {
    json!({"reason":format!("{:?}", d.reason), "field":d.field})
}
fn outcome<T>(result: Result<T, Diagnostic>) -> Value {
    match result {
        Ok(_) => json!({"accepted":true}),
        Err(d) => failure(d),
    }
}
fn codec(input: &Value) -> Codec {
    match input["codec"].as_str().unwrap_or("json") {
        "proto" => Codec::Binary,
        "json" => Codec::Json,
        other => panic!("unknown codec {other}"),
    }
}
fn bytes(corpus: &Corpus, input: &Value) -> Vec<u8> {
    corpus.bytes(
        input
            .get("wireHex")
            .or_else(|| input.get("rawJsonHex"))
            .expect("message octets"),
    )
}
fn generic(corpus: &Corpus, input: &Value) -> Value {
    let bytes = bytes(corpus, input);
    let codec = codec(input);
    let c = &input["context"];
    macro_rules! plain { ($($name:ident),* $(,)?) => {
        match text(&input["message"]) {
            $(stringify!($name) => outcome(api::decode_generic::<$name>(codec, &bytes, ())),)*
            "GetGameResponse" => outcome(api::decode_generic::<GetGameResponse>(codec, &bytes, text(&c["gameId"]))),
            "GetVersionResponse" => outcome(api::decode_generic::<GetVersionResponse>(codec, &bytes, text(&c["versionId"]))),
            "ListVersionsResponse" => outcome(api::decode_generic::<ListVersionsResponse>(codec, &bytes, text(&c["gameId"]))),
            "DepotEndpoint" => outcome(api::decode_depot_endpoint(codec, &bytes)),
            other => panic!("unknown generic message {other}"),
        }
    }; }
    plain!(
        StartPairingResponse,
        BeginSessionResponse,
        CompleteSessionResponse,
        RevokeSelfRequest,
        RevokeSelfResponse,
        ListDevicesRequest,
        ListDevicesResponse,
        Device,
        RevokeDeviceRequest,
        RevokeDeviceResponse,
        ListLibraryRequest,
        ListLibraryResponse,
        GameSummary,
        GetGameRequest,
        ListVersionsRequest,
        VersionSummary,
        GetVersionRequest,
        OpenDownloadRequest,
        RefreshTicketRequest,
        RefreshTicketResponse
    )
}
fn page_codec(corpus: &Corpus, input: &Value) -> Value {
    let bytes = bytes(corpus, input);
    let codec = codec(input);
    let result = match text(&input["message"]) {
        "ListDevicesRequest" => {
            api::decode_generic::<ListDevicesRequest>(codec, &bytes, ()).map(|m| m.page_size)
        }
        "ListLibraryRequest" => {
            api::decode_generic::<ListLibraryRequest>(codec, &bytes, ()).map(|m| m.page_size)
        }
        "ListVersionsRequest" => {
            api::decode_generic::<ListVersionsRequest>(codec, &bytes, ()).map(|m| m.page_size)
        }
        other => panic!("unknown paginated request {other}"),
    };
    match result {
        Ok(size) => json!({"pageSize":pagination::page_size(size)}),
        Err(d) => failure(d),
    }
}
fn client(v: &Value) -> ClientVersion {
    ClientVersion {
        major: v["major"].as_u64().unwrap() as u32,
        revision: v["revision"].as_u64().unwrap() as u32,
        min_server_revision: v["min_server_revision"].as_u64().unwrap() as u32,
    }
}
fn protocol(v: &Value) -> ProtocolVersion {
    ProtocolVersion {
        major: v["major"].as_u64().unwrap() as u32,
        revision: v["revision"].as_u64().unwrap() as u32,
        min_client_revision: v["min_client_revision"].as_u64().unwrap() as u32,
        ..Default::default()
    }
}
fn version(input: &Value) -> Value {
    let result = trust::check_version(&protocol(&input["server"]), client(&input["client"]));
    json!({"decision":result.err().map(|d| format!("{:?}",d.reason)).unwrap_or("compatible".into())})
}
fn download(corpus: &Corpus, input: &Value) -> Value {
    let c = &input["context"];
    let raw: Vec<_> = array(&c["keys"])
        .iter()
        .map(|k| corpus.bytes(&k["publicKeyHex"]))
        .collect();
    let inputs: Vec<_> = array(&c["keys"])
        .iter()
        .zip(&raw)
        .map(|(k, raw)| {
            assert_eq!(text(&k["role"]), "content");
            KeyInput {
                public_key: raw,
                role: KeyRole::Content,
                ticket_status: None,
            }
        })
        .collect();
    let keys = TrustedKeySet::<Content>::new(&inputs).unwrap();
    outcome(api::decode_download(
        Codec::Json,
        &bytes(corpus, input),
        &keys,
        text(&c["versionId"]),
        c["gameId"].as_str(),
    ))
}
fn strings(v: &Value) -> Vec<String> {
    array(v).iter().map(|v| text(v).into()).collect()
}
fn trust_model(corpus: &Corpus, input: &Value) -> Value {
    let pin = Pin::from_stored(
        text(&input["pinnedInstanceId"]).into(),
        corpus
            .bytes(&input["pinnedContentPublicKeyHex"])
            .try_into()
            .unwrap(),
    )
    .unwrap();
    let original_pin = pin.clone();
    let mut connections = Connections::new(&pin);
    let mut decisions = Vec::new();
    let mut state = "trusted".to_owned();
    for op in array(&input["operations"]) {
        let connection = text(&op["connection"]);
        match text(&op["kind"]) {
            "open" => {
                connections = connections.open(connection);
                decisions.push(json!({"verified":false}));
            }
            "discovery" => {
                let mut d = Discovery {
                    discovery: 1,
                    product: "impluvium".into(),
                    server_name: "Server".into(),
                    server_version: "1.0.0".into(),
                    instance_id: text(&op["instanceId"]).into(),
                    manifest_formats: vec![3],
                    source_url: "https://reference.invalid/source".into(),
                    required_client_capabilities: strings(&op["requiredCapabilities"]),
                    ..Default::default()
                };
                d.protocol.modify(|p| *p = protocol(&op["protocol"]));
                d.key_fingerprints = array(&op["contentFingerprintHexes"])
                    .iter()
                    .map(|v| KeyFingerprint {
                        role: KeyRole::Content.into(),
                        sha256: corpus.bytes(v),
                        ..Default::default()
                    })
                    .collect();
                let check = trust::check_discovery(
                    connection,
                    &d,
                    Some(&pin),
                    client(&input["client"]),
                    &strings(&input["capabilities"]),
                );
                let (next, result) = match check {
                    Ok(proof) => {
                        let (next, result) = connections.checked(connection, proof);
                        let result = result.map_err(|failure| match failure {
                            VerificationError::Stopped(d) => d,
                            other => panic!("unexpected evidence binding failure {other:?}"),
                        });
                        (next, result)
                    }
                    Err(d) => connections.rejected(connection, d),
                };
                connections = next;
                state = result
                    .err()
                    .map(|d| format!("{:?}", d.reason))
                    .unwrap_or("trusted".into());
                decisions.push(
                    json!({"state":state,"verified":connections.credential_allowed(connection)}),
                );
            }
            "credential" => decisions.push(
                json!({"credentialSent":connections.credential_allowed(connection),"state":state}),
            ),
            other => panic!("unknown trust operation {other}"),
        }
    }
    assert_eq!(pin, original_pin);
    json!({"decisions":decisions,"state":state,"credentialsDeleted":false})
}
fn arithmetic(input: &Value) -> Value {
    let result = array(&input["valuesU64"])
        .iter()
        .enumerate()
        .try_fold(0u64, |sum, (i, v)| {
            impluvium_protocol::manifest::checked_size_add(sum, text(v).parse().unwrap())
                .map_err(|d| (i, d))
        });
    match result {
        Ok(sum) => json!({"sumU64":sum.to_string()}),
        Err((i, d)) => {
            json!({"reason":format!("{:?}",d.reason),"field":d.field,"firstOverflowIndexU64":i.to_string()})
        }
    }
}

#[test]
fn api_contract_corpus() {
    let corpus = Corpus::new();
    let mut counts = BTreeMap::<(String, String), (usize, usize)>::new();
    let mut mismatches = Vec::new();
    for (file, required) in SHARDS {
        let shard = corpus.pinned(file);
        let mut executions = 0;
        for case in array(&shard["cases"]) {
            let operation = text(&case["operation"]);
            let count = counts
                .entry(((*file).into(), operation.into()))
                .or_default();
            if SKIPPED.contains(&operation) {
                count.1 += 1;
                continue;
            }
            count.0 += 1;
            executions += 1;
            let result = catch_unwind(AssertUnwindSafe(|| {
                let input = &case["input"];
                let actual = match operation {
                    "generic-api" | "generic-api-codec" => generic(&corpus, input),
                    "page-size-codec" => page_codec(&corpus, input),
                    "page-size" => {
                        json!({"pageSize":pagination::page_size(input["pageSize"].as_u64().unwrap().try_into().unwrap())})
                    }
                    "discovery" => outcome(api::decode_discovery(&bytes(&corpus, input))),
                    "version-decision" => version(input),
                    "download-response" => download(&corpus, input),
                    "trust-model" => trust_model(&corpus, input),
                    "u64-accumulator-probe" => arithmetic(input),
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
        if executions != *required {
            mismatches.push(format!(
                "{file}: expected {required} executions, observed {executions}"
            ));
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
