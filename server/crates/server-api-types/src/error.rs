// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! The error body of the web API.
//!
//! Every error answered by the web API carries one [`ApiError`] as JSON. The
//! `code` is a stable snake_case string from the closed [`ErrorCode`] set; the
//! `message` is a fixed, bounded English sentence per code. Messages never
//! contain request input such as URLs, host names, paths or credentials, so a
//! client must translate by `code`, not by message text. Protocol endpoints
//! with their own error envelope are not covered by this module.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Upper bound for the length of an error message in bytes.
pub const MAX_MESSAGE_LEN: usize = 200;
/// Upper bound for the length of a request id in bytes.
pub const MAX_REQUEST_ID_LEN: usize = 64;
/// Upper bound for the number of field errors in one body.
pub const MAX_FIELD_ERRORS: usize = 32;
/// Upper bound for the length of a field name or field code in bytes.
pub const MAX_FIELD_LEN: usize = 64;

/// Declares [`ErrorCode`] and everything derived from its list: the wire
/// value, status and default message of each code, and [`ErrorCode::ALL`].
/// Because all of them come from this one list, a code cannot exist outside
/// `ALL` and its order cannot differ from the declaration order.
macro_rules! error_codes {
    ($($(#[$doc:meta])* $variant:ident => ($wire:literal, $status:literal, $message:literal),)+) => {
        /// The closed set of error codes. The wire value is the snake_case name.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
        #[serde(rename_all = "snake_case")]
        pub enum ErrorCode {
            $($(#[$doc])* $variant,)+
        }

        impl ErrorCode {
            const COUNT: usize = [$(stringify!($variant)),+].len();

            /// Every code in catalog order.
            pub const ALL: [ErrorCode; Self::COUNT] = [$(Self::$variant,)+];

            /// The stable wire value.
            pub const fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $wire,)+
                }
            }

            /// The HTTP status of the answer.
            pub const fn status(self) -> u16 {
                match self {
                    $(Self::$variant => $status,)+
                }
            }

            /// The fixed default message.
            pub const fn message(self) -> &'static str {
                match self {
                    $(Self::$variant => $message,)+
                }
            }
        }
    };
}

error_codes! {
    /// The request is malformed in a way no other code describes.
    BadRequest => ("bad_request", 400, "The request could not be processed."),
    /// The body is not JSON of the shape the endpoint expects.
    InvalidJsonBody => ("invalid_json_body", 400, "The request body is not valid JSON for this endpoint."),
    /// The query string is invalid.
    InvalidQuery => ("invalid_query", 400, "A query parameter is missing or invalid."),
    /// A path parameter is invalid.
    InvalidPathParam => ("invalid_path_param", 400, "A path parameter is invalid."),
    /// The forwarding headers of a trusted proxy are inconsistent.
    InvalidForwardingHeaders => ("invalid_forwarding_headers", 400, "The forwarding headers of the request are invalid."),
    /// A request body failed validation (sign-in and sign-up forms).
    InvalidRequestBody => ("invalid_request_body", 400, "The request body is invalid."),
    /// The sign-in method is switched off.
    SignInMethodDisabled => ("sign_in_method_disabled", 403, "This sign-in method is disabled."),
    /// Username or password is wrong.
    InvalidUsernameOrPassword => ("invalid_username_or_password", 401, "The username or password is invalid."),
    /// The account is disabled.
    AccountDisabled => ("account_disabled", 403, "This account is disabled."),
    /// The stored password cannot be used to sign in.
    InvalidPasswordState => ("invalid_password_state", 409, "The password of this account cannot be used to sign in."),
    /// Sign-up needs an invitation id and none was sent.
    InviteIdRequired => ("invite_id_required", 400, "An invitation id is required."),
    /// The invitation does not exist, ran out or was used.
    InvalidInvite => ("invalid_invite", 400, "The invitation is invalid or no longer valid."),
    /// The username belongs to another account.
    UsernameTaken => ("username_taken", 409, "This username is already taken."),
    /// The content type of the body is missing or not supported.
    UnsupportedMediaType => ("unsupported_media_type", 415, "The content type of the request is not supported."),
    /// The body exceeds the limit of the endpoint.
    PayloadTooLarge => ("payload_too_large", 413, "The request body is too large."),
    /// The presented credential is malformed.
    InvalidCredentials => ("invalid_credentials", 401, "The credentials of the request are invalid."),
    /// The endpoint needs a signed-in caller.
    Unauthenticated => ("unauthenticated", 401, "Authentication is required."),
    /// The caller lacks the required rights.
    Forbidden => ("forbidden", 403, "You are not allowed to do this."),
    /// The cross-site request check failed.
    CsrfRejected => ("csrf_rejected", 403, "The request was rejected by the cross-site request check."),
    /// No such resource.
    NotFound => ("not_found", 404, "The requested resource does not exist."),
    /// The resource does not support the method.
    MethodNotAllowed => ("method_not_allowed", 405, "This method is not allowed for this resource."),
    /// Handling the request took too long.
    RequestTimeout => ("request_timeout", 408, "The request took too long."),
    /// An unexpected failure of the server.
    InternalError => ("internal_error", 500, "An internal error occurred."),
    /// A dependency of the server is not available yet.
    NotReady => ("not_ready", 503, "The service is not ready to handle requests."),
    /// The endpoint is registered but has no implementation yet.
    NotImplemented => ("not_implemented", 501, "This function is not available yet."),
}

#[cfg(test)]
impl ErrorCode {
    /// Position in [`ErrorCode::ALL`], which is the declaration order.
    const fn index(self) -> usize {
        self as usize
    }
}

/// One invalid field of a validation error. Both parts are bounded to
/// [`MAX_FIELD_LEN`] bytes, also when the value is deserialised.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "RawFieldError")]
pub struct FieldError {
    field: String,
    code: String,
}

#[derive(Deserialize)]
struct RawFieldError {
    field: String,
    code: String,
}

impl From<RawFieldError> for FieldError {
    fn from(raw: RawFieldError) -> Self {
        Self::new(&raw.field, &raw.code)
    }
}

impl FieldError {
    /// Creates a field error; both parts are cut to [`MAX_FIELD_LEN`] bytes.
    pub fn new(field: &str, code: &str) -> Self {
        Self {
            field: bounded(field, MAX_FIELD_LEN),
            code: bounded(code, MAX_FIELD_LEN),
        }
    }

    /// Name of the offending field.
    pub fn field(&self) -> &str {
        &self.field
    }

    /// Stable snake_case reason for this field.
    pub fn code(&self) -> &str {
        &self.code
    }
}

/// The JSON body of every web API error.
///
/// For a response that was not produced with a specific code, the HTTP status
/// on the wire wins: the body names the nearest generic code, and the status
/// in the catalog is only that code's default.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ApiError {
    /// Stable machine-readable code.
    pub code: ErrorCode,
    /// Fixed English message for the code; never carries request input.
    pub message: String,
    /// Identifier of the request, as in the `X-Request-Id` response header.
    pub request_id: String,
    /// Invalid fields of a validation error; omitted when empty. The element type is spelled out
    /// because `FieldError` is bounded on read and so deserialises through a helper type.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(optional, type = "Array<{ field: string, code: string }>")]
    pub fields: Vec<FieldError>,
}

impl ApiError {
    /// Builds the body for a code. The message is the fixed default of the
    /// code; the request id and the field list are bounded.
    pub fn new(code: ErrorCode, request_id: &str, fields: &[FieldError]) -> Self {
        let id: String = request_id
            .chars()
            .filter(char::is_ascii_graphic)
            .take(MAX_REQUEST_ID_LEN)
            .collect();
        Self {
            code,
            message: bounded(code.message(), MAX_MESSAGE_LEN),
            request_id: id,
            fields: fields
                .iter()
                .take(MAX_FIELD_ERRORS)
                .map(|field| FieldError::new(field.field(), field.code()))
                .collect(),
        }
    }
}

fn bounded(text: &str, limit: usize) -> String {
    let mut end = text.len().min(limit);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_owned()
}

/// One row of the error catalog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CatalogEntry {
    /// Wire value of the code.
    pub code: &'static str,
    /// HTTP status of the answer.
    pub status: u16,
    /// Fixed default message.
    pub message: &'static str,
}

/// The full catalog in stable order. The committed file `error-codes.json`
/// next to the manifest of this crate holds the same data for consumers that
/// are not written in Rust. The status of a row is the default of its code;
/// for a response without a specific code the status on the wire wins.
pub fn catalog() -> Vec<CatalogEntry> {
    ErrorCode::ALL
        .iter()
        .map(|code| CatalogEntry {
            code: code.as_str(),
            status: code.status(),
            message: code.message(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    /// The committed catalog file.
    ///
    /// Format: the output of `serde_json::to_string_pretty` for the catalog
    /// (a JSON array of objects with the keys `code`, `status` and `message`
    /// in that order, indented by two spaces), followed by one newline.
    const COMMITTED: &str = include_str!("../error-codes.json");

    fn rendered() -> String {
        let mut text = serde_json::to_string_pretty(&catalog()).unwrap();
        text.push('\n');
        text
    }

    #[test]
    fn committed_catalog_matches_the_code() {
        assert!(
            COMMITTED == rendered(),
            "server/crates/server-api-types/error-codes.json is out of date. Replace its content \
             with the catalog rendered as serde_json::to_string_pretty plus a trailing newline:\n{}",
            rendered()
        );
    }

    #[test]
    fn wire_values_are_unique_snake_case_and_match_serde() {
        let mut seen = HashSet::new();
        for code in ErrorCode::ALL {
            let name = code.as_str();
            assert!(seen.insert(name), "duplicate code {name}");
            assert!(
                !name.is_empty()
                    && name
                        .bytes()
                        .all(|byte| byte.is_ascii_lowercase() || byte == b'_'),
                "{name} is not snake_case"
            );
            assert_eq!(
                serde_json::to_value(code).unwrap(),
                serde_json::Value::String(name.to_owned())
            );
        }
    }

    #[test]
    fn messages_are_bounded_plain_and_statuses_are_errors() {
        for code in ErrorCode::ALL {
            let message = code.message();
            assert!(!message.is_empty() && message.len() <= MAX_MESSAGE_LEN);
            assert!(message.is_ascii());
            assert!(!message.contains("://") && !message.contains('/'));
            assert!((400..=599).contains(&code.status()), "{}", code.as_str());
        }
    }

    #[test]
    fn existing_wire_values_stay_stable() {
        let expected = [
            (ErrorCode::NotFound, "not_found", 404),
            (ErrorCode::MethodNotAllowed, "method_not_allowed", 405),
            (ErrorCode::InternalError, "internal_error", 500),
            (
                ErrorCode::InvalidForwardingHeaders,
                "invalid_forwarding_headers",
                400,
            ),
            (ErrorCode::InvalidCredentials, "invalid_credentials", 401),
            (ErrorCode::Unauthenticated, "unauthenticated", 401),
            (ErrorCode::Forbidden, "forbidden", 403),
            (ErrorCode::CsrfRejected, "csrf_rejected", 403),
        ];
        for (code, name, status) in expected {
            assert_eq!((code.as_str(), code.status()), (name, status));
        }
    }

    #[test]
    fn body_serialises_with_optional_fields() {
        let plain = ApiError::new(ErrorCode::NotFound, "abc-1", &[]);
        let value = serde_json::to_value(&plain).unwrap();
        assert_eq!(
            value,
            serde_json::json!({
                "code": "not_found",
                "message": ErrorCode::NotFound.message(),
                "request_id": "abc-1",
            })
        );
        let fields = [FieldError::new("username", "too_short")];
        let invalid = ApiError::new(ErrorCode::InvalidRequestBody, "abc-2", &fields);
        let value = serde_json::to_value(&invalid).unwrap();
        assert_eq!(
            value["fields"],
            serde_json::json!([{ "field": "username", "code": "too_short" }])
        );
        let back: ApiError = serde_json::from_value(value).unwrap();
        assert_eq!(back, invalid);
    }

    #[test]
    fn typescript_union_names_every_wire_value() {
        use ts_rs::TS;
        let union = ErrorCode::inline(&ts_rs::Config::new());
        let expected: HashSet<String> = ErrorCode::ALL
            .iter()
            .map(|code| format!("\"{}\"", code.as_str()))
            .collect();
        assert_eq!(crate::enums::tests::union_members(&union), expected);
    }

    #[test]
    fn all_lists_every_variant_at_its_position() {
        for (position, code) in ErrorCode::ALL.iter().enumerate() {
            assert_eq!(code.index(), position, "{}", code.as_str());
        }
    }

    #[test]
    fn deserialised_field_errors_are_bounded() {
        let long = "x".repeat(500);
        let value = serde_json::json!({ "field": long, "code": long });
        let field: FieldError = serde_json::from_value(value).unwrap();
        assert_eq!(field.field().len(), MAX_FIELD_LEN);
        assert_eq!(field.code().len(), MAX_FIELD_LEN);
    }

    #[test]
    fn inputs_are_bounded() {
        let long_id = "a".repeat(500);
        let many: Vec<FieldError> = (0..100)
            .map(|index| FieldError::new(&"f".repeat(500), &index.to_string()))
            .collect();
        let error = ApiError::new(ErrorCode::BadRequest, &long_id, &many);
        assert_eq!(error.request_id.len(), MAX_REQUEST_ID_LEN);
        assert_eq!(error.fields.len(), MAX_FIELD_ERRORS);
        assert!(
            error
                .fields
                .iter()
                .all(|f| f.field().len() == MAX_FIELD_LEN)
        );
        let odd = ApiError::new(ErrorCode::BadRequest, "id with space\nand\u{e9}", &[]);
        assert_eq!(odd.request_id, "idwithspaceand");
        let cut = FieldError::new(&"\u{e9}".repeat(100), "x");
        assert!(cut.field().len() <= MAX_FIELD_LEN);
    }
}
