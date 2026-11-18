// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: 0BSD

//! Full signed proof consumers and the explicitly injected state/input substages.

use impluvium_protocol::{
    decode::{Diagnostic, Reason},
    envelope,
    keys::{Device, KeyInput, SigningKey, TrustedKeySet},
    proof::{
        self, PairingContext, SessionContext, pairing,
        session::{self, Time},
        user_code::{self, UserCode},
    },
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

const SHARDS: &[(&str, usize)] = &[
    ("signed-000.json", 24),
    ("signed-ed25519.json", 21),
    ("signed-witnesses.json", 1),
    ("behavior-000.json", 13),
    ("behavior-conformance-000.json", 43),
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
    "library-model",
    "base-path",
    "collision",
    "windows-tree",
    "discovery",
    "download-response",
    "reconstruct",
    "trust-model",
    "size-precheck",
    "publication",
    "generic-api-codec",
    "page-size",
    "page-size-codec",
    "acceptance-set-model",
    "event-open-model",
    "blake3-http-response",
    "command-budget",
    "server-get",
];
fn time(v: &Value) -> Time {
    Time {
        seconds: text(&v["seconds"]).parse().unwrap(),
        nanos: v["nanos"].as_u64().unwrap() as u32,
    }
}
fn time_json(t: Time) -> Value {
    json!({"seconds":t.seconds.to_string(), "nanos":t.nanos})
}
fn failure(d: Diagnostic) -> Value {
    json!({"reason":format!("{:?}",d.reason),"field":d.field})
}
fn observation(valid: &Value) -> Result<(), Diagnostic> {
    if valid.as_bool().unwrap() {
        Ok(())
    } else {
        Err(Diagnostic {
            reason: Reason::invalid_signature,
            field: "Signature.sig",
        })
    }
}
fn outcome(result: Result<(), Diagnostic>) -> String {
    match result {
        Ok(()) => "success".into(),
        Err(d) => format!("{:?}", d.reason),
    }
}
fn signed(corpus: &Corpus, input: &Value) -> Value {
    let raw = array(&input["keys"])
        .iter()
        .map(|k| corpus.bytes(&k["publicKeyHex"]))
        .collect::<Vec<_>>();
    let inputs = array(&input["keys"])
        .iter()
        .zip(&raw)
        .map(|(k, bytes)| KeyInput {
            public_key: bytes,
            role: match text(&k["role"]) {
                "device" => KeyRole::Device,
                "content" => KeyRole::Content,
                "depot-ticket" => KeyRole::DepotTicket,
                "wrong-role" => KeyRole::Unspecified,
                other => panic!("unknown key role {other}"),
            },
            ticket_status: None,
        })
        .collect::<Vec<_>>();
    let trusted = match TrustedKeySet::<Device>::new(&inputs) {
        Ok(t) => t,
        Err(d) => return json!({"reason":format!("{:?}",d.reason),"field":d.field}),
    };
    let bytes = corpus.bytes(&input["envelopeHex"]);
    let ctx = &input["context"];
    let result = match text(&input["purpose"]) {
        "session" => {
            // The invalid-payload-type witness omits context because it must
            // fail before payload access. These placeholders cannot accept it.
            let challenge = ctx
                .get("challengeHex")
                .map(|v| corpus.bytes(v).try_into().unwrap())
                .unwrap_or([0; 32]);
            proof::verify_session(
                &bytes,
                &trusted,
                &SessionContext {
                    instance_id: ctx["instanceId"].as_str().unwrap_or(""),
                    device_id: ctx["deviceId"].as_str().unwrap_or(""),
                    challenge: &challenge,
                },
            )
            .map(|_| ())
        }
        "pairing" => {
            let id = corpus.bytes(&ctx["pairingIdHex"]).try_into().unwrap();
            proof::verify_pairing(
                &bytes,
                &trusted,
                &PairingContext {
                    instance_id: text(&ctx["instanceId"]),
                    pairing_id: &id,
                },
            )
            .map(|_| ())
        }
        other => panic!("unknown proof purpose {other}"),
    };
    match result {
        Ok(()) => json!({"accepted":true,"warnings":[]}),
        Err(d) => failure(d),
    }
}
fn session_model(input: &Value) -> Value {
    let s = &input["state"];
    let mut state = session::State {
        device: session::Device {
            device_id: text(&s["deviceId"]).into(),
            registered: true,
            revoked: s["revoked"].as_bool().unwrap(),
            activity_at: time(&s["activityAt"]),
            idle_seconds: text(&s["idleSeconds"]).parse().unwrap(),
        },
        challenges: s
            .get("challenge")
            .filter(|v| !v.is_null())
            .map(|c| {
                vec![session::Challenge {
                    value: [1; 32],
                    device_id: text(&c["deviceId"]).into(),
                    expires_at: time(&c["expiresAt"]),
                    consumed: c["consumed"].as_bool().unwrap(),
                }]
            })
            .unwrap_or_default(),
        capacity: if s["storeFull"].as_bool().unwrap() {
            1
        } else {
            2
        },
    };
    let mut results = Vec::new();
    for op in array(&input["operations"]) {
        match op["kind"].as_str().unwrap_or("complete") {
            "restart" => {
                state = state.restart(match text(&op["policy"]) {
                    "preserve" => session::Restart::Preserve,
                    "forget" => session::Restart::Forget,
                    other => panic!("unknown restart policy {other}"),
                });
                results.push("restarted".to_owned());
            }
            "begin" => {
                let (next, result) = state.begin(
                    Time {
                        seconds: 0,
                        nanos: 0,
                    },
                    [2; 32],
                );
                state = next;
                results.push(match result {
                    Ok(()) => "issued".into(),
                    Err(d) => format!("{:?}", d.reason),
                });
            }
            "complete" => {
                let now = time(&op["now"]);
                let (next, result) = state.complete(
                    &[1; 32],
                    observation(&op["proofValid"]),
                    now,
                    |mut current| {
                        if op["atomicRevoked"] == true {
                            current.device.revoked = true;
                        }
                        if op["atomicConsumed"] == true
                            && let Some(c) = current.challenges.first_mut()
                        {
                            c.consumed = true;
                        }
                        (current, op.get("atomicNow").map(time).unwrap_or(now))
                    },
                );
                state = next;
                results.push(outcome(result));
            }
            other => panic!("unknown session operation {other}"),
        }
    }
    json!({
        "results":results,
        "consumed":state.challenges.first().is_some_and(|c| c.consumed),
        "activityAt":time_json(state.device.activity_at),
        "challengePresent":!state.challenges.is_empty(),
    })
}
fn pairing_model(input: &Value) -> Value {
    let s = &input["state"];
    let mut state = pairing::State {
        exists: s["exists"].as_bool().unwrap(),
        completed: s["completed"].as_bool().unwrap(),
        approved: s["approved"].as_bool().unwrap(),
        denied: s["denied"].as_bool().unwrap(),
        expires_at: time(&s["expiresAt"]),
        interval_seconds: s["intervalSeconds"].as_u64().unwrap(),
        last_accepted_poll: s.get("lastPoll").map(time),
        device_registered: false,
    };
    let mut results = Vec::new();
    for poll in array(&input["polls"]) {
        let now = time(&poll["now"]);
        let (next, result) = state.poll(observation(&poll["proofValid"]), now, |mut current| {
            if poll["atomicCompleted"] == true {
                current.completed = true;
            }
            if poll["atomicDeleted"] == true {
                current.exists = false;
            }
            if poll["atomicDenied"] == true {
                current.denied = true;
            }
            if poll["atomicApprovalRemoved"] == true {
                current.approved = false;
            }
            (current, poll.get("atomicNow").map(time).unwrap_or(now))
        });
        state = next;
        results.push(outcome(result));
    }
    json!({
        "results":results,
        "completed":state.completed,
        "intervalSeconds":state.interval_seconds,
    })
}
fn user_input(corpus: &Corpus, input: &Value, validate: bool) -> Value {
    let input = String::from_utf8(corpus.bytes(&input["textHex"])).unwrap();
    let normalized = user_code::normalize(&input);
    if validate {
        json!({"valid":UserCode::from_normalized(&normalized).is_ok(),"normalized":normalized})
    } else {
        json!({"normalized":normalized})
    }
}
fn domain(corpus: &Corpus, input: &Value) -> Value {
    let left = corpus.bytes(&input["leftTypeHex"]);
    let right = corpus.bytes(&input["rightTypeHex"]);
    let body = corpus.bytes(&input["bodyHex"]);
    let l = envelope::pae(&left, &body).unwrap();
    let r = envelope::pae(&right, &body).unwrap();
    let key = SigningKey::<Device>::from_seed(&corpus.bytes(&input["seedHex"]).try_into().unwrap());
    let ls = key.sign(&l);
    let rs = key.sign(&r);
    json!({
        "equalTypeLengths":left.len()==right.len(),
        "paeDifferent":l!=r,
        "signatureDifferent":ls!=rs,
        "crossVerification":envelope::verify_ed25519(&key.public_key(), &r, &ls).is_ok(),
    })
}

#[test]
fn proof_payload_and_state_corpus() {
    let corpus = Corpus::new();
    let mut counts: BTreeMap<(String, String), (usize, usize)> = BTreeMap::new();
    let mut mismatches = Vec::new();
    for (file, required) in SHARDS {
        let shard = corpus.pinned(file);
        let mut executed_file = 0;
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
                    "session" | "pairing" => false,
                    "manifest" | "descriptor" | "ticket" => true,
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
            executed_file += 1;
            let result = catch_unwind(AssertUnwindSafe(|| {
                let actual = match operation {
                    "signed-document" | "blake3-signed-document" => signed(&corpus, input),
                    "session-model" => session_model(input),
                    "pairing-model" => pairing_model(input),
                    "normalize-user-code" => user_input(&corpus, input, false),
                    "user-code-input" => user_input(&corpus, input, true),
                    "domain-separation" => domain(&corpus, input),
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
        if executed_file != *required {
            mismatches.push(format!(
                "{file}: expected {required} executions, observed {executed_file}"
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
