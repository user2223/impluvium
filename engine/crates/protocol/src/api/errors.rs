// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: 0BSD

//! errors.md catalogs and context-sensitive mappings. Neither a diagnostic field
//! nor human-readable message text is ever used as a wire reason.

use crate::{decode::Reason, proto::impluvium::v2::ErrorReason};

#[allow(non_camel_case_types)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Code {
    canceled,
    unknown,
    invalid_argument,
    deadline_exceeded,
    not_found,
    already_exists,
    permission_denied,
    resource_exhausted,
    failed_precondition,
    aborted,
    out_of_range,
    unimplemented,
    internal,
    unavailable,
    data_loss,
    unauthenticated,
}
impl Code {
    pub const fn http_status(self) -> u16 {
        match self {
            Self::canceled => 499,
            Self::unknown | Self::internal | Self::data_loss => 500,
            Self::invalid_argument | Self::failed_precondition | Self::out_of_range => 400,
            Self::deadline_exceeded => 504,
            Self::not_found => 404,
            Self::already_exists | Self::aborted => 409,
            Self::permission_denied => 403,
            Self::resource_exhausted => 429,
            Self::unimplemented => 501,
            Self::unavailable => 503,
            Self::unauthenticated => 401,
        }
    }
    pub fn parse(value: &str) -> Self {
        match value {
            "canceled" => Self::canceled,
            "invalid_argument" => Self::invalid_argument,
            "deadline_exceeded" => Self::deadline_exceeded,
            "not_found" => Self::not_found,
            "already_exists" => Self::already_exists,
            "permission_denied" => Self::permission_denied,
            "resource_exhausted" => Self::resource_exhausted,
            "failed_precondition" => Self::failed_precondition,
            "aborted" => Self::aborted,
            "out_of_range" => Self::out_of_range,
            "unimplemented" => Self::unimplemented,
            "internal" => Self::internal,
            "unavailable" => Self::unavailable,
            "data_loss" => Self::data_loss,
            "unauthenticated" => Self::unauthenticated,
            _ => Self::unknown,
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WireError {
    pub code: Code,
    pub reason: Option<ErrorReason>,
}
impl WireError {
    const fn plain(code: Code) -> Self {
        Self { code, reason: None }
    }
    const fn with(code: Code, reason: ErrorReason) -> Self {
        Self {
            code,
            reason: Some(reason),
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Class {
    Structure,
    Size,
    Unsupported,
    State,
    Proof,
    TicketOther,
    TicketTime,
    TicketScope,
    Trust,
    Transport,
    Configuration,
    Client,
    Warning,
    EventServer,
    EventClient,
}

/// Exhaustive local catalog: adding a local reason requires an explicit class.
/// Context overrides (proofs, tickets and client response fields) are below.
pub const fn class(reason: Reason) -> Class {
    use Reason::*;
    match reason {
        nesting_too_deep
        | element_limit_exceeded
        | byte_limit_exceeded
        | body_too_large
        | envelope_too_large
        | chunk_size_too_large
        | size_overflow => Class::Size,
        unknown_payload_type
        | unsupported_manifest_format
        | unsupported_hash_algorithm
        | unknown_required_feature
        | unsupported_release_kind => Class::Unsupported,
        manifest_changed_for_version
        | stale_revision
        | revision_conflict
        | dependency_not_found
        | dependency_cycle
        | ticket_set_too_large => Class::State,
        proof_instance_mismatch
        | proof_device_mismatch
        | proof_challenge_mismatch
        | proof_pairing_mismatch
        | challenge_unknown
        | challenge_expired
        | challenge_consumed => Class::Proof,
        invalid_validity_window
        | ticket_not_yet_valid
        | ticket_key_retired
        | wrong_audience
        | unsorted_manifest_ids
        | wrong_subject => Class::TicketOther,
        ticket_expired => Class::TicketTime,
        ticket_scope => Class::TicketScope,
        instance_mismatch | content_key_changed => Class::Trust,
        trusted_set_size
        | invalid_trusted_key
        | role_mismatch
        | trusted_key_id_collision
        | invalid_ticket_key_set => Class::Configuration,
        not_an_impluvium_server
        | protocol_unsupported
        | server_too_old
        | client_too_old
        | manifest_set_mismatch
        | response_identity_mismatch => Class::Client,
        insecure_scheme_not_allowed
        | platform_trust_unavailable
        | origin_not_allowed
        | headers_too_large
        | redirect_not_allowed
        | redirect_cross_origin
        | redirect_limit
        | unsupported_content_encoding
        | response_too_large
        | redirect_target_not_allowed
        | request_too_large
        | rate_limited
        | server_unavailable
        | broken_error_body => Class::Transport,
        windows_symlink => Class::Warning,
        invalid_client_id | client_table_full | invalid_topic | invalid_parameter => {
            Class::EventServer
        }
        event_too_large
        | event_malformed
        | event_out_of_order
        | event_epoch_mismatch
        | event_name_mismatch
        | event_id_mismatch
        | event_sequence_invalid
        | event_payload_invalid
        | event_identity_mismatch => Class::EventClient,
        malformed_wire
        | wrong_wire_type
        | split_message
        | oneof_conflict
        | duplicate_map_key
        | invalid_json
        | duplicate_json_key
        | json_type_mismatch
        | invalid_number
        | integer_out_of_range
        | invalid_base64
        | invalid_timestamp
        | invalid_utf8
        | duplicate_field
        | no_signatures
        | malformed_signature_entry
        | invalid_payload_type
        | duplicate_signature_key_id
        | unexpected_payload_type
        | invalid_signature
        | no_trusted_signature
        | missing_field
        | invalid_uuid
        | missing_header
        | missing_chunking
        | invalid_name
        | unsorted_required_features
        | invalid_chunk_id
        | chunk_size_zero
        | unsorted_chunks
        | unsorted_entries
        | path_parent_conflict
        | invalid_entry_type
        | invalid_entry_fields
        | invalid_hash_length
        | invalid_mode
        | empty_file_hash_mismatch
        | chunk_index_out_of_range
        | chunk_size_mismatch
        | single_chunk_hash_mismatch
        | total_size_mismatch
        | unreferenced_chunk
        | path_empty
        | path_forbidden_character
        | path_absolute
        | path_drive_prefix
        | path_empty_component
        | path_dot_component
        | path_component_too_long
        | symlink_escape
        | windows_forbidden_character
        | windows_trailing_dot_or_space
        | windows_reserved_name
        | windows_case_collision
        | unsupported_symlink
        | invalid_revision
        | invalid_manifest_id
        | invalid_manifest_size
        | duplicate_requirement
        | self_requirement
        | invalid_key
        | invalid_text
        | invalid_hint_key
        | invalid_argument_text
        | invalid_env_name
        | invalid_env_value
        | duplicate_env_name
        | duplicate_launch_key
        | duplicate_setup_id
        | setup_only_with_launch_options
        | descriptor_version_mismatch
        | descriptor_game_mismatch
        | manifest_id_mismatch
        | manifest_size_mismatch
        | manifest_binding_mismatch
        | install_size_mismatch
        | executable_not_found
        | executable_not_regular
        | working_dir_not_found
        | invalid_challenge_length
        | invalid_pairing_id_length
        | invalid_user_code
        | unsupported_session_scope
        | duplicate_capability
        | invalid_public_key
        | invalid_length
        | value_out_of_range
        | duplicate_element
        | invalid_url
        | invalid_page_token
        | unknown_device
        | device_revoked
        | device_expired
        | pairing_unknown
        | pairing_expired
        | pairing_denied
        | slow_down
        | pairing_pending => Class::Structure,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Context {
    General,
    /// Any failure after the proof's enclosing size check.
    Proof,
    /// Depot verification, including encoding/envelope failures.
    Ticket,
    BeginSession,
    CompleteSession,
    Pairing,
    /// Includes unsorted_manifest_ids: a client failure, never a ticket error.
    Response,
}
pub fn to_wire(reason: Reason, context: Context) -> Option<WireError> {
    use {Code::*, ErrorReason as W, Reason as R};
    if context == Context::Proof {
        return Some(WireError::with(unauthenticated, W::ChallengeInvalid));
    }
    if context == Context::Ticket {
        return Some(match reason {
            R::ticket_expired => WireError::with(unauthenticated, W::TicketExpired),
            R::ticket_scope => WireError::with(permission_denied, W::TicketScope),
            _ => WireError::plain(unauthenticated),
        });
    }
    if context == Context::Response {
        return None;
    }
    let specific = match reason {
        R::unknown_device => Some(if context == Context::CompleteSession {
            WireError::with(unauthenticated, W::ChallengeInvalid)
        } else {
            WireError::plain(unauthenticated)
        }),
        R::device_revoked => Some(WireError::with(unauthenticated, W::DeviceRevoked)),
        R::device_expired => Some(WireError::with(unauthenticated, W::DeviceExpired)),
        R::client_too_old if context == Context::CompleteSession => {
            Some(WireError::with(failed_precondition, W::ClientTooOld))
        }
        R::rate_limited
            if matches!(
                context,
                Context::BeginSession | Context::CompleteSession | Context::Pairing
            ) =>
        {
            Some(WireError::with(resource_exhausted, W::RateLimited))
        }
        R::pairing_unknown | R::pairing_expired => {
            Some(WireError::with(failed_precondition, W::PairingExpired))
        }
        R::pairing_denied => Some(WireError::with(permission_denied, W::PairingDenied)),
        R::slow_down => Some(WireError::with(resource_exhausted, W::SlowDown)),
        R::pairing_pending => Some(WireError::with(failed_precondition, W::PairingPending)),
        _ => None,
    };
    if specific.is_some() {
        return specific;
    }
    Some(match class(reason) {
        Class::Structure => WireError::plain(invalid_argument),
        Class::Size => WireError::plain(resource_exhausted),
        Class::Unsupported | Class::State => WireError::plain(failed_precondition),
        Class::Proof => WireError::with(unauthenticated, W::ChallengeInvalid),
        Class::TicketOther => WireError::plain(unauthenticated),
        Class::TicketTime => WireError::with(unauthenticated, W::TicketExpired),
        Class::TicketScope => WireError::with(permission_denied, W::TicketScope),
        _ => return None,
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Action {
    Incompatible,
    UpdateClient,
    RenewSession,
    DeviceRevoked,
    DeviceExpired,
    MfaRequired,
    Poll,
    SlowDown,
    PairingDenied,
    PairingExpired,
    RefreshTicket,
    ReopenDownload,
    NotFound,
    StopDownload,
    RestartListing,
    Retry,
    Backoff,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WireStatus {
    Active(Code, Action),
    LocalOnly,
    Reserved,
}
/// All 22 listed enum values, including the local-only and reserved reasons.
/// Unknown numbers and names have no entry and fall back to the Connect code.
pub fn wire_catalog(reason: i32) -> Option<WireStatus> {
    use {Action::*, Code::*, WireStatus::*};
    Some(match reason {
        0 => return None,
        1 => Active(failed_precondition, Incompatible),
        2 => Active(failed_precondition, UpdateClient),
        3 => LocalOnly,
        4 => Active(unauthenticated, RenewSession),
        5 => Active(unauthenticated, DeviceRevoked),
        6 => Active(unauthenticated, DeviceExpired),
        7 => Active(permission_denied, MfaRequired),
        8 => Active(failed_precondition, Poll),
        9 => Active(resource_exhausted, SlowDown),
        10 => Active(permission_denied, PairingDenied),
        11 => Active(failed_precondition, PairingExpired),
        12 => Active(unauthenticated, RenewSession),
        13 => Active(unauthenticated, RefreshTicket),
        14 => Active(permission_denied, ReopenDownload),
        15 | 16 => Active(not_found, NotFound),
        17 => Active(aborted, StopDownload),
        18 => Active(failed_precondition, RestartListing),
        19 => Active(resource_exhausted, Retry),
        20 => Active(unavailable, Backoff),
        21 => Reserved,
        _ => return None,
    })
}
/// None means the client acts on the Connect code alone, including for unknown,
/// local-only or reserved reasons and code/reason combinations it cannot use.
pub fn client_action(code: Code, reason: i32) -> Option<Action> {
    match wire_catalog(reason) {
        Some(WireStatus::Active(expected, action)) if code == expected => Some(action),
        _ => None,
    }
}

/// Evidence supplied by the HTTP adapter, never inferred from error text or a
/// discovery fingerprint. Depot responses always use `Other` even at the server origin.
#[derive(Clone, Copy, Debug)]
pub enum Source<'a> {
    Server {
        response_url: &'a str,
        transport_verified: bool,
    },
    Other,
}
pub fn delete_credentials(
    base_url: &str,
    source: Source<'_>,
    body_prechecked: bool,
    code: Code,
    reason: i32,
) -> bool {
    let Source::Server {
        response_url,
        transport_verified: true,
    } = source
    else {
        return false;
    };
    if !body_prechecked || code != Code::unauthenticated || !matches!(reason, 5 | 6) {
        return false;
    }
    // The source surface and transport evidence come from the adapter; origin
    // equality is evaluated here against the actual stored context's base URL.
    match (url::Url::parse(base_url), url::Url::parse(response_url)) {
        (Ok(base), Ok(response)) => {
            matches!(base.scheme(), "http" | "https")
                && matches!(response.scheme(), "http" | "https")
                && base.origin() == response.origin()
        }
        _ => false,
    }
}

/// A Connect wrapper whose complete pre-check, including its optional binary
/// ErrorInfo detail, succeeded. Only the protected reader constructs it.
#[derive(Clone, Debug)]
pub struct ErrorBody {
    code: Code,
    info: Option<crate::proto::impluvium::v2::ErrorInfo>,
}
impl ErrorBody {
    pub(crate) fn checked(
        code: String,
        info: Option<crate::proto::impluvium::v2::ErrorInfo>,
    ) -> Self {
        Self {
            code: Code::parse(&code),
            info,
        }
    }
    pub fn code(&self) -> Code {
        self.code
    }
    pub fn info(&self) -> Option<&crate::proto::impluvium::v2::ErrorInfo> {
        self.info.as_ref()
    }
    pub fn wire(&self) -> WireError {
        WireError {
            code: self.code,
            reason: self
                .info
                .as_ref()
                .and_then(|i| <ErrorReason as buffa::Enumeration>::from_i32(i.reason.to_i32()))
                .filter(|r| *r != ErrorReason::Unspecified),
        }
    }
}
