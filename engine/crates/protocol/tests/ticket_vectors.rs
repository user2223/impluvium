// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: 0BSD

//! Ticket payload consumers and explicitly injected cache/acceptance substages.
//! Transport, RPC and other signed purposes remain outside this consumer.

use impluvium_protocol::{
    decode::{Diagnostic, Reason},
    keys::{DepotTicket, KeyInput, SwitchTime, TicketStatus, TrustedKeySet},
    proto::impluvium::signed::v1::KeyRole,
    ticket::{self, Context, DepotTicketClaims, Time, ValidKey, acceptance, cache},
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
    ("signed-000.json", 23),
    ("signed-conformance-000.json", 2),
    ("behavior-001.json", 5),
    ("behavior-conformance-000.json", 3),
];
const SKIPPED: &[&str] = &[
    "publication",
    "rpc-exchange",
    "event-open",
    "supersession",
    "library-model",
    "http-response",
    "server-get",
    "generic-api-codec",
    "transport",
    "redirect",
    "page-size",
    "normalize-user-code",
    "user-code-input",
    "page-size-codec",
    "event-open-model",
    "blake3-http-response",
    "domain-separation",
    "command-budget",
    "size-precheck",
];
fn time(v: &Value) -> Time {
    Time {
        seconds: text(&v["seconds"]).parse().unwrap(),
        nanos: v["nanos"].as_u64().unwrap() as u32,
    }
}
fn status(v: &Value) -> TicketStatus {
    match v["status"].as_str() {
        Some("current") => TicketStatus::Current,
        Some("retiring") => TicketStatus::Retiring {
            switch_time: v.get("switchTime").map(time),
        },
        other => panic!("unknown ticket status {other:?}"),
    }
}
fn context(v: &Value) -> Context<'_> {
    Context {
        now: time(&v["now"]),
        audience: text(&v["audience"]),
        subject: v["subject"].as_str(),
        user_id: v["userId"].as_str(),
    }
}
fn failure(d: Diagnostic) -> Value {
    json!({"reason":format!("{:?}",d.reason), "field":d.field})
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
                "content" => KeyRole::Content,
                "device" => KeyRole::Device,
                "depot-ticket" => KeyRole::DepotTicket,
                "wrong-role" => KeyRole::Unspecified,
                other => panic!("unknown key role {other}"),
            },
            ticket_status: Some(status(k)),
        })
        .collect::<Vec<_>>();
    let trusted = match TrustedKeySet::<DepotTicket>::new(&inputs) {
        Ok(v) => v,
        Err(d) => return json!({"reason":format!("{:?}",d.reason),"field":d.field}),
    };
    let bytes = corpus.bytes(&input["envelopeHex"]);
    let ctx = &input["context"];
    let id = corpus.bytes(&ctx["manifestIdHex"]).try_into().unwrap();
    match ticket::verify(&bytes, &trusted, &context(ctx), &id) {
        Ok(_) => json!({"accepted":true,"warnings":[]}),
        Err(d) => failure(d),
    }
}
fn claims(corpus: &Corpus, v: &Value) -> DepotTicketClaims {
    let mut c = DepotTicketClaims {
        aud: text(&v["aud"]).into(),
        sub: text(&v["sub"]).into(),
        user_id: text(&v["user_id"]).into(),
        manifest_ids: array(&v["manifest_ids"])
            .iter()
            .map(|id| corpus.bytes(id))
            .collect(),
        ..Default::default()
    };
    let start = time(&v["iat"]);
    c.iat.modify(|t| {
        t.seconds = start.seconds;
        t.nanos = start.nanos as i32;
    });
    let end = time(&v["exp"]);
    c.exp.modify(|t| {
        t.seconds = end.seconds;
        t.nanos = end.nanos as i32;
    });
    c
}
fn cache_model(corpus: &Corpus, input: &Value) -> Value {
    // FORMAT.md explicitly injects these claims and signature observations;
    // opaque ticket bytes here are identities, not signed known answers.
    let claims = claims(corpus, &input["claims"]);
    let mut state = cache::State::<(DepotTicketClaims, Vec<ValidKey>)>::default();
    let mut results = Vec::new();
    for request in array(&input["requests"]) {
        let bytes = corpus.bytes(&request["ticketHex"]);
        let role = match text(&request["role"]) {
            "depot-ticket" => KeyRole::DepotTicket,
            "content" => KeyRole::Content,
            "device" => KeyRole::Device,
            other => panic!("unknown cache role {other}"),
        };
        let ctx = context(&request["context"]);
        let id = corpus
            .bytes(&request["context"]["manifestIdHex"])
            .try_into()
            .unwrap();
        let keys = array(&request["validKeys"])
            .iter()
            .enumerate()
            .map(|(i, k)| ValidKey {
                key_id: [i as u8; 8],
                status: status(k),
            })
            .collect::<Vec<_>>();
        let (next, hit, result) = state.step(
            cache::Identity {
                ticket: &bytes,
                role,
                generation: text(&request["generationU64"]).parse().unwrap(),
            },
            ctx.now,
            || {
                if !request["freshVerificationValid"].as_bool().unwrap() {
                    let reason = match text(&request["freshReason"]) {
                        "no_trusted_signature" => Reason::no_trusted_signature,
                        "malformed_wire" => Reason::malformed_wire,
                        other => panic!("unknown injected fresh reason {other}"),
                    };
                    return Err(Diagnostic { reason, field: "" });
                }
                Ok((
                    (claims.clone(), keys),
                    Time {
                        seconds: claims.exp.seconds,
                        nanos: claims.exp.nanos as u32,
                    },
                ))
            },
            |(claims, keys)| {
                ticket::validate_claims(claims, keys, &ctx)?;
                ticket::check_scope(claims, &id)
            },
        );
        state = next;
        results.push(match result {
            Ok(_) => json!({"hit":hit,"accepted":true}),
            Err(d) => json!({"hit":hit,"reason":format!("{:?}",d.reason)}),
        });
    }
    json!({"results":results})
}
fn acceptance_model(input: &Value) -> Value {
    let mut state = acceptance::State {
        generation: text(&input["generationU64"]).parse().unwrap(),
        keys: array(&input["keys"])
            .iter()
            .map(|k| acceptance::Entry {
                key: text(&k["id"]).to_owned(),
                status: match text(&k["status"]) {
                    "current" => TicketStatus::Current,
                    "retiring" => TicketStatus::Retiring {
                        switch_time: Some(SwitchTime {
                            seconds: text(&k["switchU64"]).parse().unwrap(),
                            nanos: 0,
                        }),
                    },
                    other => panic!("unknown acceptance status {other}"),
                },
            })
            .collect(),
    };
    let step = text(&input["generationStepU64"]).parse().unwrap();
    let mut results = Vec::new();
    for op in array(&input["operations"]) {
        let id = text(&op["id"]).to_owned();
        let change = match text(&op["kind"]) {
            "switch" => acceptance::Change::Switch(id),
            "remove" => acceptance::Change::Remove(id),
            "hard-revoke" => acceptance::Change::HardRevoke(id),
            other => panic!("unknown acceptance operation {other}"),
        };
        let now = Time {
            seconds: text(&op["nowU64"]).parse().unwrap(),
            nanos: 0,
        };
        let (next, outcome) = state.apply(change, now, step).unwrap();
        state = next;
        results.push(match outcome {
            acceptance::Outcome::Wait => "wait",
            acceptance::Outcome::Switched => "switched",
            acceptance::Outcome::Removed => "removed",
            acceptance::Outcome::Revoked => "revoked",
        });
    }
    let keys = state
        .keys
        .iter()
        .map(|k| match k.status {
            TicketStatus::Current => json!({"id":k.key,"status":"current"}),
            TicketStatus::Retiring {
                switch_time: Some(t),
            } => json!({"id":k.key,"status":"retiring","switchU64":t.seconds.to_string()}),
            _ => panic!("missing switch time"),
        })
        .collect::<Vec<_>>();
    json!({"results":results,"generationU64":state.generation.to_string(),"keys":keys})
}
#[test]
fn ticket_payload_and_state_corpus() {
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
                    "ticket" => false,
                    "manifest" | "descriptor" | "session" | "pairing" => true,
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
                    "ticket-cache-model" => cache_model(&corpus, input),
                    "acceptance-set-model" => acceptance_model(input),
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
