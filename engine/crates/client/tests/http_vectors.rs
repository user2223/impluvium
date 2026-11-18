// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: 0BSD

//! Complete client response observations and public transport substages.
//! rpc-exchange and auth-rpc-flow model server authentication, dispatch and
//! transactions as well; they are explicitly counted as outside this adapter.

//!
//! Pinned shard inventory (executed by `client_http_corpus` when built):
//!
//! behavior-000.json
//!
//! - Execute: range=9, http-response=40, transport=6, redirect=5.
//! - Skip: generic-api=30, version-decision=4, event-receive=9, resume=7, session-model=9, pairing-model=4, library-model=2, base-path=17, collision=8, windows-tree=8, discovery=5, download-response=6, reconstruct=2, trust-model=2, size-precheck=24, publication=3.
//!
//! behavior-001.json
//!
//! - Execute: http-response=3.
//! - Skip: publication=2, ticket-cache-model=5, rpc-exchange=24, event-open=11, supersession=6, library-model=2, server-get=13.
//!
//! behavior-auth.json
//!
//! - Execute: none.
//! - Skip: auth-rpc-flow=90.
//!
//! behavior-blake3.json
//!
//! - Execute: blake3-http-response=4.
//! - Skip: blake3-reconstruct=2.
//!
//! behavior-conformance-000.json
//!
//! - Execute: http-response=8, transport=8, redirect=7, blake3-http-response=2.
//! - Skip: generic-api-codec=12, page-size=6, normalize-user-code=6, user-code-input=36, page-size-codec=18, acceptance-set-model=3, event-open-model=2, domain-separation=1, command-budget=2, size-precheck=4, library-model=4, server-get=3.
//!
//! behavior-heads.json
//!
//! - Execute: http-response=4, response-head-size=2.
//! - Skip: none.
//!
//! behavior-priority.json
//!
//! - Execute: transport=31, http-response=24.
//! - Skip: server-get=47, auth-rpc-flow=44, event-open=17, event-open-model=6.
//!
//! behavior-response-regressions.json
//!
//! - Execute: none.
//! - Skip: rpc-exchange=6, auth-rpc-flow=2, library-model=3.
//!
//! behavior-witnesses.json
//!
//! - Execute: none.
//! - Skip: event-receive=5, rpc-exchange=1.

#[allow(dead_code)]
mod support;
use impluvium_client::{
    credentials::{self, Authorization, Prerequisite},
    error::{Diagnostic, Reason},
    response::{self, Binding, Chunk, Outcome, range},
    transport::{self, BaseUrl, DepotTargets, Method, Policy, Purpose, TrustState},
};
use impluvium_protocol::{
    api::{
        self, Codec,
        trust::{ClientVersion, Pin},
    },
    descriptor,
    hash::Algorithm,
    keys::{Content, KeyInput, TrustedKeySet},
    manifest::{self, Profile},
    path::WindowsVariant,
    proto::impluvium::{signed::v1::KeyRole, v2::*},
};
use std::{
    collections::BTreeMap,
    panic::{AssertUnwindSafe, catch_unwind},
};
use support::{
    corpus::{Corpus, array, expected, text},
    json::{Value, b, n, o, s},
};

const SHARDS: &[(&str, usize)] = &[
    ("behavior-000.json", 60),
    ("behavior-001.json", 3),
    ("behavior-auth.json", 0),
    ("behavior-blake3.json", 4),
    ("behavior-conformance-000.json", 25),
    ("behavior-heads.json", 6),
    ("behavior-priority.json", 55),
    ("behavior-response-regressions.json", 0),
    ("behavior-witnesses.json", 0),
];
const SKIPPED: &[&str] = &[
    "acceptance-set-model",
    "auth-rpc-flow",
    "base-path",
    "binary-precheck",
    "blake3",
    "blake3-reconstruct",
    "blake3-signed-document",
    "collision",
    "command-budget",
    "derive-key",
    "discovery",
    "domain-separation",
    "download-response",
    "ed25519-encoding-substage",
    "ed25519-kat",
    "ed25519-point-profile",
    "ed25519-verify-profile",
    "event-open",
    "event-open-model",
    "event-receive",
    "fastcdc",
    "generic-api",
    "generic-api-codec",
    "hkdf-sha256",
    "json-precheck",
    "library-model",
    "normalize-user-code",
    "pae",
    "page-size",
    "page-size-codec",
    "pairing-model",
    "publication",
    "reconstruct",
    "resume",
    "rpc-exchange",
    "scan-binary",
    "scan-json",
    "server-get",
    "session-model",
    "sha256",
    "sha512",
    "signed-document",
    "size-precheck",
    "supersession",
    "ticket-cache-model",
    "trust-model",
    "u64-accumulator-probe",
    "user-code-input",
    "version-decision",
    "windows-tree",
];
fn failure(d: Diagnostic) -> Value {
    o([
        ("reason", s(format!("{:?}", d.reason))),
        ("field", s(d.field)),
    ])
}
fn request_failure(d: Diagnostic) -> Value {
    if d.reason == Reason::invalid_url {
        failure(d)
    } else {
        o([
            ("reason", s(format!("{:?}", d.reason))),
            ("credentialRead", b(false)),
            ("connected", b(false)),
        ])
    }
}
fn state(input: &Value) -> TrustState {
    match input["trustState"].as_str().unwrap_or("trusted") {
        "trusted" => TrustState::Trusted,
        "instance_mismatch" => TrustState::InstanceMismatch,
        "content_key_changed" => TrustState::ContentKeyChanged,
        other => panic!("unknown trust state {other}"),
    }
}
fn flag(input: &Value, key: &str, default: bool) -> bool {
    input[key].as_bool().unwrap_or(default)
}
fn targets(input: &Value) -> DepotTargets {
    let urls: Vec<_> = input["allowedUrls"]
        .as_array()
        .into_iter()
        .flatten()
        .map(text)
        .collect();
    DepotTargets::from_allowed_urls(&urls).unwrap()
}
fn binding(input: &Value) -> Binding {
    match text(&input["purpose"]) {
        "discovery" => Binding::discovery(),
        "rpc" => Binding::rpc(Method::ListLibrary, Codec::Binary),
        "chunk" => {
            let url = input["url"]
                .as_str()
                .or_else(|| input["original"].as_str())
                .unwrap_or("");
            let id = url
                .rsplit('/')
                .next()
                .and_then(|id| support::hex::decode(id).ok())
                .and_then(|id| id.try_into().ok())
                .unwrap_or([0; 32]);
            Binding::chunk(Chunk {
                size: 1,
                id,
                algorithm: Algorithm::Sha256,
                range: None,
            })
            .unwrap()
        }
        "manifest" => Binding::manifest(),
        "descriptor" => Binding::descriptor(),
        "events" => Binding::events(),
        "object" => Binding::object(1, "application/octet-stream"),
        other => panic!("unknown purpose {other}"),
    }
}

/// Some model fixtures supply a chunk-shaped original even for a non-chunk
/// purpose. Prepare a valid request of that purpose before testing redirects.
fn fixture_url(origin: &str, binding: &Binding, chunk_url: &str) -> String {
    match binding.purpose() {
        Purpose::Chunk => chunk_url.into(),
        Purpose::Discovery => format!("{origin}/api/info"),
        Purpose::Rpc(method) => format!("{origin}/api/v2/{}", method.route()),
        Purpose::Manifest => format!("{origin}/api/v2/manifests/{}", "0".repeat(64)),
        Purpose::Descriptor => {
            format!("{origin}/api/v2/descriptors/22222222-2222-2222-2222-222222222222")
        }
        Purpose::Object => {
            format!("{origin}/api/v2/objects/22222222-2222-2222-2222-222222222222")
        }
        Purpose::Events => format!("{origin}/api/v2/events?topics=library"),
    }
}

fn credential_name(purpose: credentials::CredentialPurpose) -> &'static str {
    match purpose {
        credentials::CredentialPurpose::None | credentials::CredentialPurpose::Proof => "none",
        credentials::CredentialPurpose::AccessToken => "access-token",
        credentials::CredentialPurpose::Ticket => "ticket",
    }
}

fn transport_case(input: &Value) -> Value {
    let depots = targets(input);
    let raw_url = text(&input["url"]);
    let fallback = transport::request_url(raw_url)
        .map(|u| u.origin().ascii_serialization())
        .unwrap_or_else(|_| "https://reference.invalid".into());
    let policy = Policy {
        base: input["baseUrl"].as_str().unwrap_or(&fallback),
        platform_trust_available: flag(input, "platformTrustAvailable", true),
        insecure_mode: flag(input, "insecureMode", false),
        depots: &depots,
        trust: state(input),
    };
    let prepared = match transport::preflight(&policy, raw_url, binding(input)) {
        Ok(p) => p,
        Err(d) => return request_failure(d),
    };
    let context = context(policy.base, false);
    if !flag(input, "verifiedConnection", true)
        && prepared.purpose().credential() != credentials::CredentialPurpose::None
        && prepared.purpose() != Purpose::Chunk
    {
        assert!(matches!(
            context.authorize(&prepared, "wire", None).unwrap(),
            Authorization::Pending(Prerequisite::Discovery)
        ));
        return o([
            ("decision", s("verify-connection")),
            ("credentialRead", b(false)),
            ("connected", b(true)),
        ]);
    }
    if prepared.purpose() == Purpose::Chunk && !flag(input, "ticketFromVerifiedConnection", true) {
        assert!(matches!(
            context.authorize(&prepared, "wire", None).unwrap(),
            Authorization::Pending(Prerequisite::AuthenticatedDownload)
        ));
        return o([
            ("decision", s("obtain-authenticated-download")),
            ("credentialRead", b(false)),
            ("connected", b(true)),
        ]);
    }
    o([
        ("decision", s("connect")),
        (
            "credential",
            s(credential_name(prepared.purpose().credential())),
        ),
    ])
}
fn redirect_case(input: &Value) -> Value {
    // The original request was already approved; current policy may now fail.
    let original = text(&input["original"]);
    let origin = transport::request_url(original)
        .unwrap()
        .origin()
        .ascii_serialization();
    let original_targets = DepotTargets::from_allowed_urls(&[original]).unwrap();
    let initial = Policy {
        base: &origin,
        platform_trust_available: true,
        insecure_mode: true,
        depots: &original_targets,
        trust: TrustState::Trusted,
    };
    let binding = binding(input);
    let request_url = fixture_url(&origin, &binding, original);
    let prepared = transport::preflight(&initial, &request_url, binding).unwrap();
    let depots = targets(input);
    let policy = Policy {
        base: &origin,
        platform_trust_available: flag(input, "platformTrustAvailable", true),
        insecure_mode: flag(input, "insecureMode", false),
        depots: &depots,
        trust: state(input),
    };
    match transport::redirect(
        &policy,
        &prepared,
        &transport::request_url(text(&input["current"])).unwrap(),
        input["followed"].as_u64().unwrap() as usize,
        text(&input["location"]),
    ) {
        Ok(p) => o([
            ("decision", s("follow")),
            ("target", s(p.url().as_str())),
            ("credential", s(credential_name(p.purpose().credential()))),
        ]),
        Err(d)
            if matches!(
                d.reason,
                Reason::redirect_limit
                    | Reason::redirect_cross_origin
                    | Reason::redirect_target_not_allowed
                    | Reason::redirect_not_allowed
            ) =>
        {
            o([("reason", s(format!("{:?}", d.reason)))])
        }
        Err(d) => request_failure(d),
    }
}
fn range_case(input: &Value) -> Value {
    match range::normalize(
        text(&input["range"]),
        text(&input["sizeU64"]).parse().unwrap(),
    ) {
        Some(r) => o([
            ("firstU64", s(r.first.to_string())),
            ("lastU64", s(r.last.to_string())),
        ]),
        None => o([("satisfiable", b(false))]),
    }
}
fn heads(corpus: &Corpus, input: &Value) -> Value {
    assert_eq!(input["protocol"], "h2");
    let mut sizes = Vec::new();
    for (i, head) in array(&input["heads"]).iter().enumerate() {
        let fields: Vec<_> = array(head)
            .iter()
            .map(|field| {
                (
                    corpus.bytes(&field["nameHex"]),
                    corpus.bytes(&field["valueHex"]),
                )
            })
            .collect();
        match response::h2_head_size(
            fields
                .iter()
                .map(|(name, value)| (name.as_slice(), value.as_slice())),
        ) {
            Ok(size) => sizes.push(s(size.to_string())),
            Err(d) => {
                let measured = response::h2_header_list_size(
                    fields
                        .iter()
                        .map(|(name, value)| (name.as_slice(), value.as_slice())),
                );
                return o([
                    ("reason", s(format!("{:?}", d.reason))),
                    ("field", s(d.field)),
                    ("headIndex", n(i)),
                    ("measuredU64", s(measured.to_string())),
                ]);
            }
        }
    }
    o([("accepted", b(true)), ("sizesU64", Value::Array(sizes))])
}

fn context(base: &str, verified: bool) -> credentials::Context {
    let key =
        support::hex::decode("d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a")
            .unwrap()
            .try_into()
            .unwrap();
    let pin = Pin::from_stored("33333333-3333-3333-3333-333333333333".into(), key).unwrap();
    let mut context = credentials::Context::new(BaseUrl::parse(base).unwrap(), pin.clone());
    if verified {
        let mut discovery = Discovery {
            discovery: 1,
            product: "impluvium".into(),
            server_name: "Public fixture".into(),
            server_version: "1".into(),
            instance_id: pin.instance_id().into(),
            manifest_formats: vec![3],
            source_url: "https://reference.invalid/source".into(),
            ..Default::default()
        };
        discovery.protocol.modify(|p| {
            p.major = 2;
            p.revision = 1;
            p.min_client_revision = 1;
        });
        let evidence = api::trust::check_discovery(
            "wire",
            &discovery,
            Some(&pin),
            ClientVersion {
                major: 2,
                revision: 1,
                min_server_revision: 1,
            },
            &[],
        )
        .unwrap();
        context.checked("wire", evidence).unwrap();
    }
    context
}

fn request_text(corpus: &Corpus, input: &Value) -> String {
    input
        .get("requestHex")
        .map(|v| String::from_utf8(corpus.bytes(v)).unwrap())
        .unwrap_or_default()
}
fn request_path(request: &str) -> &str {
    request
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or("")
}
fn response_binding(corpus: &Corpus, input: &Value) -> Binding {
    let c = &input["context"];
    match text(&c["purpose"]) {
        "object" => Binding::object(
            c["successLimit"].as_u64().unwrap() as usize,
            text(&c["contentType"]),
        ),
        "rpc" => Binding::rpc(
            Method::ListDevices,
            if c["contentType"] == "application/json" {
                Codec::Json
            } else {
                Codec::Binary
            },
        ),
        "chunk" => {
            let request = request_text(corpus, input);
            let size = c["chunkSize"].as_u64().unwrap_or(1);
            let id = c
                .get("chunkIdHex")
                .map(|v| corpus.bytes(v))
                .unwrap_or_else(|| {
                    support::hex::decode(request_path(&request).rsplit('/').next().unwrap_or(""))
                        .unwrap()
                });
            let id = if id.is_empty() {
                [0; 32]
            } else {
                id.try_into().unwrap()
            };
            let range = request
                .lines()
                .find_map(|line| line.strip_prefix("Range: "))
                .and_then(|r| range::normalize(r.trim(), size));
            Binding::chunk(Chunk {
                size,
                id,
                algorithm: Algorithm::from_wire(c["hashAlgorithm"].as_u64().unwrap_or(1) as i32)
                    .unwrap(),
                range,
            })
            .unwrap()
        }
        _ => binding(c),
    }
}
fn trusted(corpus: &Corpus, c: &Value) -> TrustedKeySet<Content> {
    let bytes: Vec<_> = array(&c["keys"])
        .iter()
        .map(|k| corpus.bytes(&k["publicKeyHex"]))
        .collect();
    let keys: Vec<_> = bytes
        .iter()
        .map(|key| KeyInput {
            public_key: key,
            role: KeyRole::Content,
            ticket_status: None,
        })
        .collect();
    TrustedKeySet::new(&keys).unwrap()
}
fn success(
    corpus: &Corpus,
    input: &Value,
    body: response::CheckedBody,
) -> Result<Value, Diagnostic> {
    let c = &input["context"];
    let request = request_text(corpus, input);
    match body.binding().purpose() {
        Purpose::Chunk => {
            let candidate = c
                .get("completeChunkHex")
                .expect("successful chunk fixture requires a complete candidate");
            if !body.chunk_accepts(&corpus.bytes(candidate)) {
                return Ok(o([
                    ("decision", s("discard-chunk")),
                    ("keepCredentials", b(true)),
                ]));
            }
        }
        Purpose::Manifest => {
            let path = request_path(&request);
            let id: [u8; 32] = if let Some(id) = c.get("manifestIdHex") {
                corpus.bytes(id)
            } else {
                support::hex::decode(path.rsplit('/').next().unwrap()).unwrap()
            }
            .try_into()
            .unwrap();
            body.manifest(&trusted(corpus, c), &id, Profile::Base)?;
        }
        Purpose::Descriptor => {
            let bytes = corpus.bytes(&c["manifestPayloadHex"]);
            let (manifest, _) = manifest::decode_payload(&bytes, Profile::Base)?;
            let context = descriptor::Context {
                expected_version_id: request_path(&request)
                    .rsplit('/')
                    .next()
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| text(&c["versionId"])),
                expected_game_id: c["gameId"].as_str(),
                manifest: &manifest,
                manifest_payload: &bytes,
                known_manifest_id: None,
                known_revision: None,
                windows: WindowsVariant::Installation,
            };
            body.descriptor(&trusted(corpus, c), &context)?;
        }
        Purpose::Discovery => {
            body.discovery()?;
        }
        Purpose::Object => (),
        other => panic!("unexpected successful response purpose {other:?}"),
    }
    Ok(o([("decision", s("accept")), ("keepCredentials", b(true))]))
}
fn http_case(corpus: &Corpus, input: &Value) -> Value {
    let binding = response_binding(corpus, input);
    let raw = corpus.bytes(&input["responseHex"]);
    match response::http1(&binding, &raw) {
        Err(d) => {
            if d.reason == Reason::headers_too_large {
                return o([
                    ("reason", s(format!("{:?}", d.reason))),
                    ("field", s(d.field)),
                    ("keepCredentials", b(true)),
                ]);
            }
            o([
                ("reason", s(format!("{:?}", d.reason))),
                ("keepCredentials", b(true)),
            ])
        }
        Ok(Outcome::Error(error)) => {
            let c = &input["context"];
            let context = context(
                if flag(c, "serverOrigin", false) {
                    "https://reference.invalid"
                } else {
                    "https://other.invalid"
                },
                flag(c, "verifiedConnection", false),
            );
            let origin = "https://reference.invalid";
            let request = request_text(corpus, input);
            let chunk_url = if request_path(&request).is_empty() {
                format!("{origin}/depot/v2/{}/{}", "0".repeat(64), "0".repeat(64))
            } else {
                format!("{origin}{}", request_path(&request))
            };
            let targets = DepotTargets::from_allowed_urls(&[&chunk_url]).unwrap();
            let policy = Policy {
                base: origin,
                platform_trust_available: true,
                insecure_mode: false,
                depots: &targets,
                trust: TrustState::Trusted,
            };
            let url = fixture_url(origin, &binding, &chunk_url);
            let request = transport::preflight(&policy, &url, binding).unwrap();
            let delete = context.cleanup(&request, "wire", &error).is_some();
            o([
                (
                    "error",
                    o([
                        ("code", s(format!("{:?}", error.code()))),
                        ("reason", n(error.info().map_or(0, |i| i.reason.to_i32()))),
                    ]),
                ),
                ("keepCredentials", b(!delete)),
            ])
        }
        Ok(Outcome::Success(body)) => match success(corpus, input, body) {
            Ok(v) => v,
            Err(d) => failure(d),
        },
        _ => panic!("unexpected response observation"),
    }
}

#[test]
fn client_http_corpus() {
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
                    "transport" => transport_case(input),
                    "redirect" => redirect_case(input),
                    "range" => range_case(input),
                    "response-head-size" => heads(&corpus, input),
                    "http-response" | "blake3-http-response" => http_case(&corpus, input),
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

fn raw_response(status: u16, fields: &str, body: &[u8]) -> Vec<u8> {
    let mut out = format!("HTTP/1.1 {status} Fixture\r\n{fields}\r\n").into_bytes();
    out.extend_from_slice(body);
    out
}

#[test]
fn chunk_redirect_checks_every_coding_line_before_exposing_a_target() {
    use std::cell::Cell;
    let url = "https://reference.invalid/depot/v2/0000000000000000000000000000000000000000000000000000000000000000/1111111111111111111111111111111111111111111111111111111111111111";
    let depots = DepotTargets::from_allowed_urls(&[url]).unwrap();
    let policy = Policy {
        base: "https://reference.invalid",
        platform_trust_available: true,
        insecure_mode: false,
        depots: &depots,
        trust: TrustState::Trusted,
    };
    let bound = Binding::chunk(Chunk {
        size: 1,
        id: [0x11; 32],
        algorithm: Algorithm::Sha256,
        range: None,
    })
    .unwrap();
    let original = transport::preflight(&policy, url, bound.clone()).unwrap();
    let fields = format!("Location: {url}\r\nContent-Encoding: identity\r\n");
    let Outcome::Redirect(target) =
        response::http1(&bound, &raw_response(302, &fields, b"")).unwrap()
    else {
        panic!("redirect candidate expected")
    };
    transport::redirect(&policy, &original, original.url(), 0, &target).unwrap();
    let rpc = Binding::rpc(Method::ListLibrary, Codec::Binary);
    for coding in [
        "Content-Encoding: identity\r\nContent-Encoding: gzip\r\n",
        "Content-Encoding: gzip\r\nContent-Encoding: identity\r\n",
        "Content-Encoding: identity, gzip\r\n",
    ] {
        let raw = raw_response(302, &format!("Location: {url}\r\n{coding}"), b"");
        for (bound, reason) in [
            (&bound, Reason::unsupported_content_encoding),
            (&rpc, Reason::redirect_not_allowed),
        ] {
            let calls = Cell::new(0);
            let result = response::decode_with(bound, &raw, |_| {
                calls.set(calls.get() + 1);
                Ok(())
            });
            assert_eq!(result.err().unwrap(), Diagnostic { reason, field: "" });
            assert_eq!(calls.get(), 0);
        }
    }
}

#[test]
fn redirect_adapter_exercises_every_non_chunk_purpose() {
    let original = "https://reference.invalid/depot/v2/0000000000000000000000000000000000000000000000000000000000000000/1111111111111111111111111111111111111111111111111111111111111111";
    for purpose in [
        "discovery",
        "rpc",
        "manifest",
        "descriptor",
        "object",
        "events",
    ] {
        let input = o([
            ("purpose", s(purpose)),
            ("original", s(original)),
            ("current", s(original)),
            ("followed", n(0)),
            ("location", s(original)),
        ]);
        assert_eq!(
            redirect_case(&input),
            o([("reason", s("redirect_not_allowed"))])
        );
    }
}

#[test]
fn http_adapter_always_asks_product_cleanup_for_the_actual_origin() {
    let corpus = Corpus::new();
    let raw = raw_response(
        401,
        "Content-Type: application/json\r\n",
        br#"{"code":"unauthenticated","details":[{"type":"impluvium.v2.ErrorInfo","value":"CAU"}]}"#,
    );
    let cases = [
        (true, true, false),
        (false, true, true),
        (true, false, true),
    ];
    for (server_origin, verified, keep) in cases {
        let input = o([
            ("responseHex", s(support::hex::encode(&raw))),
            (
                "context",
                o([
                    ("purpose", s("discovery")),
                    ("serverOrigin", b(server_origin)),
                    ("verifiedConnection", b(verified)),
                ]),
            ),
        ]);
        assert_eq!(
            http_case(&corpus, &input),
            o([
                (
                    "error",
                    o([("code", s("unauthenticated")), ("reason", n(5))]),
                ),
                ("keepCredentials", b(keep)),
            ])
        );
    }
}

#[test]
#[should_panic(expected = "successful chunk fixture requires a complete candidate")]
fn chunk_adapter_cannot_accept_without_a_complete_candidate() {
    let corpus = Corpus::new();
    let raw = raw_response(200, "Content-Type: application/octet-stream\r\n", &[0]);
    let input = o([
        ("responseHex", s(support::hex::encode(&raw))),
        (
            "context",
            o([
                ("purpose", s("chunk")),
                ("chunkSize", n(1)),
                (
                    "chunkIdHex",
                    s(support::hex::encode(Algorithm::Sha256.digest(&[0]))),
                ),
            ]),
        ),
    ]);
    http_case(&corpus, &input);
}

#[test]
fn h2_error_measurement_comes_from_the_product_counter() {
    let value = vec![b'x'; response::HEAD_LIMIT];
    let fields = [(b"x".as_slice(), value.as_slice())];
    assert_eq!(response::h2_header_list_size(fields), 65_569);
    assert_eq!(
        response::h2_head_size(fields).unwrap_err(),
        Diagnostic {
            reason: Reason::headers_too_large,
            field: "",
        }
    );
}

#[test]
fn early_rejection_never_calls_the_decoder() {
    use std::cell::Cell;
    let bound = Binding::object(1, "application/octet-stream");
    let oversized_head = vec![b'X'; response::HEAD_LIMIT + 1];
    let cases = [
        (oversized_head, Reason::headers_too_large),
        (
            raw_response(302, "Content-Encoding: gzip\r\n", b""),
            Reason::redirect_not_allowed,
        ),
        (
            raw_response(
                413,
                "Content-Encoding: identity\r\nContent-Encoding: gzip\r\n",
                b"",
            ),
            Reason::unsupported_content_encoding,
        ),
        (
            raw_response(413, "Content-Encoding: gzip, identity\r\n", b""),
            Reason::unsupported_content_encoding,
        ),
        (
            raw_response(
                200,
                "Content-Type: application/octet-stream\r\nContent-Length: 0\r\n",
                b"ab",
            ),
            Reason::response_too_large,
        ),
        (
            raw_response(401, "Content-Type: text/html\r\n", b"revoked"),
            Reason::broken_error_body,
        ),
        (
            raw_response(
                429,
                "Content-Type: application/json\r\n",
                b"{\"code\":\"unauthenticated\",\"code\":\"unknown\"}",
            ),
            Reason::rate_limited,
        ),
        (
            raw_response(
                503,
                "Content-Type: application/json\r\n",
                &vec![b' '; response::ERROR_LIMIT + 1],
            ),
            Reason::server_unavailable,
        ),
    ];
    for (raw, reason) in cases {
        let calls = Cell::new(0);
        let result = response::decode_with(&bound, &raw, |_| {
            calls.set(calls.get() + 1);
            Ok(())
        });
        assert_eq!(result.err().unwrap(), Diagnostic { reason, field: "" });
        assert_eq!(calls.get(), 0);
    }
    let calls = Cell::new(0);
    let valid = raw_response(200, "Content-Type: application/octet-stream\r\n", b"a");
    assert!(
        response::decode_with(&bound, &valid, |_| {
            calls.set(calls.get() + 1);
            Ok(())
        })
        .unwrap()
        .is_ok()
    );
    assert_eq!(calls.get(), 1);
}

#[test]
fn abandoned_body_cannot_be_finished_or_resumed() {
    let bound = Binding::object(1, "application/octet-stream");
    let fields = [response::Header {
        name: "content-type",
        value: "application/octet-stream",
    }];
    let response::HeadDecision::Body(mut reader) =
        response::head(&bound, 40, 200, &fields).unwrap()
    else {
        panic!("body expected")
    };
    reader.push(b"a").unwrap();
    let failure = reader.push(b"b").unwrap_err();
    assert_eq!(reader.remaining(), 0);
    assert_eq!(reader.push(b"").unwrap_err(), failure);
    assert_eq!(reader.finish().err().unwrap(), failure);
}

#[test]
fn method_binding_has_no_fallback_and_preserves_schema_context() {
    let methods = [
        Method::StartPairing,
        Method::PollPairing,
        Method::BeginSession,
        Method::CompleteSession,
        Method::RevokeSelf,
        Method::ListDevices,
        Method::RevokeDevice,
        Method::ListLibrary,
        Method::GetGame,
        Method::ListVersions,
        Method::GetVersion,
        Method::OpenDownload,
        Method::RefreshTicket,
    ];
    for method in methods {
        assert_eq!(Method::from_route(method.route()), Some(method));
        for codec in [Codec::Binary, Codec::Json] {
            assert_eq!(
                Binding::rpc(method, codec).success_limit(),
                method.response_limit()
            );
        }
    }
    assert_eq!(Method::from_route("impluvium.v2.AuthService/Unknown"), None);
    assert_eq!(
        Method::StartPairing.credential(),
        credentials::CredentialPurpose::None
    );
    assert_eq!(
        Method::PollPairing.credential(),
        credentials::CredentialPurpose::Proof
    );
    assert_eq!(
        Method::BeginSession.credential(),
        credentials::CredentialPurpose::None
    );
    assert_eq!(
        Method::CompleteSession.credential(),
        credentials::CredentialPurpose::Proof
    );
    let raw = raw_response(
        200,
        "Content-Type: application/json; charset=utf-8\r\n",
        b"{}",
    );
    let Outcome::Success(body) =
        response::http1(&Binding::rpc(Method::RevokeSelf, Codec::Json), &raw).unwrap()
    else {
        panic!("success expected")
    };
    body.generic::<RevokeSelfResponse>(()).unwrap();
    assert_eq!(
        body.generic::<ListLibraryResponse>(()).unwrap_err().reason,
        Reason::broken_error_body
    );
    let Outcome::Success(body) =
        response::http1(&Binding::rpc(Method::CompleteSession, Codec::Json), &raw).unwrap()
    else {
        panic!("success expected")
    };
    assert_eq!(
        body.generic::<CompleteSessionResponse>(())
            .unwrap_err()
            .reason,
        Reason::missing_field
    );
}

#[test]
fn origin_normalization_and_destination_checks_precede_credentials() {
    assert_eq!(
        transport::Origin::parse("https://BÜCHER.example:443/path").unwrap(),
        transport::Origin::parse("https://xn--bcher-kva.example").unwrap()
    );
    assert_eq!(
        transport::Origin::parse("http://0x7f000001").unwrap(),
        transport::Origin::parse("http://127.0.0.1:80").unwrap()
    );
    assert_eq!(
        BaseUrl::parse("https://Example.invalid/prefix/")
            .unwrap()
            .serialized(),
        "https://example.invalid/prefix"
    );
    assert!(BaseUrl::parse("https://example.invalid/prefix//").is_err());
    for invalid in [
        "https://user@example.invalid",
        "file:///secret",
        "https://example.invalid/#fragment",
    ] {
        assert_eq!(
            transport::request_url(invalid).unwrap_err().reason,
            Reason::invalid_url
        );
    }
    let bad = "https://reference.invalid/api/v2/impluvium.v2.LibraryService/ListLibrary";
    let targets = DepotTargets::from_allowed_urls(&[bad]).unwrap();
    let policy = Policy {
        base: "https://reference.invalid",
        platform_trust_available: true,
        insecure_mode: false,
        depots: &targets,
        trust: TrustState::Trusted,
    };
    assert_eq!(
        transport::preflight(
            &policy,
            bad,
            Binding::chunk(Chunk {
                size: 1,
                id: [0; 32],
                algorithm: Algorithm::Sha256,
                range: None
            })
            .unwrap()
        )
        .unwrap_err()
        .reason,
        Reason::origin_not_allowed
    );
    assert_eq!(
        transport::CONNECTION_MODE,
        transport::ConnectionMode::Direct
    );
}

#[test]
fn credential_reads_and_cleanup_require_pin_bound_connection_evidence() {
    struct Store {
        reads: usize,
        deletes: usize,
    }
    impl credentials::CredentialSource for Store {
        type Error = ();
        type Credential = ();
        fn read(
            &mut self,
            _: credentials::Identity<'_>,
            purpose: credentials::CredentialPurpose,
        ) -> Result<(), ()> {
            assert_eq!(purpose, credentials::CredentialPurpose::AccessToken);
            self.reads += 1;
            Ok(())
        }
        fn delete(&mut self, _: &Pin) -> Result<(), ()> {
            self.deletes += 1;
            Ok(())
        }
    }
    let depots = DepotTargets::default();
    let policy = Policy {
        base: "https://reference.invalid",
        platform_trust_available: true,
        insecure_mode: false,
        depots: &depots,
        trust: TrustState::Trusted,
    };
    let request = transport::preflight(
        &policy,
        "https://reference.invalid/api/v2/impluvium.v2.AuthService/RevokeSelf",
        Binding::rpc(Method::RevokeSelf, Codec::Json),
    )
    .unwrap();
    let mut store = Store {
        reads: 0,
        deletes: 0,
    };
    let mut context = context(policy.base, true);
    assert!(matches!(
        context.authorize(&request, "other", None).unwrap(),
        Authorization::Pending(Prerequisite::Discovery)
    ));
    let Authorization::Read(permit) = context.authorize(&request, "wire", None).unwrap() else {
        panic!("read permission expected")
    };
    permit.read(&mut store).unwrap();
    assert_eq!(store.reads, 1);
    let raw = raw_response(503,"Content-Type: application/json\r\n",br#"{"code":"unauthenticated","details":[{"type":"impluvium.v2.ErrorInfo","value":"CAU"}]}"#);
    let Outcome::Error(error) = response::http1(request.binding(), &raw).unwrap() else {
        panic!("checked error expected")
    };
    assert!(context.cleanup(&request, "other", &error).is_none());
    context
        .cleanup(&request, "wire", &error)
        .unwrap()
        .apply(&mut store)
        .unwrap();
    assert_eq!(store.deletes, 1);
    context.open("wire");
    assert!(context.cleanup(&request, "wire", &error).is_none());
    assert!(matches!(
        context.authorize(&request, "wire", None).unwrap(),
        Authorization::Pending(Prerequisite::Discovery)
    ));
    context
        .rejected(
            "wire",
            Diagnostic {
                reason: Reason::content_key_changed,
                field: "Discovery.key_fingerprints",
            },
        )
        .unwrap_err();
    assert_eq!(
        context.authorize(&request, "wire", None).err().unwrap(),
        Diagnostic {
            reason: Reason::content_key_changed,
            field: ""
        }
    );
    assert_eq!(store.deletes, 1);
    assert_eq!(
        credentials::ticket_header(&vec![0; 4096]).unwrap().len(),
        5469
    );
    assert!(credentials::ticket_header(&vec![0; 4097]).is_none());
    assert_eq!(
        credentials::ticket_header(&[0xfb, 0xff]).unwrap(),
        "Bearer -_8"
    );
}

#[test]
fn ticket_provenance_is_created_only_from_a_checked_response_of_this_context() {
    let depots = DepotTargets::from_allowed_urls(&["https://reference.invalid/depot/v2/0000000000000000000000000000000000000000000000000000000000000000/1111111111111111111111111111111111111111111111111111111111111111"]).unwrap();
    let policy = Policy {
        base: "https://reference.invalid",
        platform_trust_available: true,
        insecure_mode: false,
        depots: &depots,
        trust: TrustState::Trusted,
    };
    let request = transport::preflight(
        &policy,
        "https://reference.invalid/api/v2/impluvium.v2.DownloadService/RefreshTicket",
        Binding::rpc(Method::RefreshTicket, Codec::Json),
    )
    .unwrap();
    let raw = raw_response(
        200,
        "Content-Type: application/json\r\n",
        br#"{"ticket":"AQ","manifestIds":["AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"]}"#,
    );
    let Outcome::Success(body) = response::http1(request.binding(), &raw).unwrap() else {
        panic!("checked response expected")
    };
    let mut context = context(policy.base, true);
    let (_, ticket) = context.refresh_response(&request, "wire", &body).unwrap();
    let ticket = ticket.unwrap();
    let chunk = transport::preflight(
        &policy,
        depots.urls()[0].as_str(),
        Binding::chunk(Chunk {
            size: 1,
            id: [0x11; 32],
            algorithm: Algorithm::Sha256,
            range: None,
        })
        .unwrap(),
    )
    .unwrap();
    assert!(
        matches!(context.authorize(&chunk,"unverified-depot",Some(&ticket)).unwrap(),Authorization::Ticket(bytes) if bytes == b"\x01")
    );
    let replacement = credentials::Context::new(context.base().clone(), context.pin().clone());
    assert!(matches!(
        replacement
            .authorize(&chunk, "unverified-depot", Some(&ticket))
            .unwrap(),
        Authorization::Pending(Prerequisite::AuthenticatedDownload)
    ));
    let (_, unverified_ticket) = context.refresh_response(&request, "other", &body).unwrap();
    assert!(unverified_ticket.is_none());
    context
        .rejected(
            "wire",
            Diagnostic {
                reason: Reason::instance_mismatch,
                field: "Discovery.instance_id",
            },
        )
        .unwrap_err();
    assert_eq!(
        context
            .authorize(&chunk, "unverified-depot", Some(&ticket))
            .err()
            .unwrap()
            .reason,
        Reason::instance_mismatch
    );
}

#[test]
fn redirect_origin_changes_and_count_are_checked_before_new_preflight() {
    let url = "https://reference.invalid/depot/v2/0000000000000000000000000000000000000000000000000000000000000000/1111111111111111111111111111111111111111111111111111111111111111";
    let depots = DepotTargets::from_allowed_urls(&[url]).unwrap();
    let initial = Policy {
        base: "https://reference.invalid",
        platform_trust_available: true,
        insecure_mode: false,
        depots: &depots,
        trust: TrustState::Trusted,
    };
    let original = transport::preflight(
        &initial,
        url,
        Binding::chunk(Chunk {
            size: 1,
            id: [0x11; 32],
            algorithm: Algorithm::Sha256,
            range: None,
        })
        .unwrap(),
    )
    .unwrap();
    let unavailable = Policy {
        platform_trust_available: false,
        ..initial
    };
    for target in [
        url.replace("https:", "http:"),
        url.replace("reference.invalid", "other.invalid"),
        url.replace("reference.invalid", "reference.invalid:444"),
    ] {
        assert_eq!(
            transport::redirect(&unavailable, &original, original.url(), 0, &target)
                .unwrap_err()
                .reason,
            Reason::redirect_cross_origin
        );
    }
    assert_eq!(
        transport::redirect(&unavailable, &original, original.url(), 5, "https://[")
            .unwrap_err()
            .reason,
        Reason::redirect_limit
    );
    assert_eq!(
        transport::redirect(&unavailable, &original, original.url(), 4, "https://[")
            .unwrap_err()
            .reason,
        Reason::invalid_url
    );
    assert_eq!(
        transport::redirect(&unavailable, &original, original.url(), 4, url)
            .unwrap_err()
            .reason,
        Reason::platform_trust_unavailable
    );
}

#[test]
fn pairing_proofs_require_unpinned_discovery_on_the_same_connection() {
    let depots = DepotTargets::default();
    let policy = Policy {
        base: "https://reference.invalid",
        platform_trust_available: true,
        insecure_mode: false,
        depots: &depots,
        trust: TrustState::Trusted,
    };
    let request = transport::preflight(
        &policy,
        "https://reference.invalid/api/v2/impluvium.v2.AuthService/PollPairing",
        Binding::rpc(Method::PollPairing, Codec::Json),
    )
    .unwrap();
    let mut pairing = credentials::PairingContext::new(BaseUrl::parse(policy.base).unwrap());
    assert!(matches!(
        pairing.authorize(&request, "pairing-wire").unwrap(),
        Authorization::Pending(Prerequisite::Discovery)
    ));
    let mut discovery = Discovery {
        discovery: 1,
        product: "impluvium".into(),
        server_name: "Public fixture".into(),
        server_version: "1".into(),
        instance_id: "33333333-3333-3333-3333-333333333333".into(),
        manifest_formats: vec![3],
        source_url: "https://reference.invalid/source".into(),
        ..Default::default()
    };
    discovery.protocol.modify(|p| {
        p.major = 2;
        p.revision = 1;
        p.min_client_revision = 1;
    });
    let evidence = api::trust::check_discovery(
        "pairing-wire",
        &discovery,
        None,
        ClientVersion {
            major: 2,
            revision: 1,
            min_server_revision: 1,
        },
        &[],
    )
    .unwrap();
    pairing.checked("pairing-wire", evidence).unwrap();
    assert!(matches!(
        pairing.authorize(&request, "other").unwrap(),
        Authorization::Pending(Prerequisite::Discovery)
    ));
    struct ProofSource;
    impl credentials::CredentialSource for ProofSource {
        type Error = ();
        type Credential = ();
        fn read(
            &mut self,
            identity: credentials::Identity<'_>,
            purpose: credentials::CredentialPurpose,
        ) -> Result<(), ()> {
            assert_eq!(purpose, credentials::CredentialPurpose::Proof);
            let credentials::Identity::Pairing { instance_id, .. } = identity else {
                panic!("pairing identity expected")
            };
            assert_eq!(instance_id, "33333333-3333-3333-3333-333333333333");
            Ok(())
        }
        fn delete(&mut self, _: &Pin) -> Result<(), ()> {
            panic!("pairing does not delete pinned credentials")
        }
    }
    let Authorization::Read(permit) = pairing.authorize(&request, "pairing-wire").unwrap() else {
        panic!("proof permission expected")
    };
    permit.read(&mut ProofSource).unwrap();
    pairing.open();
    assert!(matches!(
        pairing.authorize(&request, "pairing-wire").unwrap(),
        Authorization::Pending(Prerequisite::Discovery)
    ));
}

#[test]
fn range_decimal_overflow_clips_bounds_without_wrapping() {
    assert_eq!(
        range::normalize("BYTES=1-2", 4),
        Some(range::ByteRange { first: 1, last: 2 })
    );
    assert_eq!(
        range::normalize("bytes=0-18446744073709551616", 4),
        Some(range::ByteRange { first: 0, last: 3 })
    );
    assert_eq!(
        range::normalize("bytes=-18446744073709551616", 4),
        Some(range::ByteRange { first: 0, last: 3 })
    );
    assert_eq!(
        range::normalize("bytes=18446744073709551616-", u64::MAX),
        None
    );
    assert_eq!(range::normalize("bytes=0-0", 0), None);
    assert_eq!(
        impluvium_client::error::retry_after("0000086400"),
        Some(86_400)
    );
    for invalid in [
        "86401",
        "+1",
        "1.0",
        "Thu, 01 Jan 2026 00:00:00 GMT",
        "00000000000",
    ] {
        assert_eq!(impluvium_client::error::retry_after(invalid), None);
    }
}
