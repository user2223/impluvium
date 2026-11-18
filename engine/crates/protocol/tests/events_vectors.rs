// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: 0BSD

//! Corpus adapters for the event and decoded-state boundaries only. Parsing a
//! supplied HTTP head here does not exercise a transport or an RPC exchange.

use impluvium_protocol::{
    events::{
        self, Epoch, Position, Receiver,
        library::{Budgets, Decision, Entry, Library},
        open::{self, BearerState, Form, Generation, Key, Request, Table},
    },
    proto::impluvium::v2::{GameSummary, LibraryChanged, ListLibraryResponse},
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
    ("behavior-000.json", 18),
    ("behavior-001.json", 19),
    ("behavior-witnesses.json", 5),
    ("behavior-priority.json", 23),
    ("behavior-conformance-000.json", 6),
    ("behavior-response-regressions.json", 3),
];
const SKIPPED: &[&str] = &[
    "generic-api",
    "generic-api-codec",
    "version-decision",
    "range",
    "http-response",
    "transport",
    "redirect",
    "session-model",
    "pairing-model",
    "base-path",
    "collision",
    "windows-tree",
    "discovery",
    "download-response",
    "reconstruct",
    "trust-model",
    "size-precheck",
    "publication",
    "ticket-cache-model",
    "rpc-exchange",
    "server-get",
    "auth-rpc-flow",
    "page-size",
    "normalize-user-code",
    "user-code-input",
    "page-size-codec",
    "acceptance-set-model",
    "blake3-http-response",
    "domain-separation",
    "command-budget",
];
fn u64_at(v: &Value, field: &str) -> u64 {
    v[field].as_str().unwrap_or("0").parse().unwrap()
}
fn epoch(v: &Value) -> Epoch {
    hex::decode(text(v)).unwrap().try_into().unwrap()
}
fn flag(v: &Value, field: &str, default: bool) -> bool {
    v[field].as_bool().unwrap_or(default)
}

fn with_request<T>(c: &Corpus, input: &Value, run: impl FnOnce(&Request<'_>) -> T) -> T {
    let raw = String::from_utf8(c.bytes(&input["requestHex"])).unwrap();
    let mut lines = raw.split("\r\n");
    let mut first = lines.next().unwrap().split(' ');
    let method = first.next().unwrap();
    let target = first.next().unwrap();
    let headers: Vec<_> = lines
        .take_while(|line| !line.is_empty())
        .map(|line| {
            let (k, v) = line.split_once(':').unwrap();
            (k, v.trim())
        })
        .collect();
    let valid = flag(input, "credentialValid", true);
    run(&Request {
        server_starting: flag(input, "serverStarting", false),
        method,
        query: target.split_once('?').map_or("", |(_, q)| q),
        headers: &headers,
        session_cookie_name: input["sessionCookieName"].as_str().unwrap_or("session"),
        server_origin: text(&input["serverOrigin"]),
        user: "user",
        bearer_owner: input["bearerOwner"].as_str().unwrap_or("device"),
        cookie_owner: input["cookieOwner"].as_str().unwrap_or("session"),
        bearer: BearerState {
            known: flag(input, "bearerValid", valid),
            expired: false,
            device_revoked: false,
            device_expired: false,
        },
        cookie_valid: flag(input, "cookieValid", valid),
        topic_authorized: flag(input, "topicAuthorized", true),
    })
}
fn key(value: &str) -> Key {
    let mut parts = value.splitn(3, ':');
    let form = match parts.next().unwrap() {
        "bearer" => Form::Bearer,
        "cookie" => Form::Cookie,
        x => panic!("unknown auth {x}"),
    };
    Key {
        user: "user".into(),
        form,
        owner: parts.next().unwrap().into(),
        client: parts.next().unwrap().into(),
    }
}
fn key_text(key: &Key) -> String {
    format!(
        "{}:{}:{}",
        if key.form == Form::Bearer {
            "bearer"
        } else {
            "cookie"
        },
        key.owner,
        key.client
    )
}
fn register(table: Table, key: &Key, generation: Generation) -> Table {
    let query = format!("topics=library&client={}", key.client);
    let headers = match key.form {
        Form::Bearer => vec![("Authorization", "Bearer public")],
        Form::Cookie => vec![
            ("Cookie", "session=public"),
            ("Origin", "https://reference.invalid"),
        ],
    };
    let request = Request {
        server_starting: false,
        method: "GET",
        query: &query,
        headers: &headers,
        session_cookie_name: "session",
        server_origin: "https://reference.invalid",
        user: &key.user,
        bearer_owner: &key.owner,
        cookie_owner: &key.owner,
        bearer: BearerState {
            known: true,
            expired: false,
            device_revoked: false,
            device_expired: false,
        },
        cookie_valid: true,
        topic_authorized: true,
    };
    let (next, result) = table.open(&request, generation);
    result.unwrap();
    next
}
fn initial_table(count: usize, owner: Option<&Key>) -> Table {
    let mut table = Table::default();
    if let Some(key) = owner {
        table = register(table, key, Generation(u128::MAX));
    }
    for i in usize::from(owner.is_some())..count {
        table = register(
            table,
            &Key {
                user: "user".into(),
                form: Form::Bearer,
                owner: "padding-device".into(),
                client: format!("padding_{i}"),
            },
            Generation(u128::MAX - i as u128 - 1),
        );
    }
    table
}
fn opening(c: &Corpus, input: &Value) -> Value {
    with_request(c, input, |request| {
        let count = input["clientIdentifierCount"].as_u64().unwrap_or(0) as usize;
        let existing = if flag(input, "newClientIdentifier", true) {
            None
        } else {
            open::check(request).unwrap().key
        };
        let (_, result) = initial_table(count, existing.as_ref()).open(request, Generation(1));
        match result {
            Ok(_) => {
                json!({"status":200,"contentType":open::Accepted::CONTENT_TYPE,"cacheControl":open::Accepted::CACHE_CONTROL,"accelBuffering":open::Accepted::ACCEL_BUFFERING})
            }
            Err(e) => {
                let mut value = json!({"status":e.status});
                if let Some(d) = e.diagnostic {
                    value["reason"] = json!(format!("{:?}", d.reason));
                }
                if matches!(e.status, 503 | 405) {
                    value["wireReason"] = json!(e.wire.reason.map_or(0, |r| r as i32));
                }
                value
            }
        }
    })
}
fn opening_model(c: &Corpus, input: &Value) -> Value {
    let mut owner = input["initialOwner"].as_str().map(key);
    let mut table = initial_table(
        input["initialIdentifierEntries"].as_u64().unwrap_or(0) as usize,
        owner.as_ref(),
    );
    let mut ids = BTreeMap::new();
    let mut results = Vec::new();
    for (index, attempt) in array(&input["attempts"]).iter().enumerate() {
        if attempt["kind"] == "release" {
            let generation = ids
                .iter()
                .find(|(_, id)| **id == attempt["streamId"])
                .map_or(Generation(0), |(g, _)| *g);
            let selected = key(text(&attempt["key"]));
            let (next, released) = table.release(&selected, generation);
            table = next;
            if released && owner.as_ref() == Some(&selected) {
                owner = None;
            }
            results.push(json!({"released":released}));
            continue;
        }
        assert!(
            attempt["kind"].is_null(),
            "unknown event-open-model operation"
        );
        let generation = Generation(index as u128 + 1);
        ids.insert(
            generation,
            attempt.get("streamId").cloned().unwrap_or(json!(index)),
        );
        let (next, result) = with_request(c, attempt, |request| table.open(request, generation));
        table = next;
        let (status, supersedes, created) = match result {
            Ok(opened) => {
                if let Some(key) = opened.accepted.key {
                    owner = Some(key);
                }
                (200, opened.superseded.is_some(), opened.created_identifier)
            }
            Err(e) => (e.status, false, false),
        };
        results.push(json!({"status":status,"supersedes":supersedes,"createdIdentifier":created}));
    }
    let mut out = json!({"results":results,"owner":owner.as_ref().map(key_text),"identifierEntries":table.count("user")});
    if flag(input, "observeGenerations", false) {
        out["generations"] = json!(
            table
                .entries()
                .map(|(k, g)| json!({"key":key_text(k),"streamId":ids[&g]}))
                .collect::<Vec<_>>()
        );
    }
    out
}
fn receive(c: &Corpus, input: &Value) -> Value {
    let context = &input["context"];
    let receiver = Receiver {
        epoch: if flag(context, "first", false) {
            None
        } else {
            Some(epoch(&context["epochHex"]))
        },
        last_data: u64_at(context, "lastDataU64"),
    };
    match receiver.receive(&c.bytes(&input["eventHex"])) {
        Err(d) => json!({"reason":format!("{:?}", d.reason),"field":d.field}),
        Ok((state, received)) => {
            json!({"accepted":true,"dataEvent":received.data_event,"epochHex":hex::encode(state.epoch.unwrap()),"lastDataU64":state.last_data.to_string()})
        }
    }
}
fn resume(input: &Value) -> Value {
    let decision = events::resume(
        input["point"].as_str(),
        Position {
            epoch: epoch(&input["epochHex"]),
            seq: u64_at(input, "headU64"),
        },
        input["missingAfterU64"]
            .as_str()
            .map(|s| s.parse().unwrap()),
    );
    let label = match decision.reason {
        None if decision.resumed => "resumed",
        None => "live",
        Some(r) => match r as i32 {
            1 => "TOO_OLD",
            2 => "AHEAD",
            3 => "FOREIGN_EPOCH",
            4 => "MALFORMED",
            x => panic!("unexpected resume reason {x}"),
        },
    };
    json!({"decision":label})
}
fn supersession(input: &Value) -> Value {
    let convert = |v: &Value| {
        v["client"].as_str().map(|client| Key {
            user: "user".into(),
            form: match text(&v["auth"]) {
                "bearer" => Form::Bearer,
                "cookie" => Form::Cookie,
                x => panic!("unknown auth {x}"),
            },
            owner: text(&v["owner"]).into(),
            client: client.into(),
        })
    };
    let left = convert(&input["left"]);
    json!({"supersedes":left.is_some() && left == convert(&input["right"])})
}
fn game(entry: &Value) -> GameSummary {
    GameSummary {
        game_id: text(&entry["gameId"]).into(),
        name: entry["name"].as_str().unwrap_or("").into(),
        entry_version: u64_at(entry, "versionU64"),
        ..Default::default()
    }
}
fn entry(entry: &Value) -> Entry {
    Entry {
        game_id: text(&entry["gameId"]).into(),
        version: u64_at(entry, "versionU64"),
        game: if flag(entry, "deleted", false) {
            None
        } else {
            Some(game(entry))
        },
    }
}
fn entries(values: &[Entry]) -> Value {
    json!(values.iter().map(|e| {
        let mut value = json!({"gameId":e.game_id,"versionU64":e.version.to_string(),"deleted":e.game.is_none()});
        if let Some(game) = &e.game { value["name"] = json!(game.name); }
        value
    }).collect::<Vec<_>>())
}
fn decision(d: Decision) -> &'static str {
    match d {
        Decision::Applied => "applied",
        Decision::Duplicate => "duplicate",
        Decision::HeldGap => "held-gap",
        Decision::BufferedBuilding => "buffered-building",
        Decision::PageMerged => "page-merged",
        Decision::HeartbeatCurrent => "heartbeat-current",
        Decision::HeartbeatRebuild => "heartbeat-rebuild",
        Decision::NewEpochRebuild => "new-epoch-rebuild",
        Decision::SameEpoch => "same-epoch",
        Decision::RebuildStarted => "rebuild-started",
        Decision::RebuildReplaced => "rebuild-replaced",
        Decision::RebuildFailed => "rebuild-failed",
        Decision::RebuildGap => "rebuild-gap",
        Decision::PageEpochRebuild => "page-epoch-rebuild",
        Decision::Budget(e) => panic!("unexpected model budget {e:?}"),
    }
}
fn library(input: &Value) -> Value {
    let initial = array(&input["entries"])
        .iter()
        .map(entry)
        .collect::<Vec<_>>();
    let mut state = Library::from_baseline(
        Position {
            epoch: epoch(&input["epoch"]),
            seq: u64_at(input, "appliedU64"),
        },
        &initial,
        Budgets {
            pages: 32,
            buffered_events: 64,
            reorder_events: 64,
            entries: 6400,
        },
    )
    .unwrap();
    let mut decisions = Vec::new();
    for op in array(&input["operations"]) {
        let (next, d) = match text(&op["kind"]) {
            "begin-rebuild" => state.begin_rebuild(),
            "complete-rebuild" => state.complete_rebuild(),
            "fail-rebuild" => state.fail_rebuild(),
            "connected" => state.connected(epoch(&op["epoch"])),
            "heartbeat" => state.heartbeat(u64_at(op, "seqU64")),
            "data" => {
                let change = op.get("entry").map(|entry| {
                    let mut change = LibraryChanged {
                        game_id: text(&entry["gameId"]).into(),
                        deleted: flag(entry, "deleted", false),
                        ..Default::default()
                    };
                    if !change.deleted {
                        change.game.modify(|g| {
                            *g = game(entry);
                            g.entry_version = u64_at(op, "seqU64");
                        });
                    }
                    change
                });
                state.data(u64_at(op, "seqU64"), u64_at(op, "prevU64"), change)
            }
            "page" => state.page(&ListLibraryResponse {
                epoch: epoch(&op["epoch"]).to_vec(),
                snapshot_seq: u64_at(op, "snapshotU64"),
                games: array(&op["entries"]).iter().map(game).collect(),
                ..Default::default()
            }),
            other => panic!("unknown library-model operation {other}"),
        };
        state = next;
        decisions.push(decision(d));
    }
    let mut out = json!({"epoch":state.epoch().map(hex::encode),"appliedU64":state.applied().to_string(),"rebuild":state.rebuilding(),"buffered":state.buffered(),"decisions":decisions,"entries":entries(state.entries())});
    let observe = &input["observeBuild"];
    for (name, value) in [
        ("stale", json!(state.stale())),
        ("modelEpoch", json!(state.model_epoch().map(hex::encode))),
        (
            "candidateEpoch",
            json!(state.candidate_epoch().map(hex::encode)),
        ),
        ("candidateEntries", entries(state.candidate_entries())),
        (
            "candidateAppliedU64",
            json!(state.candidate_applied().to_string()),
        ),
    ] {
        if observe == &json!(true)
            || observe
                .as_array()
                .is_some_and(|names| names.iter().any(|v| v.as_str() == Some(name)))
        {
            out[name] = value;
        }
    }
    out
}

#[test]
fn events_contract_corpus() {
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
                    "event-open" => opening(&corpus, input),
                    "event-open-model" => opening_model(&corpus, input),
                    "event-receive" => receive(&corpus, input),
                    "resume" => resume(input),
                    "supersession" => supersession(input),
                    "library-model" => library(input),
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
