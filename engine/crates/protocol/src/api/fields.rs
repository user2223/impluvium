// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: 0BSD

//! Stage 1 of encoding.md §7.2. Maximum lengths/counts and timestamps have
//! already passed the scanner. No context check runs during this traversal.

use crate::{
    decode::{Diagnostic, Projection, Reason, Value, error},
    limits::{Kind, MessageKind, Occurrence},
    manifest,
};

pub(super) fn number(p: &Projection, field: u32) -> u64 {
    match p.fields.iter().find(|(n, _)| *n == field).map(|(_, v)| v) {
        Some(Value::Uint32(n)) => u64::from(*n),
        Some(Value::Uint64(n)) => *n,
        _ => 0,
    }
}
pub(super) fn string(p: &Projection, field: u32) -> &str {
    match p.fields.iter().find(|(n, _)| *n == field).map(|(_, v)| v) {
        Some(Value::String(s)) => s,
        _ => "",
    }
}
pub(super) fn identity(
    actual: &str,
    expected: &str,
    field: &'static str,
) -> Result<(), Diagnostic> {
    if actual == expected {
        Ok(())
    } else {
        Err(error(Reason::response_identity_mismatch, field))
    }
}
fn display(s: &str) -> bool {
    !s.chars()
        .any(|c| c <= '\u{1f}' || ('\u{7f}'..='\u{9f}').contains(&c))
}
pub(super) fn depot_url(s: &str, field: &'static str) -> Result<(), Diagnostic> {
    let valid = url::Url::parse(s).is_ok_and(|u| {
        matches!(u.scheme(), "http" | "https")
            && u.host_str().is_some()
            && u.username().is_empty()
            && u.password().is_none()
            && u.query().is_none()
            && u.fragment().is_none()
            && (!u.path().ends_with('/') || (u.path() == "/" && !explicit_path(s)))
    });
    if valid {
        Ok(())
    } else {
        Err(error(Reason::invalid_url, field))
    }
}

// WHATWG serializes an empty HTTP path as '/'. Distinguish that permitted
// empty base path from an explicit path that normalizes to a trailing slash.
// Actual URL parsing and path normalization remain the URL library's job.
fn explicit_path(s: &str) -> bool {
    let normalized: String = s
        .trim_matches(|c: char| c <= '\u{20}')
        .chars()
        .filter(|c| !matches!(c, '\t' | '\r' | '\n'))
        .map(|c| if c == '\\' { '/' } else { c })
        .collect();
    normalized
        .split_once(':')
        .is_some_and(|(_, rest)| rest.trim_start_matches('/').contains('/'))
}

fn required(field: &str) -> bool {
    matches!(
        field,
        "StartPairingResponse.pairing_id"
            | "StartPairingResponse.user_code"
            | "StartPairingResponse.verification_uri"
            | "StartPairingResponse.verification_uri_complete"
            | "StartPairingResponse.interval_seconds"
            | "StartPairingResponse.expires_at"
            | "BeginSessionResponse.challenge"
            | "BeginSessionResponse.expires_at"
            | "CompleteSessionResponse.access_token"
            | "CompleteSessionResponse.expires_at"
            | "CompleteSessionResponse.scope"
            | "PollPairingResponse.device_id"
            | "PollPairingResponse.instance_id"
            | "PollPairingResponse.content_public_key"
            | "Device.device_id"
            | "Device.name"
            | "Device.platform"
            | "Device.arch"
            | "Device.client_version"
            | "Device.created_at"
            | "RevokeDeviceRequest.device_id"
            | "ListLibraryResponse.epoch"
            | "GameSummary.game_id"
            | "GameSummary.name"
            | "GetGameRequest.game_id"
            | "GetGameResponse.game"
            | "ListVersionsRequest.game_id"
            | "VersionSummary.version_id"
            | "VersionSummary.game_id"
            | "VersionSummary.kind"
            | "VersionSummary.manifest_size"
            | "VersionSummary.manifest_id"
            | "VersionSummary.descriptor_revision"
            | "GetVersionRequest.version_id"
            | "GetVersionResponse.version"
            | "OpenDownloadRequest.version_id"
            | "RefreshTicketRequest.version_id"
            | "OpenDownloadResponse.descriptor"
            | "OpenDownloadResponse.manifest_ids"
            | "OpenDownloadResponse.ticket"
            | "OpenDownloadResponse.depot_endpoints"
            | "RefreshTicketResponse.ticket"
            | "RefreshTicketResponse.manifest_ids"
            | "DepotEndpoint.base_url"
            | "Discovery.server_name"
            | "Discovery.server_version"
            | "Discovery.instance_id"
            | "Discovery.protocol"
            | "Discovery.manifest_formats"
            | "Discovery.source_url"
            | "ProtocolVersion.major"
            | "ProtocolVersion.revision"
            | "ProtocolVersion.min_client_revision"
            | "ServerEvent.epoch"
            | "ServerEvent.time"
            | "LibraryChanged.game_id"
    )
}
fn default(v: &Value) -> bool {
    match v {
        Value::Uint32(n) => *n == 0,
        Value::Uint64(n) => *n == 0,
        Value::Enum(n) => *n == 0,
        Value::String(s) => s.is_empty(),
        Value::Bytes(b) => b.is_empty(),
        _ => false,
    }
}
fn length(v: &Value) -> usize {
    match v {
        Value::String(s) => s.len(),
        Value::Bytes(b) => b.len(),
        _ => 0,
    }
}
fn exact(field: &str) -> Option<(usize, Reason)> {
    use Reason::*;
    Some(match field {
        "StartPairingResponse.pairing_id" => (16, invalid_pairing_id_length),
        "StartPairingResponse.user_code" => (9, invalid_user_code),
        "BeginSessionResponse.challenge" => (32, invalid_challenge_length),
        "CompleteSessionResponse.access_token" => (43, invalid_base64),
        "PollPairingResponse.content_public_key" => (32, invalid_trusted_key),
        "ListLibraryResponse.epoch" | "ServerEvent.epoch" => (16, invalid_length),
        "VersionSummary.manifest_id"
        | "OpenDownloadResponse.manifest_ids"
        | "RefreshTicketResponse.manifest_ids" => (32, invalid_length),
        _ => return None,
    })
}
fn unique(field: &str) -> bool {
    matches!(
        field,
        "GameSummary.platforms"
            | "VersionSummary.platforms"
            | "CompleteSessionResponse.accepted_capabilities"
            | "Discovery.capabilities"
            | "Discovery.required_client_capabilities"
            | "Discovery.manifest_formats"
    )
}
fn syntax(v: &Value, field: &'static str) -> Result<(), Diagnostic> {
    use Reason::*;
    let Value::String(s) = v else { return Ok(()) };
    let reason = if field.ends_with(".game_id")
        || field.ends_with(".version_id")
        || field.ends_with(".device_id")
        || field.ends_with(".instance_id")
        || field.ends_with(".icon_object_id")
        || field.ends_with(".cover_object_id")
    {
        (!manifest::uuid(s)).then_some(invalid_uuid)
    } else if matches!(
        field,
        "Discovery.capabilities"
            | "Discovery.required_client_capabilities"
            | "CompleteSessionResponse.accepted_capabilities"
    ) {
        (!manifest::feature_name(s)).then_some(invalid_name)
    } else if field == "StartPairingResponse.user_code" {
        let b = s.as_bytes();
        (!(b.len() == 9
            && b[4] == b'-'
            && b.iter()
                .enumerate()
                .all(|(i, c)| i == 4 || crate::proof::user_code::ALPHABET.contains(c))))
        .then_some(invalid_user_code)
    } else if field == "CompleteSessionResponse.access_token" {
        let value = |b: u8| match b {
            b'A'..=b'Z' => Some(b - b'A'),
            b'a'..=b'z' => Some(b - b'a' + 26),
            b'0'..=b'9' => Some(b - b'0' + 52),
            b'-' => Some(62),
            b'_' => Some(63),
            _ => None,
        };
        (!(s.bytes().all(|b| value(b).is_some())
            && s.bytes().last().and_then(value).is_some_and(|n| n & 3 == 0)))
        .then_some(invalid_base64)
    } else if matches!(
        field,
        "StartPairingResponse.verification_uri"
            | "StartPairingResponse.verification_uri_complete"
            | "Discovery.source_url"
    ) {
        // These are informational links, not request preflight. The field
        // tables require absolute URLs; only source_url restricts the scheme.
        let valid = url::Url::parse(s).is_ok_and(|u| {
            field != "Discovery.source_url" || matches!(u.scheme(), "http" | "https")
        });
        return if valid {
            Ok(())
        } else {
            Err(error(invalid_url, field))
        };
    } else if matches!(
        field,
        "Device.name"
            | "Device.platform"
            | "Device.arch"
            | "Device.client_version"
            | "GameSummary.name"
            | "VersionSummary.display_name"
            | "Discovery.server_name"
            | "Discovery.server_version"
            | "Discovery.commit"
    ) {
        (!display(s)).then_some(invalid_text)
    } else {
        None
    };
    reason.map_or(Ok(()), |r| Err(error(r, field)))
}

pub(crate) fn validate(p: &Projection) -> Result<(), Diagnostic> {
    use Reason::*;
    if p.message == MessageKind::KeyFingerprint {
        return Ok(());
    }
    for rule in p.message.fields() {
        let field = rule.diagnostic;
        // Discovery identification is one joint check performed by its caller.
        if matches!(field, "Discovery.discovery" | "Discovery.product") {
            continue;
        }
        let mut values = Vec::new();
        let zero = Value::Enum(0);
        for (_, v) in p.fields.iter().filter(|(n, _)| *n == rule.number) {
            let unknown = if let Kind::Enum(list) = rule.kind {
                match v {
                    Value::Enum(n) => !list.iter().any(|(_, k)| n == k),
                    Value::UnknownEnumName(_) => true,
                    _ => false,
                }
            } else {
                false
            };
            if unknown && rule.occurrence == Occurrence::Repeated {
                continue;
            }
            values.push(if unknown && field != "VersionSummary.kind" {
                &zero
            } else {
                v
            });
        }
        let presence_reason = if field == "CompleteSessionResponse.scope" {
            unsupported_session_scope
        } else {
            missing_field
        };
        if required(field)
            && (values.is_empty()
                || (rule.occurrence != Occurrence::Repeated && default(values[0])))
        {
            return Err(error(presence_reason, field));
        }
        for (i, v) in values.iter().enumerate() {
            // Optional scalar defaults, including empty optional UUIDs, are absent.
            if rule.occurrence == Occurrence::Scalar && !required(field) && default(v) {
                continue;
            }
            if let Some((n, reason)) = exact(field)
                && length(v) != n
            {
                return Err(error(reason, field));
            }
            syntax(v, field)?;
            let n = match v {
                Value::Uint32(n) => u64::from(*n),
                Value::Uint64(n) => *n,
                Value::Enum(n) => *n as u64,
                _ => 0,
            };
            let in_range = match field {
                "StartPairingResponse.interval_seconds" => (1..=60).contains(&n),
                "CompleteSessionResponse.scope" => n == 1,
                "VersionSummary.manifest_size" => (1..=536_870_912).contains(&n),
                "ProtocolVersion.major"
                | "ProtocolVersion.revision"
                | "ProtocolVersion.min_client_revision" => n >= 1,
                "ErrorInfo.retry_after_seconds" => n <= 86_400,
                _ => true,
            };
            if !in_range {
                return Err(error(
                    if field == "CompleteSessionResponse.scope" {
                        unsupported_session_scope
                    } else {
                        value_out_of_range
                    },
                    field,
                ));
            }
            if unique(field) && values[..i].contains(v) {
                return Err(error(duplicate_element, field));
            }
            if matches!(
                field,
                "OpenDownloadResponse.manifest_ids" | "RefreshTicketResponse.manifest_ids"
            ) && i > 0
                && let (Value::Bytes(prev), Value::Bytes(next)) = (values[i - 1], *v)
                && prev >= next
            {
                return Err(error(unsorted_manifest_ids, field));
            }
            if let Value::MapEntry(k, value) = v
                && (k.is_empty()
                    || !k.bytes().all(|b| (0x21..=0x7e).contains(&b))
                    || !display(value))
            {
                return Err(error(invalid_text, field));
            }
            if let Value::Message(child) = v {
                validate(child)?;
            }
        }
    }
    Ok(())
}
