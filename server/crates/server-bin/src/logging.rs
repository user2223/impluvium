// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Structured JSON-lines logging.
//!
//! Every log entry is one JSON object on one line with a timestamp, a level,
//! a message, optionally a request id and selected fields. Request entries
//! hold method, route template, status and duration; they never hold query
//! strings or header values. String values pass through [`redact`] before
//! they are written, and the panic hook records the code location only,
//! never the payload.
//!
//! The module depends on `serde_json` and the request log trait of
//! `server-http` only, so it can be included on its own in tests.

#![allow(dead_code)]

use std::io::Write;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Map, Value};
use server_http::observe::{RequestLogEntry, RequestLogSink};

/// Placeholder for a removed secret.
pub(crate) const REDACTED: &str = "[redacted]";

/// Severity of an entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Level {
    /// Normal operation.
    Info,
    /// Something went wrong for one caller.
    Warn,
    /// Something went wrong on the server.
    Error,
}

impl Level {
    fn as_str(self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Warn => "warn",
            Self::Error => "error",
        }
    }
}

/// Writes JSON lines to a destination.
pub(crate) struct JsonLogger {
    out: Mutex<Box<dyn Write + Send>>,
}

impl JsonLogger {
    /// A logger that writes to `writer`.
    pub(crate) fn new(writer: Box<dyn Write + Send>) -> Self {
        Self {
            out: Mutex::new(writer),
        }
    }

    /// Writes one entry. String values among `fields` and the message are
    /// redacted; the reserved names `ts`, `level`, `message` and
    /// `request_id` cannot be overridden by `fields`.
    pub(crate) fn log(
        &self,
        level: Level,
        message: &str,
        request_id: Option<&str>,
        fields: Map<String, Value>,
    ) {
        let line = render(now(), level, message, request_id, fields);
        let mut out = self.out.lock().unwrap_or_else(PoisonError::into_inner);
        // A log that cannot be written has nowhere to report that.
        let _ = writeln!(out, "{line}");
        let _ = out.flush();
    }

    /// Logs an error message. The text may contain paths or credentials of
    /// the failing operation and is redacted.
    pub(crate) fn error(&self, message: &str) {
        self.log(Level::Error, message, None, Map::new());
    }

    /// Logs a panic by its code location only. The payload is not read.
    pub(crate) fn panic_at(&self, location: Option<(&str, u32, u32)>) {
        let mut fields = Map::new();
        if let Some((file, line, column)) = location {
            // The location is a compile-time constant of the source, not input.
            fields.insert("file".into(), Value::String(file.to_owned()));
            fields.insert("line".into(), line.into());
            fields.insert("column".into(), column.into());
        }
        let line = render_raw(now(), Level::Error, "panic", None, fields);
        let mut out = self.out.lock().unwrap_or_else(PoisonError::into_inner);
        let _ = writeln!(out, "{line}");
        let _ = out.flush();
    }
}

impl RequestLogSink for JsonLogger {
    fn request(&self, entry: &RequestLogEntry) {
        let status = entry.status;
        let level = match status {
            500.. => Level::Error,
            400..=499 => Level::Warn,
            _ => Level::Info,
        };
        let mut fields = Map::new();
        let method: String = entry
            .method
            .chars()
            .filter(char::is_ascii_uppercase)
            .collect();
        fields.insert("method".into(), Value::String(method));
        // The entry holds a route template; the query string is cut off anyway.
        let path: String = entry
            .path
            .split('?')
            .next()
            .unwrap_or_default()
            .chars()
            .take(200)
            .collect();
        fields.insert("path".into(), Value::String(path));
        fields.insert("status".into(), status.into());
        let millis = u64::try_from(entry.duration.as_millis()).unwrap_or(u64::MAX);
        fields.insert("duration_ms".into(), millis.into());
        let line = render_raw(now(), level, "request", Some(&entry.request_id), fields);
        let mut out = self.out.lock().unwrap_or_else(PoisonError::into_inner);
        let _ = writeln!(out, "{line}");
        let _ = out.flush();
    }
}

/// Installs a process-wide panic hook that logs the location and nothing of
/// the payload.
pub(crate) fn install_panic_hook(logger: Arc<JsonLogger>) {
    std::panic::set_hook(Box::new(move |info| {
        let location = info
            .location()
            .map(|place| (place.file(), place.line(), place.column()));
        logger.panic_at(location);
    }));
}

fn render(
    timestamp: String,
    level: Level,
    message: &str,
    request_id: Option<&str>,
    fields: Map<String, Value>,
) -> String {
    let cleaned: Map<String, Value> = fields
        .into_iter()
        .map(|(name, value)| (name, sanitize(value)))
        .collect();
    render_raw(timestamp, level, &redact(message), request_id, cleaned)
}

fn render_raw(
    timestamp: String,
    level: Level,
    message: &str,
    request_id: Option<&str>,
    mut fields: Map<String, Value>,
) -> String {
    fields.insert("ts".into(), Value::String(timestamp));
    fields.insert("level".into(), Value::String(level.as_str().into()));
    fields.insert("message".into(), Value::String(message.to_owned()));
    if let Some(id) = request_id {
        let id: String = id.chars().filter(char::is_ascii_graphic).take(64).collect();
        fields.insert("request_id".into(), Value::String(id));
    }
    // A map of strings, numbers and nested values always serialises.
    serde_json::to_string(&Value::Object(fields)).unwrap_or_default()
}

fn sanitize(value: Value) -> Value {
    match value {
        Value::String(text) => Value::String(redact(&text)),
        Value::Array(items) => Value::Array(items.into_iter().map(sanitize).collect()),
        Value::Object(map) => Value::Object(
            map.into_iter()
                .map(|(name, inner)| (name, sanitize(inner)))
                .collect(),
        ),
        other => other,
    }
}

const SENSITIVE_NAMES: [&str; 11] = [
    "password",
    "passwd",
    "secret",
    "token",
    "key",
    "authorization",
    "cookie",
    "session",
    "signature",
    "credential",
    "auth",
];

fn is_separator(character: char) -> bool {
    character.is_whitespace()
        || matches!(
            character,
            '"' | '\'' | ',' | ';' | '(' | ')' | '{' | '}' | '[' | ']' | '<' | '>'
        )
}

fn looks_like_path(word: &str) -> bool {
    let bytes = word.as_bytes();
    word.starts_with('/')
        || word.starts_with("~/")
        || word.starts_with("\\\\")
        || (bytes.len() >= 3
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && (bytes[2] == b'\\' || bytes[2] == b'/'))
}

fn looks_like_secret(word: &str) -> bool {
    word.len() >= 24
        && word.bytes().any(|byte| byte.is_ascii_digit())
        && word
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_-+=./".contains(&byte))
}

/// Carries across words of one text.
#[derive(Default)]
struct State {
    hide_line: bool,
    hide_next: bool,
}

fn redact_word(word: &str, state: &mut State) -> String {
    if state.hide_line || state.hide_next {
        state.hide_next = false;
        return REDACTED.to_owned();
    }
    let lower = word.to_ascii_lowercase();
    let header = lower.strip_suffix(':').unwrap_or_default();
    if matches!(
        header,
        "authorization" | "proxy-authorization" | "cookie" | "set-cookie"
    ) {
        state.hide_line = true;
        return word.to_owned();
    }
    if matches!(lower.as_str(), "bearer" | "basic" | "digest") {
        state.hide_next = true;
        return word.to_owned();
    }
    if word.contains("://") {
        return "[url]".to_owned();
    }
    if looks_like_path(word) {
        return "[path]".to_owned();
    }
    if let Some(position) = word.find('?') {
        return format!("{}?{REDACTED}", &word[..position]);
    }
    if let Some(position) = word.find('=') {
        let name = &lower[..position];
        if SENSITIVE_NAMES
            .iter()
            .any(|fragment| name.contains(fragment))
        {
            return format!("{}={REDACTED}", &word[..position]);
        }
    }
    if looks_like_secret(word) {
        return REDACTED.to_owned();
    }
    word.to_owned()
}

/// Removes what must not reach a log from free text: URLs and query
/// strings, absolute file system paths, `name=value` pairs of names that
/// look like credentials, the values after `Authorization:`, `Cookie:` and
/// `Bearer`, and long token-like words. The text keeps its other words and
/// its spacing.
pub(crate) fn redact(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut word = String::new();
    let mut state = State::default();
    let flush = |word: &mut String, out: &mut String, state: &mut State| {
        if !word.is_empty() {
            out.push_str(&redact_word(word, state));
            word.clear();
        }
    };
    for character in text.chars() {
        if is_separator(character) {
            flush(&mut word, &mut out, &mut state);
            if character == '\n' {
                state.hide_line = false;
                state.hide_next = false;
            }
            out.push(character);
        } else {
            word.push(character);
        }
    }
    flush(&mut word, &mut out, &mut state);
    out
}

fn now() -> String {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    format_timestamp(elapsed.as_secs(), elapsed.subsec_millis())
}

/// Formats seconds since the Unix epoch as an RFC 3339 UTC timestamp with
/// millisecond precision.
pub(crate) fn format_timestamp(seconds: u64, millis: u32) -> String {
    let days = i64::try_from(seconds / 86_400).unwrap_or(i64::MAX / 2);
    let rest = seconds % 86_400;
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{millis:03}Z",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    )
}

/// Days since 1970-01-01 to a proleptic Gregorian date (Hinnant's algorithm).
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    /// A writer that keeps everything in memory.
    #[derive(Clone, Default)]
    struct Memory(Arc<Mutex<Vec<u8>>>);

    impl Write for Memory {
        fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buffer);
            Ok(buffer.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl Memory {
        fn text(&self) -> String {
            String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
        }
    }

    fn logger() -> (JsonLogger, Memory) {
        let memory = Memory::default();
        (JsonLogger::new(Box::new(memory.clone())), memory)
    }

    fn entry(path: &str, status: u16) -> RequestLogEntry {
        RequestLogEntry {
            request_id: "abc-1".into(),
            method: "GET".into(),
            path: path.into(),
            status,
            duration: Duration::from_millis(12),
        }
    }

    #[test]
    fn timestamps_are_rfc3339_utc() {
        assert_eq!(format_timestamp(0, 0), "1970-01-01T00:00:00.000Z");
        assert_eq!(
            format_timestamp(1_700_000_000, 5),
            "2023-11-14T22:13:20.005Z"
        );
        assert_eq!(format_timestamp(951_782_400, 0), "2000-02-29T00:00:00.000Z");
        assert_eq!(
            format_timestamp(4_102_444_799, 999),
            "2099-12-31T23:59:59.999Z"
        );
    }

    #[test]
    fn request_entries_are_one_json_line() {
        let (logger, memory) = logger();
        logger.request(&entry("/api/v1/items/{id}", 200));
        logger.request(&entry("/api/v1/items/{id}?token=SECRET-QUERY", 404));
        logger.request(&entry("(unmatched)", 503));
        let text = memory.text();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 3);
        let first: Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(first["level"], "info");
        assert_eq!(first["message"], "request");
        assert_eq!(first["request_id"], "abc-1");
        assert_eq!(first["method"], "GET");
        assert_eq!(first["path"], "/api/v1/items/{id}");
        assert_eq!(first["status"], 200);
        assert_eq!(first["duration_ms"], 12);
        assert!(first["ts"].as_str().unwrap().ends_with('Z'));
        let second: Value = serde_json::from_str(lines[1]).unwrap();
        assert_eq!(second["level"], "warn");
        assert_eq!(second["path"], "/api/v1/items/{id}");
        let third: Value = serde_json::from_str(lines[2]).unwrap();
        assert_eq!(third["level"], "error");
        assert!(!text.contains("SECRET-QUERY"));
    }

    #[test]
    fn synthetic_secrets_never_reach_the_output() {
        let (logger, memory) = logger();
        let mut fields = Map::new();
        fields.insert(
            "detail".into(),
            Value::String("GET /download?token=SECRET-QUERY failed".into()),
        );
        fields.insert(
            "header".into(),
            Value::String("Authorization: Bearer SECRET-BEARER".into()),
        );
        fields.insert(
            "cookie".into(),
            Value::String("Cookie: session=SECRET-COOKIE; theme=dark".into()),
        );
        fields.insert(
            "nested".into(),
            serde_json::json!({ "list": ["password=SECRET-PASSWORD"] }),
        );
        logger.log(Level::Warn, "request failed", Some("abc-2"), fields);
        logger.error("cannot open /home/someone/.config/impluvium/SECRET-FILE.key: denied");
        logger.error("fetch https://user:SECRET-URL@host.example/path?x=SECRET-URLQ failed");
        logger.error("jwt eyJhbGciOiJIUzI1NiJ9.SECRETPAYLOAD0123456789.signature0123 seen");
        let text = memory.text();
        for secret in [
            "SECRET-QUERY",
            "SECRET-BEARER",
            "SECRET-COOKIE",
            "SECRET-PASSWORD",
            "SECRET-FILE",
            "/home/someone",
            "SECRET-URL",
            "SECRET-URLQ",
            "SECRETPAYLOAD",
        ] {
            assert!(!text.contains(secret), "{secret} in {text}");
        }
        assert!(
            text.lines()
                .all(|line| serde_json::from_str::<Value>(line).is_ok())
        );
        assert!(text.contains("request failed"));
    }

    #[test]
    fn reserved_names_cannot_be_overridden_by_fields() {
        let (logger, memory) = logger();
        let mut fields = Map::new();
        fields.insert("level".into(), Value::String("debug".into()));
        fields.insert("ts".into(), Value::String("never".into()));
        fields.insert("message".into(), Value::String("other".into()));
        logger.log(Level::Info, "real", Some("abc-3"), fields);
        let value: Value = serde_json::from_str(memory.text().trim()).unwrap();
        assert_eq!(value["level"], "info");
        assert_eq!(value["message"], "real");
        assert_ne!(value["ts"], "never");
    }

    #[test]
    fn redact_keeps_harmless_text() {
        assert_eq!(
            redact("listening on port 8080, 3 workers"),
            "listening on port 8080, 3 workers"
        );
        assert_eq!(redact("two\nlines"), "two\nlines");
        assert_eq!(redact("key=value"), "key=[redacted]");
        assert_eq!(redact("size=10"), "size=10");
    }

    #[test]
    fn credential_lines_end_at_the_line_break() {
        assert_eq!(
            redact("Cookie: a=b c\nnext line"),
            "Cookie: [redacted] [redacted]\nnext line"
        );
    }

    #[test]
    fn the_panic_record_has_the_location_and_no_payload() {
        let (logger, memory) = logger();
        logger.panic_at(Some(("server/crates/example/src/lib.rs", 10, 5)));
        logger.panic_at(None);
        let text = memory.text();
        let first: Value = serde_json::from_str(text.lines().next().unwrap()).unwrap();
        assert_eq!(first["message"], "panic");
        assert_eq!(first["level"], "error");
        assert_eq!(first["file"], "server/crates/example/src/lib.rs");
        assert_eq!(first["line"], 10);
        assert_eq!(first["column"], 5);
        assert_eq!(text.lines().count(), 2);
    }
}
