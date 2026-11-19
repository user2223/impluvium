// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! The canonical request path of the web application, and the classification built on it.
//!
//! One canonical form serves both the decision (server route, asset or page) and the file lookup,
//! so the two can never disagree about what a request names. Empty and `.` segments are dropped,
//! and every segment is percent-decoded exactly once. A path that stays ambiguous after that is
//! refused instead of being interpreted: a `..` segment, a second layer of percent-encoding, an
//! encoded separator, `;`, `\`, and control characters.

/// The request path cannot be reduced to a canonical form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AmbiguousPath;

/// Prefixes whose missing files are never answered with the page shell.
const ASSET_PREFIXES: [&str; 4] = ["_nuxt", "_fonts", "fonts", "wallpapers"];

/// Extensions of files that the application serves as assets, compared without regard to case. A
/// missing file with one of these extensions is a plain miss, not a client route.
const ASSET_EXTENSIONS: &[&str] = &[
    "html",
    "htm",
    "js",
    "mjs",
    "css",
    "json",
    "map",
    "png",
    "jpg",
    "jpeg",
    "gif",
    "webp",
    "avif",
    "svg",
    "ico",
    "ttf",
    "otf",
    "woff",
    "woff2",
    "txt",
    "xml",
    "webmanifest",
    "wasm",
    "gz",
    "br",
    "mp3",
    "ogg",
    "mp4",
    "webm",
    "pdf",
];

fn hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// Decodes the percent escapes of one segment, once.
fn decode_once(segment: &str) -> Result<String, AmbiguousPath> {
    let bytes = segment.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] == b'%' {
            let high = bytes.get(at + 1).copied().and_then(hex);
            let low = bytes.get(at + 2).copied().and_then(hex);
            match (high, low) {
                (Some(high), Some(low)) => out.push((high << 4) | low),
                _ => return Err(AmbiguousPath),
            }
            at += 3;
        } else {
            out.push(bytes[at]);
            at += 1;
        }
    }
    String::from_utf8(out).map_err(|_| AmbiguousPath)
}

/// The decoded segments of a request path, without the query.
///
/// The path must start with `/`. The result is empty for the root.
pub(crate) fn canonical_segments(raw: &str) -> Result<Vec<String>, AmbiguousPath> {
    if !raw.starts_with('/') {
        return Err(AmbiguousPath);
    }
    let mut segments = Vec::new();
    for part in raw.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        let decoded = decode_once(part)?;
        if decoded == "." {
            continue;
        }
        let reserved =
            |character: char| character.is_control() || matches!(character, '%' | ';' | '\\' | '/');
        if decoded == ".." || decoded.chars().any(reserved) {
            return Err(AmbiguousPath);
        }
        segments.push(decoded);
    }
    Ok(segments)
}

/// Whether the path belongs to the server: the API, the depot, and the concrete server routes
/// under `/auth`. Such a path is never answered with the page shell. The browser pages under
/// `/auth` are not listed here.
pub(crate) fn is_server_path(segments: &[String]) -> bool {
    let parts: Vec<&str> = segments.iter().map(String::as_str).collect();
    matches!(
        parts.as_slice(),
        ["api" | "depot", ..]
            | ["auth", "oidc"]
            | ["auth", "signout"]
            | ["auth", "callback", "oidc"]
    )
}

/// Whether the path lies in an asset prefix, or is one.
pub(crate) fn in_asset_prefix(segments: &[String]) -> bool {
    segments
        .first()
        .is_some_and(|first| ASSET_PREFIXES.contains(&first.as_str()))
}

/// Whether the last segment ends in a known asset extension.
pub(crate) fn has_asset_extension(segments: &[String]) -> bool {
    segments
        .last()
        .and_then(|last| last.rsplit_once('.'))
        .is_some_and(|(_, extension)| {
            ASSET_EXTENSIONS
                .iter()
                .any(|known| known.eq_ignore_ascii_case(extension))
        })
}

/// The media type of a served file, from its name.
pub(crate) fn content_type(name: &str) -> &'static str {
    let extension = name
        .rsplit_once('.')
        .map_or("", |(_, extension)| extension)
        .to_ascii_lowercase();
    match extension.as_str() {
        "html" | "htm" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" => "application/json; charset=utf-8",
        "map" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "ico" => "image/x-icon",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "txt" => "text/plain; charset=utf-8",
        "xml" => "application/xml; charset=utf-8",
        "webmanifest" => "application/manifest+json",
        "wasm" => "application/wasm",
        "mp3" => "audio/mpeg",
        "ogg" => "audio/ogg",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "pdf" => "application/pdf",
        _ => "application/octet-stream",
    }
}

fn is_word(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

fn is_word_or_dash(byte: u8) -> bool {
    is_word(byte) || byte == b'-'
}

/// Whether a file name carries a content hash: a run of at least eight word characters or dashes
/// that follows a dot or a dash and precedes a dot, or a name that consists of such a run in front
/// of a script or style extension (with or without a source map suffix).
fn name_is_hashed(name: &str) -> bool {
    let bytes = name.as_bytes();
    let before_extension = [".js", ".mjs", ".css", ".js.map", ".mjs.map", ".css.map"]
        .iter()
        .any(|suffix| {
            name.strip_suffix(suffix)
                .is_some_and(|stem| stem.len() >= 8 && stem.bytes().all(is_word_or_dash))
        });
    before_extension
        || bytes.iter().enumerate().any(|(index, byte)| {
            if *byte != b'.' && *byte != b'-' {
                return false;
            }
            let run = bytes[index + 1..]
                .iter()
                .take_while(|byte| is_word_or_dash(**byte))
                .count();
            run >= 8 && bytes.get(index + 1 + run) == Some(&b'.')
        })
}

/// Whether a file of the artifact is provably content-addressed by its name: a script, style or
/// asset below `_nuxt/` whose name holds a hash. Everything else must be revalidated.
pub(crate) fn is_hashed_asset(file: &str) -> bool {
    let Some(rest) = file.strip_prefix("_nuxt/") else {
        return false;
    };
    let name = rest.rsplit('/').next().unwrap_or(rest);
    !super::is_html_file(file) && name_is_hashed(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn segments(raw: &str) -> Result<Vec<String>, AmbiguousPath> {
        canonical_segments(raw)
    }

    #[test]
    fn empty_and_dot_segments_are_dropped_and_segments_decode_once() {
        assert_eq!(segments("/"), Ok(vec![]));
        assert_eq!(segments("//a/./b//"), Ok(vec!["a".into(), "b".into()]));
        assert_eq!(segments("/%2e/%2E/a"), Ok(vec!["a".into()]));
        assert_eq!(segments("/a%20b"), Ok(vec!["a b".into()]));
        assert_eq!(segments("/caf%C3%A9"), Ok(vec!["caf\u{e9}".into()]));
    }

    #[test]
    fn ambiguous_paths_are_refused() {
        for raw in [
            "/a/..",
            "/a/%2e%2E",
            "/a%252Eb",
            "/a%25",
            "/a%2Fb",
            "/a%5cb",
            "/a;b",
            "/a%3Bb",
            "/a%00b",
            "/a%0Ab",
            "/a%7F",
            "/a%zz",
            "/a%2",
            "/a%",
            "/a%FF",
            "relative",
            "",
        ] {
            assert_eq!(segments(raw), Err(AmbiguousPath), "{raw:?}");
        }
    }

    #[test]
    fn server_paths_are_the_namespaces_and_the_concrete_auth_routes() {
        let server = |raw: &str| is_server_path(&segments(raw).unwrap());
        for raw in [
            "/api",
            "/api/",
            "/api/v1/x",
            "//api/x",
            "/%61pi/x",
            "/depot",
            "/depot/x/y",
            "/auth/oidc",
            "/auth//oidc",
            "/auth/signout",
            "/auth/callback/oidc",
        ] {
            assert!(server(raw), "{raw}");
        }
        for raw in [
            "/",
            "/API/x",
            "/apis",
            "/store/api",
            "/auth",
            "/auth/signin",
            "/auth/register",
            "/auth/mfa",
            "/auth/mfa/totp",
            "/auth/mfa/webauthn",
            "/auth/oidc/x",
            "/auth/callback",
            "/auth/callback/other",
        ] {
            assert!(!server(raw), "{raw}");
        }
    }

    #[test]
    fn asset_prefixes_and_extensions_are_recognised_without_regard_to_case() {
        let prefix = |raw: &str| in_asset_prefix(&segments(raw).unwrap());
        let extension = |raw: &str| has_asset_extension(&segments(raw).unwrap());
        for raw in [
            "/_nuxt",
            "/_nuxt/x",
            "/_fonts/",
            "/fonts/a/b",
            "/wallpapers/x",
        ] {
            assert!(prefix(raw), "{raw}");
        }
        assert!(!prefix("/store/_nuxt/x"));
        for raw in [
            "/a.js", "/a.JS", "/a/b.Png", "/a.woff2", "/.js", "/a.br", "/a.GZ",
        ] {
            assert!(extension(raw), "{raw}");
        }
        for raw in ["/", "/a", "/a.b", "/a.js/b", "/store/1.5", "/a."] {
            assert!(!extension(raw), "{raw}");
        }
    }

    #[test]
    fn only_content_hashed_files_below_the_asset_directory_are_hashed() {
        for file in [
            "_nuxt/entry.D3f9a8b1.js",
            "_nuxt/index-BxQ1ab9c.css",
            "_nuxt/Bxq1ab9c-_.js",
            "_nuxt/abcdefgh.mjs",
            "_nuxt/abcdefgh.css.map",
            "_nuxt/sub/logo.0123456789.svg",
            "_nuxt/font-0123_abcd.woff2",
        ] {
            assert!(is_hashed_asset(file), "{file}");
        }
        for file in [
            "index.html",
            "loading.css",
            "_nuxt/app.js",
            "_nuxt/short1.js",
            "_nuxt/builds/latest.json",
            "_nuxt/page.0123456789.html",
            "_nuxt/page.0123456789.HTML",
            "_nuxt/page.0123456789.htm",
            "_fonts/inter-0123456789.woff2",
            "wallpapers/0123456789abcdef.jpg",
            "x/_nuxt/entry.D3f9a8b1.js",
        ] {
            assert!(!is_hashed_asset(file), "{file}");
        }
    }

    #[test]
    fn media_types_follow_the_extension() {
        assert_eq!(content_type("index.html"), "text/html; charset=utf-8");
        assert_eq!(content_type("_nuxt/a.JS"), "text/javascript; charset=utf-8");
        assert_eq!(content_type("a.woff2"), "font/woff2");
        assert_eq!(content_type("a.unknown"), "application/octet-stream");
        assert_eq!(content_type("noextension"), "application/octet-stream");
    }
}
