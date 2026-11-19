// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Serving the built single-page web application.
//!
//! The files of a checked web application directory are held in memory ([`SpaFiles`]); a request
//! never touches the disk. The router answers with them wherever no registered route matches.
//!
//! The request path is reduced once to a canonical form (see the `path` module), and that form
//! decides everything:
//!
//! - an ambiguous path (`..`, double encoding, encoded separators, `;`, `\`, control characters)
//!   gets a `400` error answer;
//! - the API and depot namespaces and the concrete server routes under `/auth` are never answered
//!   with a page; they get the usual `404` error answer when no route took the request;
//! - a file of the web application is served as it is. A missing file below an asset prefix
//!   (`_nuxt`, `_fonts`, `fonts`, `wallpapers`) or with the extension of an asset is a `404`;
//! - any other path is a client route and gets the page shell, `index.html`, with status `200`.
//!
//! Hashed build assets are cached for a year and marked immutable; every other file, the shell
//! included, must be revalidated. Precompressed siblings (`.br`, `.gz`) are served to clients that
//! accept them, and every file answer varies on `Accept-Encoding`. The `ETag` of an answer is the
//! SHA-256 digest of the bytes sent, which makes `If-None-Match` revalidation a `304`.
//!
//! Only `GET` and `HEAD` are answered; other methods get a `405` error answer. Ranges are not
//! supported.

mod path;
#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::fmt;

use axum::body::{Body, Bytes};
use axum::extract::Request;
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use axum::response::{IntoResponse, Response};
use server_api_types::ErrorCode;

use crate::error::error_response;
use crate::headers::SpaShell;

const SHELL: &str = "index.html";
const IMMUTABLE: &str = "public, max-age=31536000, immutable";
const REVALIDATE: &str = "no-cache";

/// One file of the web application, as the check of the directory read it.
#[derive(Debug, Clone)]
pub struct SpaEntry {
    /// The name relative to the directory, with `/` between the parts.
    pub name: String,
    /// The bytes of the file.
    pub bytes: Vec<u8>,
    /// The standard base64 SHA-256 digest of the bytes, with padding.
    pub sha256: String,
}

/// Why the files cannot be served.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpaFilesError {
    /// There is no `index.html`, the page shell.
    MissingShell,
    /// A name occurs twice.
    DuplicateName(String),
    /// The digest of a file is not standard base64.
    BadDigest(String),
}

impl fmt::Display for SpaFilesError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingShell => write!(formatter, "the web application has no {SHELL}"),
            Self::DuplicateName(name) => write!(formatter, "{name:?} occurs twice"),
            Self::BadDigest(name) => write!(formatter, "the digest of {name:?} is not base64"),
        }
    }
}

impl std::error::Error for SpaFilesError {}

#[derive(Debug, PartialEq, Eq)]
struct StoredFile {
    bytes: Bytes,
    etag: HeaderValue,
}

/// The files of a web application, ready to be served.
#[derive(PartialEq, Eq)]
pub struct SpaFiles {
    files: HashMap<String, StoredFile>,
}

impl fmt::Debug for SpaFiles {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SpaFiles")
            .field("files", &self.files.len())
            .finish()
    }
}

fn is_base64(text: &str) -> bool {
    !text.is_empty()
        && text
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'/' | b'='))
}

impl SpaFiles {
    /// Holds the files. The page shell `index.html` must be among them.
    pub fn new(entries: Vec<SpaEntry>) -> Result<Self, SpaFilesError> {
        let mut files = HashMap::with_capacity(entries.len());
        for entry in entries {
            if !is_base64(&entry.sha256) {
                return Err(SpaFilesError::BadDigest(entry.name));
            }
            let etag = HeaderValue::from_str(&format!("\"{}\"", entry.sha256))
                .map_err(|_| SpaFilesError::BadDigest(entry.name.clone()))?;
            let stored = StoredFile {
                bytes: Bytes::from(entry.bytes),
                etag,
            };
            if files.insert(entry.name.clone(), stored).is_some() {
                return Err(SpaFilesError::DuplicateName(entry.name));
            }
        }
        if !files.contains_key(SHELL) {
            return Err(SpaFilesError::MissingShell);
        }
        Ok(Self { files })
    }

    /// Whether a request may name this file directly. The precompressed siblings are only
    /// ever sent in place of their original.
    fn is_servable(&self, name: &str) -> bool {
        !name.is_empty()
            && !name.ends_with(".br")
            && !name.ends_with(".gz")
            && self.files.contains_key(name)
    }
}

/// Whether a file of the application is an HTML document, judged by its name in any letter case.
/// The same judgement decides how a file is served, whether it may be cached as immutable and
/// whether the check of the directory scans it for inline scripts.
pub fn is_html_file(name: &str) -> bool {
    path::content_type(name).starts_with("text/html")
}

/// Answers a request that no registered route took.
pub(crate) fn respond(files: &SpaFiles, request: &Request) -> Response {
    let Ok(segments) = path::canonical_segments(request.uri().path()) else {
        return error_response(ErrorCode::BadRequest);
    };
    if path::is_server_path(&segments) {
        return error_response(ErrorCode::NotFound);
    }
    let head = match *request.method() {
        Method::GET => false,
        Method::HEAD => true,
        _ => {
            let mut response = error_response(ErrorCode::MethodNotAllowed);
            response
                .headers_mut()
                .insert(header::ALLOW, HeaderValue::from_static("GET, HEAD"));
            return response;
        }
    };
    let name = segments.join("/");
    let target = if files.is_servable(&name) {
        name.as_str()
    } else if path::in_asset_prefix(&segments) || path::has_asset_extension(&segments) {
        return error_response(ErrorCode::NotFound);
    } else {
        SHELL
    };
    serve_file(files, target, head, request.headers())
}

fn serve_file(files: &SpaFiles, target: &str, head: bool, request: &HeaderMap) -> Response {
    let Some(plain) = files.files.get(target) else {
        return error_response(ErrorCode::InternalError);
    };
    let html = is_html_file(target);
    let mut chosen = plain;
    let mut encoding = None;
    // The script hashes of the policy and the check for inline styles cover the plain bytes of
    // an HTML file only, so a precompressed sibling of one is never sent.
    let offered = if html {
        Vec::new()
    } else {
        accepted_encodings(request)
    };
    for (token, suffix) in offered {
        if let Some(sibling) = files.files.get(&format!("{target}{suffix}")) {
            chosen = sibling;
            encoding = Some(token);
            break;
        }
    }
    let cache = if path::is_hashed_asset(target) {
        IMMUTABLE
    } else {
        REVALIDATE
    };
    let media_type = path::content_type(target);
    let mut builder = Response::builder()
        .header(header::CACHE_CONTROL, cache)
        .header(header::VARY, "Accept-Encoding")
        .header(header::ETAG, chosen.etag.clone());
    let body = if matches_etag(request, &chosen.etag) {
        builder = builder.status(StatusCode::NOT_MODIFIED);
        Body::empty()
    } else {
        builder = builder
            .header(header::CONTENT_TYPE, media_type)
            .header(header::CONTENT_LENGTH, chosen.bytes.len());
        if let Some(token) = encoding {
            builder = builder.header(header::CONTENT_ENCODING, token);
        }
        if head {
            Body::empty()
        } else {
            Body::from(chosen.bytes.clone())
        }
    };
    let Ok(response) = builder.body(body) else {
        return error_response(ErrorCode::InternalError);
    };
    // The same marking on a `304` keeps a cache from replacing the policy of the page.
    if html {
        SpaShell(response).into_response()
    } else {
        response
    }
}

/// Whether `If-None-Match` names the entity tag, weakly compared, or is `*`.
fn matches_etag(request: &HeaderMap, etag: &HeaderValue) -> bool {
    request.get_all(header::IF_NONE_MATCH).iter().any(|value| {
        value.as_bytes().split(|byte| *byte == b',').any(|item| {
            let item = item.trim_ascii();
            let item = item.strip_prefix(b"W/").unwrap_or(item);
            item == b"*" || item == etag.as_bytes()
        })
    })
}

/// The quality of a `q=` parameter in thousandths; anything malformed counts as zero.
fn quality(text: &str) -> u16 {
    let (whole, fraction) = text.split_once('.').unwrap_or((text, ""));
    if !fraction.bytes().all(|byte| byte.is_ascii_digit()) {
        return 0;
    }
    let thousandths: String = fraction
        .chars()
        .chain(std::iter::repeat('0'))
        .take(3)
        .collect();
    match (whole, thousandths.parse::<u16>()) {
        ("0", Ok(number)) => number,
        ("1", Ok(_)) => 1000,
        _ => 0,
    }
}

/// The encodings the client accepts and the artifact may offer, best first, as the token of
/// `Content-Encoding` and the suffix of the sibling file. A listed name beats `*`; a quality of
/// zero refuses; at equal quality `br` comes first.
fn accepted_encodings(request: &HeaderMap) -> Vec<(&'static str, &'static str)> {
    let (mut brotli, mut gzip, mut any) = (None, None, None);
    for value in request.get_all(header::ACCEPT_ENCODING) {
        let Ok(text) = value.to_str() else {
            continue;
        };
        for entry in text.split(',') {
            let mut parts = entry.split(';');
            let name = parts.next().unwrap_or("").trim().to_ascii_lowercase();
            let mut weight = 1000;
            for parameter in parts {
                let parameter = parameter.trim();
                if let Some(text) = parameter
                    .strip_prefix("q=")
                    .or_else(|| parameter.strip_prefix("Q="))
                {
                    weight = quality(text.trim());
                }
            }
            match name.as_str() {
                "br" => brotli = Some(weight),
                "gzip" | "x-gzip" => gzip = Some(weight),
                "*" => any = Some(weight),
                _ => {}
            }
        }
    }
    let mut offered = [
        (brotli.or(any).unwrap_or(0), "br", ".br"),
        (gzip.or(any).unwrap_or(0), "gzip", ".gz"),
    ];
    offered.sort_by_key(|(weight, ..)| std::cmp::Reverse(*weight));
    offered
        .into_iter()
        .filter(|(weight, ..)| *weight > 0)
        .map(|(_, token, suffix)| (token, suffix))
        .collect()
}
