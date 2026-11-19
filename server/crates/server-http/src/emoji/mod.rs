// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! `GET /api/v1/emoji/{codepoint}`: the emoji pictures for signed-in callers.
//!
//! The pictures are held in memory ([`EmojiFiles`]); a request never touches
//! the disk and the code point of a request is only ever used as a key of that
//! map, never as part of a file path. The caller needs the right `emoji:read`:
//! the roles user and admin have it, and a token needs it explicitly while its
//! owner must hold it as well. The registry policy expresses this; the handler
//! checks nothing about the caller.
//!
//! A code point is one or more lowercase hexadecimal numbers of one to six
//! digits joined by `-`, such as `1f600` or `1f1e9-1f1ea`, at most
//! [`MAX_PARTS`] numbers. Anything else is a `400`; a well-formed name without
//! a picture is a `404`.

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use axum::body::{Body, Bytes};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use server_api_types::ErrorCode;
use server_domain::policy::{Policy, Requirement, Right};

use crate::error::HttpError;
use crate::extract::ApiPath;
use crate::registry::{Contract, Limits, Method, ParamType, ResponseKind, Route};

/// Most numbers a code point may join. The longest emoji sequences have ten.
pub const MAX_PARTS: usize = 12;
/// Most hexadecimal digits of one number.
const MAX_DIGITS: usize = 6;
/// Longest accepted name: the longest number sequence with its separators.
const MAX_NAME_LEN: usize = MAX_PARTS * (MAX_DIGITS + 1) - 1;

const RIGHT: &str = "emoji:read";
const CACHE: &str = "private, max-age=31536000";

/// Whether the text is a code point name: one to [`MAX_PARTS`] numbers of one
/// to six lowercase hexadecimal digits, joined by `-`.
pub fn is_codepoint(text: &str) -> bool {
    if text.is_empty() || text.len() > MAX_NAME_LEN {
        return false;
    }
    let mut parts = 0usize;
    for part in text.split('-') {
        parts += 1;
        let digits = part.len();
        if parts > MAX_PARTS
            || digits == 0
            || digits > MAX_DIGITS
            || !part
                .bytes()
                .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
        {
            return false;
        }
    }
    true
}

/// Why a set of pictures is refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmojiFilesError {
    /// A name is not a code point name.
    BadName(String),
    /// A name occurs twice.
    DuplicateName(String),
}

impl fmt::Display for EmojiFilesError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadName(name) => write!(formatter, "{name:?} is not a code point name"),
            Self::DuplicateName(name) => write!(formatter, "{name:?} occurs twice"),
        }
    }
}

impl std::error::Error for EmojiFilesError {}

/// The emoji pictures, by code point name without the `.svg` suffix.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct EmojiFiles {
    pictures: HashMap<String, Bytes>,
}

impl EmojiFiles {
    /// No pictures: every well-formed request is a `404`.
    pub fn empty() -> Self {
        Self::default()
    }

    /// Holds the pictures. Every name must be a code point name and unique.
    pub fn new(entries: Vec<(String, Vec<u8>)>) -> Result<Self, EmojiFilesError> {
        let mut pictures = HashMap::with_capacity(entries.len());
        for (name, bytes) in entries {
            if !is_codepoint(&name) {
                return Err(EmojiFilesError::BadName(name));
            }
            if pictures.insert(name.clone(), Bytes::from(bytes)).is_some() {
                return Err(EmojiFilesError::DuplicateName(name));
            }
        }
        Ok(Self { pictures })
    }

    /// Number of pictures.
    pub fn len(&self) -> usize {
        self.pictures.len()
    }

    /// Whether there are no pictures.
    pub fn is_empty(&self) -> bool {
        self.pictures.is_empty()
    }
}

fn answer(files: &EmojiFiles, name: &str) -> Response {
    if !is_codepoint(name) {
        return HttpError::new(ErrorCode::InvalidPathParam).into_response();
    }
    let Some(bytes) = files.pictures.get(name) else {
        return HttpError::new(ErrorCode::NotFound).into_response();
    };
    let mut response = Response::new(Body::from(bytes.clone()));
    *response.status_mut() = StatusCode::OK;
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("image/svg+xml"),
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static(CACHE));
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    response
}

/// The emoji route.
///
/// # Panics
///
/// Never in practice: path, right and contract are fixed and valid, which the
/// tests of this module check.
pub fn routes(files: Arc<EmojiFiles>) -> Vec<Route> {
    let requirement = Requirement::authenticated()
        .require_rights(&[Right::new(RIGHT).expect("the right name is valid")])
        .expect("the list of rights is not empty");
    let route = Route::new(
        Method::Get,
        "/api/v1/emoji/{codepoint}",
        Policy::protected(requirement),
        Contract::new(
            ResponseKind::Stream,
            &[200],
            Limits::new(0, Duration::from_secs(10)),
        )
        .path_param("codepoint", ParamType::String),
        move |ApiPath(name): ApiPath<String>| {
            let files = Arc::clone(&files);
            async move { answer(&files, &name) }
        },
    )
    .expect("the emoji route is valid");
    vec![route]
}

#[cfg(test)]
mod tests;
