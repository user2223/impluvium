// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: 0BSD

use super::*;
use crate::proto::impluvium::{
    manifest::v3::{Entry, Manifest},
    signed::v1::Envelope,
    v2::{BeginSessionResponse, CompleteSessionRequest, Discovery, ErrorInfo},
};
use buffa::{ExtensionSet, Message};

fn varint(out: &mut Vec<u8>, mut n: u64) {
    while n >= 128 {
        out.push(n as u8 | 128);
        n >>= 7;
    }
    out.push(n as u8);
}
fn opaque(out: &mut Vec<u8>, number: u64, bytes: &[u8]) {
    varint(out, number << 3 | 2);
    varint(out, bytes.len() as u64);
    out.extend_from_slice(bytes);
}

#[test]
fn unknown_fields_are_unlimited_in_count_and_never_stored() {
    let mut entry = Vec::new();
    for _ in 0..10_000 {
        entry.extend_from_slice(&[0x80, 0x01, 0x00]); // field 16, VARINT
        entry.extend_from_slice(&[0x89, 0x01, 0, 0, 0, 0, 0, 0, 0, 0]);
        entry.extend_from_slice(&[0x95, 0x01, 0, 0, 0, 0]);
        // Opaque unknown messages do not contribute to nesting depth.
        opaque(&mut entry, 8, &[0xff; 80]);
    }
    entry.extend_from_slice(&[0x38, 0x01, 0x3a, 0x02, 0x02, 0x03, 0x38, 0x04]);
    let mut manifest = Vec::new();
    opaque(&mut manifest, 8, &entry);
    opaque(&mut manifest, 8, &[0x3a, 0, 0x38, 5]);
    let decoded: Manifest =
        decode_binary(&manifest, InputLimit::payload(limits::MANIFEST_BYTES)).unwrap();
    assert!(decoded.unknown_fields().is_empty());
    assert!(decoded.entries[0].unknown_fields().is_empty());
    assert_eq!(decoded.entries[0].chunk_indexes, [1, 2, 3, 4]);
    assert_eq!(decoded.entries[1].chunk_indexes, [5]);
    // Re-encoding the projection cannot re-emit any optional unknown Entry field.
    assert!(decoded.encode_to_vec().len() < 32);
}

#[test]
fn decoder_profile_allows_valid_empty_message_flood_above_runtime_default_budget() {
    let count = (32 * 1024 * 1024) / core::mem::size_of::<Entry>() + 1;
    assert!(count * core::mem::size_of::<Entry>() > 32 * 1024 * 1024);
    let wire = [0x42, 0x00].repeat(count);
    let decoded: Manifest =
        decode_binary(&wire, InputLimit::payload(limits::MANIFEST_BYTES)).unwrap();
    assert_eq!(decoded.entries.len(), count);
    assert_eq!(decoder_profile().unknown_field_limit(), 0);
}

#[test]
fn envelope_metadata_boundary_includes_unknowns_and_payload_framing() {
    // Three bytes of unknown framing plus two bytes of empty payload framing.
    for metadata in [16_384usize, 16_385] {
        let mut wire = vec![0x12, 0x00];
        let body = vec![0; metadata - 5];
        opaque(&mut wire, 4, &body);
        assert_eq!(wire.len(), metadata);
        let result =
            decode_binary::<Envelope>(&wire, InputLimit::envelope(crate::envelope::MANIFEST));
        if metadata == 16_384 {
            assert!(result.unwrap().unknown_fields().is_empty());
        } else {
            assert_eq!(result.unwrap_err(), error(Reason::byte_limit_exceeded, ""));
        }
    }
}

#[test]
fn raw_ambiguities_and_malformed_framing_are_rejected_before_projection() {
    let limit = InputLimit::payload(limits::MANIFEST_BYTES);
    for wire in [
        &[0x0a, 0x00, 0x0a, 0x00][..],
        &[0x38, 0x80][..],
        &[0x3a, 0x01, 0x80][..],
        &[
            0x0a, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x01,
        ][..],
    ] {
        assert!(decode_binary::<Entry>(wire, limit).is_err());
    }
    assert_eq!(
        decode_binary::<Manifest>(&[0x2a, 0x00, 0x2a, 0x00], limit).unwrap_err(),
        error(Reason::split_message, "Manifest.header")
    );
    let body = InputLimit::body(65_536);
    assert_eq!(
        decode_json::<CompleteSessionRequest>(br#"{"deviceId":"x","device_id":null}"#, body)
            .unwrap_err(),
        error(Reason::duplicate_field, "CompleteSessionRequest.device_id")
    );
    assert_eq!(
        decode_json::<Discovery>(br#"{"protocol":{},"protocol":{}}"#, body).unwrap_err(),
        error(Reason::duplicate_json_key, "")
    );
    assert_eq!(
        decode_json::<ErrorInfo>(br#"{"metadata":{"k":"v","\u006b":"w"}}"#, body).unwrap_err(),
        error(Reason::duplicate_json_key, "")
    );
}

#[test]
fn unknown_json_subtrees_are_checked_and_depth_is_exact() {
    let limit = InputLimit::body(65_536);
    for depth in [64usize, 65] {
        let raw = format!(
            "{{\"future\":{}0{}}}",
            "[".repeat(depth - 1),
            "]".repeat(depth - 1)
        );
        let result = scan_json(MessageKind::Discovery, raw.as_bytes(), limit);
        if depth == 64 {
            assert!(result.is_ok());
        } else {
            assert_eq!(result.unwrap_err(), error(Reason::nesting_too_deep, ""));
        }
    }
    let unknowns = (0..2_000)
        .map(|n| format!("\"future{n}\":{{}}"))
        .collect::<Vec<_>>()
        .join(",");
    let result: Discovery = decode_json(format!("{{{unknowns}}}").as_bytes(), limit).unwrap();
    assert!(result.unknown_fields().is_empty());
    assert_eq!(
        scan_json(
            MessageKind::Discovery,
            br#"{"future":{"k":0,"k":1}}"#,
            limit
        )
        .unwrap_err(),
        error(Reason::duplicate_json_key, "")
    );
}

#[test]
fn counters_and_content_fail_without_overflow_or_float_rounding() {
    assert!(InputLimit::body(65_536).check_add(u64::MAX, 1).is_err());
    assert!(InputLimit::body(65_536).check_add(1, u64::MAX).is_err());
    assert!(InputLimit::body(65_536).check_add(65_535, 1).is_ok());
    let p = project_json(
        MessageKind::Manifest,
        br#"{"totalSize":"1.8446744073709551615e19"}"#,
        InputLimit::body(65_536),
    )
    .unwrap();
    assert_eq!(p.fields, [(7, Value::Uint64(u64::MAX))]);
    let response: BeginSessionResponse = decode_json(
        br#"{"challenge":"-_8=","expiresAt":"1970-01-01t01:00:00+01:00"}"#,
        InputLimit::body(65_536),
    )
    .unwrap();
    assert_eq!(response.challenge, [0xfb, 0xff]);
    assert_eq!(response.expires_at.as_option().unwrap().seconds, 0);
}

#[test]
fn unknown_field_flood_reaches_the_body_boundary_without_a_count_cap() {
    let mut wire = [0x80, 0x01, 0x00].repeat(21_844);
    opaque(&mut wire, 16, &[0]);
    assert_eq!(wire.len(), 65_536);
    let limit = InputLimit::body(65_536);
    let decoded: Discovery = decode_binary(&wire, limit).unwrap();
    assert!(decoded.unknown_fields().is_empty());
    wire.push(0);
    assert_eq!(
        scan_binary(MessageKind::Discovery, &wire, limit).unwrap_err(),
        error(Reason::body_too_large, "")
    );
}

#[test]
fn packed_element_limit_precedes_the_next_elements_content() {
    // The first eight elements exhaust Discovery.manifest_formats. The ninth
    // element exceeds its count before its truncated varint content is examined.
    let wire = [0x3a, 9, 0, 0, 0, 0, 0, 0, 0, 0, 0x80];
    assert_eq!(
        scan_binary(MessageKind::Discovery, &wire, InputLimit::body(65_536)).unwrap_err(),
        error(Reason::element_limit_exceeded, "Discovery.manifest_formats")
    );
}

#[test]
fn error_details_complete_step_six_before_duplicate_info_and_binary_checks() {
    let cases = [
        (
            r#"{"code":"unknown","details":[{"type":"impluvium.v2.ErrorInfo","value":""},{"type":"impluvium.v2.ErrorInfo","value":""},{"type":"x","value":"%"}]}"#,
            Reason::invalid_base64,
        ),
        (
            r#"{"code":"unknown","details":[{"type":"impluvium.v2.ErrorInfo","value":""},{"type":"impluvium.v2.ErrorInfo","value":""},{"type":"x"}]}"#,
            Reason::missing_field,
        ),
        (
            r#"{"code":"unknown","details":[{"type":"impluvium.v2.ErrorInfo","value":"/w"},{"type":"impluvium.v2.ErrorInfo","value":""}]}"#,
            Reason::duplicate_field,
        ),
    ];
    for (body, reason) in cases {
        assert_eq!(
            decode_error_body(body.as_bytes()).unwrap_err(),
            error(reason, "")
        );
    }
    let details = std::iter::once(
        r#"{"type":"impluvium.v2.ErrorInfo","value":""},{"type":"impluvium.v2.ErrorInfo","value":""}"#,
    )
    .chain(std::iter::repeat_n(r#"{"type":"x","value":""}"#, 15))
    .collect::<Vec<_>>()
    .join(",");
    let body = format!(r#"{{"code":"unknown","details":[{details}]}}"#);
    assert_eq!(
        decode_error_body(body.as_bytes()).unwrap_err(),
        error(Reason::element_limit_exceeded, "")
    );
    let value = "A".repeat(10_924); // 8193 decoded bytes, beyond step 6's limit.
    let body = format!(
        r#"{{"code":"unknown","details":[{{"type":"impluvium.v2.ErrorInfo","value":""}},{{"type":"impluvium.v2.ErrorInfo","value":""}},{{"type":"x","value":"{value}"}}]}}"#
    );
    assert_eq!(
        decode_error_body(body.as_bytes()).unwrap_err(),
        error(Reason::byte_limit_exceeded, "")
    );
}
