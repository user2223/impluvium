// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: 0BSD

use impluvium_protocol::{
    decode::Reason,
    events::{
        self, Client, Credential, Lifecycle, Position, Receiver, StreamChain, WriteDecision,
        library::{BudgetExceeded, Budgets, Decision, Entry, Library},
        open::{self, BearerState, Generation, Request, Table},
    },
    proto::impluvium::v2::{
        ErrorReason, GameSummary, LibraryChanged, ListLibraryResponse, ResyncReason,
        SessionEndReason,
    },
};
use serde_json::{Value, json};

const EPOCH: [u8; 16] = [1; 16];
const GAME: &str = "11111111-1111-1111-1111-111111111111";
fn budgets() -> Budgets {
    Budgets {
        pages: 2,
        buffered_events: 2,
        reorder_events: 2,
        entries: 2,
    }
}
fn position(seq: u64) -> Position {
    Position { epoch: EPOCH, seq }
}
fn summary(name: &str, version: u64) -> GameSummary {
    GameSummary {
        game_id: GAME.into(),
        name: name.into(),
        entry_version: version,
        ..Default::default()
    }
}
fn baseline() -> Library {
    Library::from_baseline(position(5), &[Entry::game(summary("Old", 5))], budgets()).unwrap()
}
fn page(epoch: [u8; 16], seq: u64) -> ListLibraryResponse {
    ListLibraryResponse {
        epoch: epoch.to_vec(),
        snapshot_seq: seq,
        games: vec![summary("New", seq)],
        ..Default::default()
    }
}
fn wire(name: &str, seq: u64, prev: u64, payload: Value, id: bool) -> Vec<u8> {
    let mut data = json!({"epoch":"AQEBAQEBAQEBAQEBAQEBAQ==","seq":seq.to_string(),"prevSeq":prev.to_string(),"time":"2026-01-01T00:00:00Z"});
    for (k, v) in payload.as_object().unwrap() {
        data[k] = v.clone();
    }
    let prefix = if id {
        format!("id: {}\n", position(seq).id())
    } else {
        String::new()
    };
    format!("{prefix}event: {name}\ndata: {data}\n\n").into_bytes()
}
fn connected(client: Client) -> Client {
    client.receive(&wire(
        "connected",
        0,
        0,
        json!({"connected":{"headSeq":"100"}}),
        false,
    ))
}

#[test]
fn resume_uses_applied_state_while_building_and_after_a_broken_stream() {
    let client = connected(Client::new(baseline()));
    let mut client = client;
    client.library = client.library.begin_rebuild().0;
    client.library = client.library.page(&page(EPOCH, 8)).0;
    let client = client.receive(&wire("future", 9, 8, json!({"future":{}}), true));
    assert_eq!(client.receiver.last_data, 9);
    assert_eq!(client.library.resume_point(), Some(position(5)));
    let broken = client.receive(&wire("heartbeat", 8, 0, json!({"heartbeat":{}}), true));
    assert!(
        matches!(broken.lifecycle, Lifecycle::Broken(d) if d.reason == Reason::event_sequence_invalid)
    );
    assert_eq!(broken.library.resume_point(), Some(position(5)));
    let reconnect = broken.reconnect().unwrap();
    assert_eq!(reconnect.receiver, Receiver::default());
    assert_eq!(reconnect.library.resume_point(), Some(position(5)));
}

#[test]
fn resync_open_closed_unknown_and_access_changed_have_distinct_actions() {
    for (reason, lifecycle) in [
        (0, Lifecycle::Open),
        (1, Lifecycle::Open),
        (2, Lifecycle::Open),
        (3, Lifecycle::Open),
        (4, Lifecycle::Open),
        (5, Lifecycle::Reconnect),
        (6, Lifecycle::AccessChanged),
        (1234, Lifecycle::Open),
    ] {
        let client = connected(Client::new(baseline()));
        let next = client.receive(&wire(
            "resync_required",
            47,
            99,
            json!({"resyncRequired":{"reason":reason}}),
            false,
        ));
        assert_eq!(next.lifecycle, lifecycle);
        assert!(next.library.rebuilding());
        if reason == 6 {
            assert!(next.library.entries().is_empty());
            assert!(next.library.resume_point().is_none());
            assert!(next.reconnect().is_none());
        } else {
            assert_eq!(next.library.entries()[0].game.as_ref().unwrap().name, "Old");
            assert_eq!(next.library.resume_point(), Some(position(5)));
        }
        assert_eq!(next.receiver.last_data, 0);
    }
}

#[test]
fn terminal_events_restrict_reconnection_and_never_discard_the_library() {
    for (name, payload, expected) in [
        (
            "session_ended",
            json!({"sessionEnded":{"reason":999}}),
            Lifecycle::SessionEnded,
        ),
        (
            "superseded",
            json!({"superseded":{}}),
            Lifecycle::Superseded,
        ),
    ] {
        let client =
            connected(Client::new(baseline())).receive(&wire(name, 14, 80, payload, false));
        assert_eq!(client.lifecycle, expected);
        assert!(client.reconnect().is_none());
        assert_eq!(client.library.resume_point(), Some(position(5)));
        let ignored = client.receive(&wire("future", 100, 5, json!({"future":{}}), true));
        assert_eq!(ignored.receiver, client.receiver);
    }
}

#[test]
fn epoch_restart_discards_candidate_and_foreign_pages_cannot_replace_live_data() {
    let building = baseline().begin_rebuild().0.page(&page(EPOCH, 8)).0;
    let (foreign, d) = building.page(&page([2; 16], 20));
    assert_eq!(d, Decision::PageEpochRebuild);
    assert!(foreign.candidate_entries().is_empty());
    assert_eq!(foreign.entries()[0].game.as_ref().unwrap().name, "Old");
    let (restarted, _) = building.connected([2; 16]);
    assert_eq!(restarted.applied(), 0);
    assert!(restarted.resume_point().is_none());
    assert_eq!(restarted.model_epoch(), Some(EPOCH));
    assert!(restarted.candidate_entries().is_empty());
    let after_delayed_page = restarted.page(&page(EPOCH, 9)).0;
    assert_eq!(after_delayed_page.entries(), restarted.entries());
    let replacement = after_delayed_page
        .begin_rebuild()
        .0
        .page(&page([2; 16], 0))
        .0
        .complete_rebuild()
        .0;
    assert_eq!(replacement.model_epoch(), Some([2; 16]));
    assert_eq!(replacement.entries()[0].version, 0);
}

#[test]
fn tombstone_survives_delayed_pages_and_recreation_requires_a_newer_version() {
    let deletion = LibraryChanged {
        game_id: GAME.into(),
        deleted: true,
        ..Default::default()
    };
    let deleted = baseline().data(6, 5, Some(deletion)).0;
    let delayed = deleted.page(&page(EPOCH, 5)).0;
    assert!(delayed.entries()[0].game.is_none());
    let mut recreated = LibraryChanged {
        game_id: GAME.into(),
        ..Default::default()
    };
    recreated.game.modify(|g| *g = summary("Recreated", 7));
    let recreated = delayed.data(7, 6, Some(recreated)).0;
    assert_eq!(
        recreated.entries()[0].game.as_ref().unwrap().name,
        "Recreated"
    );
    assert_eq!(recreated.entries()[0].version, 7);
}

#[test]
fn rebuild_merges_buffered_deletion_after_pages_and_checks_early_heartbeats() {
    let building = baseline().begin_rebuild().0;
    assert_eq!(building.heartbeat(5).1, Decision::HeartbeatCurrent);
    let building = building.heartbeat(9).0;
    let deletion = LibraryChanged {
        game_id: GAME.into(),
        deleted: true,
        ..Default::default()
    };
    let building = building.data(9, 8, Some(deletion)).0;
    let building = building.page(&page(EPOCH, 8)).0.page(&page(EPOCH, 7)).0;
    assert_eq!(building.applied(), 5);
    assert_eq!(building.candidate_applied(), 8);
    let (finished, d) = building.complete_rebuild();
    assert_eq!(d, Decision::RebuildReplaced);
    assert_eq!(finished.applied(), 9);
    assert_eq!(finished.entries()[0].version, 9);
    assert!(finished.entries()[0].game.is_none());
}

#[test]
fn incomplete_pagination_cannot_replace_the_baseline() {
    let mut first = page(EPOCH, 8);
    first.next_page_token = "more".into();
    let state = baseline();
    let (failed, decision) = state.begin_rebuild().0.page(&first).0.complete_rebuild();
    assert_eq!(decision, Decision::RebuildGap);
    assert_eq!(failed.entries(), state.entries());
    assert!(failed.stale());
}

#[test]
fn client_budgets_fail_before_growth_and_keep_the_old_baseline() {
    let state = baseline();
    let held = state.data(8, 7, None).0.data(10, 9, None).0;
    assert_eq!(held.buffered(), 2);
    let (full, d) = held.data(12, 11, None);
    assert_eq!(d, Decision::Budget(BudgetExceeded::ReorderEvents));
    assert_eq!(full.buffered(), 2);
    assert_eq!(full.entries(), state.entries());
    assert_eq!(full.applied(), 5);
    let building = state
        .begin_rebuild()
        .0
        .data(8, 7, None)
        .0
        .data(10, 9, None)
        .0;
    let (full, d) = building.data(12, 11, None);
    assert_eq!(d, Decision::Budget(BudgetExceeded::BufferedEvents));
    assert_eq!(full.buffered(), 2);
    let building = state
        .begin_rebuild()
        .0
        .page(&page(EPOCH, 8))
        .0
        .page(&page(EPOCH, 9))
        .0;
    let (full, d) = building.page(&page(EPOCH, 10));
    assert_eq!(d, Decision::Budget(BudgetExceeded::Pages));
    assert!(full.stale());
    assert_eq!(full.entries(), state.entries());
    let mut too_many = page(EPOCH, 8);
    too_many.games.extend((2..=3).map(|i| GameSummary {
        game_id: format!("{i:08}-1111-1111-1111-111111111111"),
        ..summary("Extra", 8)
    }));
    let (full, d) = state.begin_rebuild().0.page(&too_many);
    assert_eq!(d, Decision::Budget(BudgetExceeded::Entries));
    assert_eq!(full.entries(), state.entries());
}

#[test]
fn filtered_streams_and_backlog_use_last_written_without_wrapping() {
    let stream = StreamChain {
        last_written: 5,
        closed: false,
    };
    let (stream, d) = stream.data(8, Credential::Active, true, false, false, true);
    assert_eq!(d, WriteDecision::Skip);
    let (stream, d) = stream.data(10, Credential::Active, true, true, false, false);
    assert_eq!(d, WriteDecision::Skip);
    assert_eq!(
        stream.heartbeat(Credential::Active).1,
        WriteDecision::Heartbeat {
            seq: 5,
            prev_seq: 0
        }
    );
    let (stream, d) = stream.data(12, Credential::Active, true, true, true, false);
    assert_eq!(
        d,
        WriteDecision::Data {
            seq: 12,
            prev_seq: 5
        }
    );
    let (closed, d) = stream.overflow(Credential::Active);
    assert_eq!(d, WriteDecision::ResyncRequired(ResyncReason::Overflow));
    assert!(closed.closed);
    assert_eq!(
        closed
            .data(13, Credential::Active, true, true, false, true)
            .1,
        WriteDecision::Closed
    );
    assert_eq!(events::next_sequence(u64::MAX - 1), Some(u64::MAX));
    assert_eq!(events::next_sequence(u64::MAX), None);
    assert_eq!(
        Position::parse(&position(u64::MAX).id()),
        Some(position(u64::MAX))
    );
    let receiver = Receiver {
        epoch: Some(EPOCH),
        last_data: u64::MAX - 1,
    };
    let (receiver, _) = receiver
        .receive(&wire(
            "future",
            u64::MAX,
            u64::MAX - 1,
            json!({"future":{}}),
            true,
        ))
        .unwrap();
    assert_eq!(receiver.last_data, u64::MAX);
}

#[test]
fn credential_recheck_precedes_topic_checks_and_entry_filtering() {
    let stream = StreamChain {
        last_written: 5,
        closed: false,
    };
    let (closed, d) = stream.data(6, Credential::Revoked, false, false, false, false);
    assert_eq!(d, WriteDecision::SessionEnded(SessionEndReason::Revoked));
    assert!(closed.closed);
    assert_eq!(
        stream.credential(Credential::Expired).1,
        Some(WriteDecision::SessionEnded(SessionEndReason::Expired))
    );
    assert_eq!(
        stream.heartbeat(Credential::Revoked).1,
        WriteDecision::SessionEnded(SessionEndReason::Revoked)
    );
    assert_eq!(
        stream.overflow(Credential::Expired).1,
        WriteDecision::SessionEnded(SessionEndReason::Expired)
    );
    assert_eq!(
        stream.superseded(Credential::Active).1,
        WriteDecision::Superseded
    );
    assert_eq!(
        stream
            .data(6, Credential::Active, false, false, false, false)
            .1,
        WriteDecision::ResyncRequired(ResyncReason::AccessChanged)
    );
}

#[test]
fn receive_order_preserves_profile_field_and_joint_diagnostics() {
    let receiver = Receiver {
        epoch: Some(EPOCH),
        last_data: 5,
    };
    let bad = wire(
        "heartbeat",
        0,
        0,
        json!({"libraryChanged":{"gameId":"bad"}}),
        false,
    );
    let d = receiver.receive(&bad).unwrap_err();
    assert_eq!(d.reason, Reason::missing_field);
    assert_eq!(d.field, "ServerEvent.heartbeat");
    let bad = wire(
        "library_changed",
        4,
        8,
        json!({"libraryChanged":{"gameId":GAME,"deleted":true,"game":{"gameId":"bad","name":"Bad"}}}),
        false,
    );
    let d = receiver.receive(&bad).unwrap_err();
    assert_eq!(d.reason, Reason::invalid_uuid);
    assert_eq!(d.field, "GameSummary.game_id");
    let bad = wire(
        "future",
        9,
        8,
        json!({"heartbeat":{},"libraryChanged":{}}),
        true,
    );
    assert_eq!(
        receiver.receive(&bad).unwrap_err().reason,
        Reason::oneof_conflict
    );
    let valid = wire("heartbeat", 5, 123, json!({"heartbeat":{}}), true);
    assert!(receiver.receive(&valid).is_ok());
    let crlf = String::from_utf8(valid).unwrap().replace('\n', "\r\n");
    assert!(receiver.receive(crlf.as_bytes()).is_ok());
}

#[test]
fn query_presence_header_multiplicity_and_generation_release_are_ordered() {
    let headers = [
        ("Authorization", "Bearer public"),
        ("Last-Event-ID", "ignored"),
    ];
    let mut request = Request {
        server_starting: false,
        method: "GET",
        query: "topics=library&client=one&last_event_id=",
        headers: &headers,
        session_cookie_name: "session",
        server_origin: "https://reference.invalid",
        user: "user",
        bearer_owner: "device",
        cookie_owner: "session",
        bearer: BearerState {
            known: true,
            expired: false,
            device_revoked: false,
            device_expired: false,
        },
        cookie_valid: true,
        topic_authorized: true,
    };
    assert_eq!(
        open::check(&request).unwrap().resume_point.as_deref(),
        Some("")
    );
    let duplicate = [
        ("Authorization", "Bearer public"),
        ("Last-Event-ID", "one"),
        ("last-event-id", "two"),
    ];
    request.headers = &duplicate;
    assert_eq!(
        open::check(&request)
            .unwrap_err()
            .diagnostic
            .unwrap()
            .reason,
        Reason::invalid_parameter
    );
    request.headers = &headers;
    let (table, opened) = Table::default().open(&request, Generation(1));
    let key = opened.unwrap().accepted.key.unwrap();
    let (table, opened) = table.open(&request, Generation(2));
    assert_eq!(opened.unwrap().superseded, Some(Generation(1)));
    let (table, removed) = table.release(&key, Generation(1));
    assert!(!removed);
    let (table, removed) = table.release(&key, Generation(2));
    assert!(removed);
    let (table, _) = table.open(&request, Generation(3));
    assert!(!table.release(&key, Generation(2)).1);
    request.bearer.expired = true;
    request.bearer.device_revoked = true;
    let (unchanged, rejected) = table.open(&request, Generation(4));
    assert_eq!(
        rejected.unwrap_err().wire.reason,
        Some(ErrorReason::TokenExpired)
    );
    assert_eq!(
        unchanged.entries().collect::<Vec<_>>(),
        table.entries().collect::<Vec<_>>()
    );
    request.bearer.expired = false;
    request.bearer.device_revoked = false;
    request.topic_authorized = false;
    let rejected = open::check(&request).unwrap_err();
    let discarded = Client::new(baseline()).opening_refused(&rejected);
    assert_eq!(discarded.lifecycle, Lifecycle::AccessChanged);
    assert!(discarded.library.entries().is_empty());
}
