// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! The check of a built web application directory.
//!
//! A built directory holds the files of the single-page application and the manifest
//! `csp-hashes.json` that the build writes next to them. The check runs before the server listens
//! and refuses anything that does not match the manifest. It only reads; nothing is served here.
//!
//! The rules mirror the checks of the build tooling (`tools/web-spa` and the manifest code of the
//! web server): the manifest has exactly the keys `version` (1), `files` and `scripts`; `files`
//! names every file of the directory except the manifest, with its size and the base64 SHA-256 of
//! its bytes, and must include `index.html` and `loading.css`; `scripts` has one entry per HTML
//! file with the sorted, de-duplicated hashes of its inline scripts in the form `'sha256-...'`,
//! where the browser's newline normalisation applies; an inline `<style>` element or `style`
//! attribute is refused. Beyond the build tooling, this check refuses links, special files and
//! anything beyond fixed size and count limits.
//!
//! The hashes of `index.html` become the script hashes of the content security policy.
//!
//! Not mirrored: the HTML scanner below is a small tokenizer, not a full HTML parser. It handles
//! comments, quoted attributes and the raw text of `script`, `textarea`, `title`, `noscript`,
//! `iframe` and `xmp`. The manifest was written by the build for these same bytes, so a
//! disagreement can only make the server refuse a directory, never accept a wrong one.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use impluvium_protocol::hash::Algorithm;
use serde::Deserialize;
use server_http::headers::ScriptHash;

/// The manifest file name.
pub(crate) const MANIFEST: &str = "csp-hashes.json";
/// Largest manifest file.
const MAX_MANIFEST_BYTES: u64 = 4 * 1024 * 1024;
/// Largest HTML file; the scanner reads it into memory.
const MAX_HTML_BYTES: u64 = 4 * 1024 * 1024;
/// Largest single file.
const MAX_FILE_BYTES: u64 = 256 * 1024 * 1024;
/// Largest total size of all files.
const MAX_TOTAL_BYTES: u64 = 1024 * 1024 * 1024;
/// Largest number of files.
const MAX_FILES: usize = 20_000;
/// Deepest directory nesting.
const MAX_DEPTH: usize = 12;

/// A checked web application directory.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct SpaArtifact {
    /// The hashes of the inline scripts of `index.html`, for the content security policy.
    pub(crate) script_hashes: Vec<ScriptHash>,
    /// The number of files covered by the manifest.
    pub(crate) files: usize,
}

/// Why a directory is not a usable web application. Names are relative to the directory.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum SpaArtifactError {
    /// The directory does not exist or is not a directory.
    NotADirectory,
    /// A required file is absent.
    MissingFile(String),
    /// The manifest is not valid JSON of the expected shape.
    InvalidManifest,
    /// An entry is a link or a special file.
    NotRegularFile(String),
    /// An entry name is not valid UTF-8.
    BadName,
    /// A size or count limit was exceeded.
    TooLarge(String),
    /// A file is on disk but not in the manifest.
    NotInManifest(String),
    /// A file is in the manifest but not on disk.
    MissingFromDisk(String),
    /// A file differs from its manifest entry in size or hash.
    FileMismatch(String),
    /// The inline scripts of an HTML file differ from the manifest.
    ScriptMismatch(String),
    /// An HTML file has an inline style or cannot be scanned.
    HtmlRefused(String),
    /// A script hash of the manifest is not a SHA-256 hash.
    BadScriptHash,
    /// Reading failed.
    Io(io::ErrorKind),
}

impl fmt::Display for SpaArtifactError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotADirectory => f.write_str("the web application directory does not exist"),
            Self::MissingFile(name) => write!(f, "the web application has no {name:?}"),
            Self::InvalidManifest => write!(f, "{MANIFEST} is not a valid manifest"),
            Self::NotRegularFile(name) => {
                write!(f, "{name:?} is a link or a special file, not a plain file")
            }
            Self::BadName => f.write_str("a file name in the web application is not valid UTF-8"),
            Self::TooLarge(what) => write!(f, "the web application exceeds a limit: {what}"),
            Self::NotInManifest(name) => write!(f, "{name:?} is not listed in {MANIFEST}"),
            Self::MissingFromDisk(name) => {
                write!(f, "{name:?} is listed in {MANIFEST} but does not exist")
            }
            Self::FileMismatch(name) => {
                write!(f, "{name:?} does not match its entry in {MANIFEST}")
            }
            Self::ScriptMismatch(name) => {
                write!(f, "the inline scripts of {name:?} do not match {MANIFEST}")
            }
            Self::HtmlRefused(name) => {
                write!(f, "{name:?} has an inline style or cannot be checked")
            }
            Self::BadScriptHash => write!(f, "{MANIFEST} holds a script hash that is not SHA-256"),
            Self::Io(kind) => write!(f, "the web application could not be read: {kind}"),
        }
    }
}

impl std::error::Error for SpaArtifactError {}

impl From<io::Error> for SpaArtifactError {
    fn from(error: io::Error) -> Self {
        Self::Io(error.kind())
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    version: u32,
    files: BTreeMap<String, FileEntry>,
    scripts: BTreeMap<String, Vec<String>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FileEntry {
    size: u64,
    sha256: String,
}

/// Standard base64 with padding.
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut text = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let first = u32::from(chunk[0]);
        let second = chunk.get(1).copied().map_or(0, u32::from);
        let third = chunk.get(2).copied().map_or(0, u32::from);
        let group = (first << 16) | (second << 8) | third;
        let sextet = |shift: u32| char::from(ALPHABET[((group >> shift) & 0x3f) as usize]);
        text.push(sextet(18));
        text.push(sextet(12));
        text.push(if chunk.len() > 1 { sextet(6) } else { '=' });
        text.push(if chunk.len() > 2 { sextet(0) } else { '=' });
    }
    text
}

/// Opens a plain file without following a link and returns it with its size.
fn open_plain(path: &Path, name: &str, limit: u64) -> Result<(File, u64), SpaArtifactError> {
    let before = fs::symlink_metadata(path).map_err(|error| match error.kind() {
        io::ErrorKind::NotFound => SpaArtifactError::MissingFromDisk(name.to_owned()),
        kind => SpaArtifactError::Io(kind),
    })?;
    if !before.file_type().is_file() {
        return Err(SpaArtifactError::NotRegularFile(name.to_owned()));
    }
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = options
        .open(path)
        .map_err(|error| match error.raw_os_error() {
            // Only the final component can be a link here; a swapped-in link is refused.
            #[cfg(unix)]
            Some(libc::ELOOP) => SpaArtifactError::NotRegularFile(name.to_owned()),
            _ => SpaArtifactError::Io(error.kind()),
        })?;
    let meta = file.metadata()?;
    if !meta.is_file() {
        return Err(SpaArtifactError::NotRegularFile(name.to_owned()));
    }
    if meta.len() > limit {
        return Err(SpaArtifactError::TooLarge(format!("size of {name:?}")));
    }
    Ok((file, meta.len()))
}

/// Lists the plain files below `root` by their `/`-separated relative names, refusing links and
/// special files.
fn list_files(
    root: &Path,
    max_entries: usize,
) -> Result<BTreeMap<String, PathBuf>, SpaArtifactError> {
    let mut found = BTreeMap::new();
    let mut entries = 0usize;
    let mut pending: Vec<(PathBuf, String, usize)> = vec![(root.to_path_buf(), String::new(), 0)];
    while let Some((directory, prefix, depth)) = pending.pop() {
        for entry in fs::read_dir(&directory)? {
            let entry = entry?;
            let file_name = entry.file_name();
            let Some(part) = file_name.to_str() else {
                return Err(SpaArtifactError::BadName);
            };
            let name = format!("{prefix}{part}");
            entries += 1;
            if entries > max_entries {
                return Err(SpaArtifactError::TooLarge("number of entries".to_owned()));
            }
            let kind = fs::symlink_metadata(entry.path())?.file_type();
            if kind.is_symlink() {
                return Err(SpaArtifactError::NotRegularFile(name));
            }
            if kind.is_dir() {
                if depth + 1 > MAX_DEPTH {
                    return Err(SpaArtifactError::TooLarge("directory depth".to_owned()));
                }
                pending.push((entry.path(), format!("{name}/"), depth + 1));
            } else if kind.is_file() {
                if prefix.is_empty() && name == MANIFEST {
                    continue;
                }
                found.insert(name, entry.path());
            } else {
                return Err(SpaArtifactError::NotRegularFile(name));
            }
        }
    }
    Ok(found)
}

/// Checks a built web application directory against its manifest.
pub(crate) fn validate_artifact(dir: &Path) -> Result<SpaArtifact, SpaArtifactError> {
    if !fs::metadata(dir).is_ok_and(|meta| meta.is_dir()) {
        return Err(SpaArtifactError::NotADirectory);
    }
    let manifest = read_manifest(dir)?;
    let disk = list_files(dir, MAX_FILES)?;
    for required in ["index.html", "loading.css"] {
        if !manifest.files.contains_key(required) {
            return Err(SpaArtifactError::InvalidManifest);
        }
        if !disk.contains_key(required) {
            return Err(SpaArtifactError::MissingFile(required.to_owned()));
        }
    }
    if let Some(name) = disk.keys().find(|name| !manifest.files.contains_key(*name)) {
        return Err(SpaArtifactError::NotInManifest(name.clone()));
    }
    let mut computed_scripts: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut total: u64 = 0;
    for (name, entry) in &manifest.files {
        let Some(path) = disk.get(name) else {
            return Err(SpaArtifactError::MissingFromDisk(name.clone()));
        };
        let is_html = name.ends_with(".html");
        let limit = if is_html {
            MAX_HTML_BYTES
        } else {
            MAX_FILE_BYTES
        };
        let (file, size) = open_plain(path, name, limit)?;
        total = total.saturating_add(size);
        if total > MAX_TOTAL_BYTES {
            return Err(SpaArtifactError::TooLarge("total size".to_owned()));
        }
        if size != entry.size {
            return Err(SpaArtifactError::FileMismatch(name.clone()));
        }
        let mut hasher = Algorithm::Sha256.hasher();
        let mut html = Vec::new();
        let mut reader = file.take(limit + 1);
        let mut buffer = [0u8; 64 * 1024];
        loop {
            let count = reader.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            hasher.update(&buffer[..count]);
            if is_html {
                html.extend_from_slice(&buffer[..count]);
            }
        }
        if base64(&hasher.finish()) != entry.sha256 {
            return Err(SpaArtifactError::FileMismatch(name.clone()));
        }
        if is_html {
            let text = String::from_utf8_lossy(&html);
            let hashes = inline_script_hashes(&text)
                .map_err(|_| SpaArtifactError::HtmlRefused(name.clone()))?;
            computed_scripts.insert(name.clone(), hashes);
        }
    }
    if manifest.scripts.len() != computed_scripts.len()
        || computed_scripts.iter().any(|(name, computed)| {
            manifest
                .scripts
                .get(name)
                .is_none_or(|listed| listed != computed)
        })
    {
        let name = computed_scripts
            .iter()
            .find(|(name, computed)| manifest.scripts.get(*name) != Some(*computed))
            .map_or_else(|| MANIFEST.to_owned(), |(name, _)| name.clone());
        return Err(SpaArtifactError::ScriptMismatch(name));
    }
    let mut script_hashes = Vec::new();
    for quoted in computed_scripts.get("index.html").into_iter().flatten() {
        let bare = quoted.trim_matches('\'');
        script_hashes.push(ScriptHash::parse(bare).map_err(|_| SpaArtifactError::BadScriptHash)?);
    }
    Ok(SpaArtifact {
        script_hashes,
        files: manifest.files.len(),
    })
}

fn read_manifest(dir: &Path) -> Result<Manifest, SpaArtifactError> {
    let (file, _) = open_plain(&dir.join(MANIFEST), MANIFEST, MAX_MANIFEST_BYTES).map_err(
        |error| match error {
            SpaArtifactError::MissingFromDisk(name) => SpaArtifactError::MissingFile(name),
            other => other,
        },
    )?;
    let mut bytes = Vec::new();
    file.take(MAX_MANIFEST_BYTES + 1).read_to_end(&mut bytes)?;
    let manifest: Manifest =
        serde_json::from_slice(&bytes).map_err(|_| SpaArtifactError::InvalidManifest)?;
    if manifest.version != 1 {
        return Err(SpaArtifactError::InvalidManifest);
    }
    Ok(manifest)
}

/// The scanner could not accept the HTML.
#[derive(Debug, PartialEq, Eq)]
struct HtmlRefused;

/// The sorted, de-duplicated hashes `'sha256-...'` of the inline scripts that need one.
fn inline_script_hashes(html: &str) -> Result<Vec<String>, HtmlRefused> {
    let text = html.replace("\r\n", "\n").replace('\r', "\n");
    let bytes = text.as_bytes();
    // ASCII lowercasing keeps every offset, so one copy serves all raw-text searches.
    let lower = text.to_ascii_lowercase();
    let mut hashes = BTreeSet::new();
    let mut at = 0;
    while let Some(offset) = text[at..].find('<') {
        let start = at + offset;
        let rest = &text[start..];
        if rest.starts_with("<!--") {
            match text[start + 4..].find("-->") {
                Some(end) => at = start + 4 + end + 3,
                None => break,
            }
            continue;
        }
        let next = bytes.get(start + 1).copied().unwrap_or(0);
        if !next.is_ascii_alphabetic() {
            // A closing tag, a doctype or a processing instruction runs to the next `>`;
            // anything else is a stray `<`.
            at = if matches!(next, b'/' | b'!' | b'?') {
                text[start..]
                    .find('>')
                    .map_or(text.len(), |end| start + end + 1)
            } else {
                start + 1
            };
            continue;
        }
        let (tag, attributes, after) = parse_start_tag(&text, start + 1)?;
        if tag == "style" || attributes.iter().any(|(name, _)| name == "style") {
            return Err(HtmlRefused);
        }
        at = after;
        if matches!(
            tag.as_str(),
            "script" | "textarea" | "title" | "noscript" | "iframe" | "xmp"
        ) {
            let close = format!("</{tag}");
            let Some(end) = lower[after..].find(&close) else {
                return Err(HtmlRefused);
            };
            let content = &text[after..after + end];
            if tag == "script" {
                let attribute = |wanted: &str| {
                    attributes
                        .iter()
                        .find(|(name, _)| name == wanted)
                        .map(|(_, value)| value.as_str())
                };
                if attribute("src").is_none()
                    && needs_hash(attribute("type"), attribute("language"))
                {
                    let digest = Algorithm::Sha256.digest(content.as_bytes());
                    hashes.insert(format!("'sha256-{}'", base64(&digest)));
                }
            }
            at = text[after + end..]
                .find('>')
                .map_or(text.len(), |close_end| after + end + close_end + 1);
        }
    }
    Ok(hashes.into_iter().collect())
}

/// A parsed start tag: lowercase name, attributes with lowercase names (a missing value is
/// empty) and the index after the closing `>`.
type StartTag = (String, Vec<(String, String)>, usize);

/// Parses a start tag whose name begins at `from`.
fn parse_start_tag(text: &str, from: usize) -> Result<StartTag, HtmlRefused> {
    let bytes = text.as_bytes();
    let mut at = from;
    while at < bytes.len() && (bytes[at].is_ascii_alphanumeric() || bytes[at] == b'-') {
        at += 1;
    }
    let tag = text[from..at].to_ascii_lowercase();
    let mut attributes = Vec::new();
    loop {
        while at < bytes.len() && (bytes[at].is_ascii_whitespace() || bytes[at] == b'/') {
            at += 1;
        }
        match bytes.get(at) {
            None => return Err(HtmlRefused),
            Some(b'>') => return Ok((tag, attributes, at + 1)),
            Some(_) => {}
        }
        let name_start = at;
        while at < bytes.len()
            && !bytes[at].is_ascii_whitespace()
            && !matches!(bytes[at], b'=' | b'>' | b'/')
        {
            at += 1;
        }
        if at == name_start {
            // A lone `=`: skip it, as browsers do.
            at += 1;
            continue;
        }
        let name = text[name_start..at].to_ascii_lowercase();
        while at < bytes.len() && bytes[at].is_ascii_whitespace() {
            at += 1;
        }
        let mut value = String::new();
        if bytes.get(at) == Some(&b'=') {
            at += 1;
            while at < bytes.len() && bytes[at].is_ascii_whitespace() {
                at += 1;
            }
            match bytes.get(at) {
                Some(&quote) if quote == b'"' || quote == b'\'' => {
                    let Some(length) = text[at + 1..].find(char::from(quote)) else {
                        return Err(HtmlRefused);
                    };
                    value = text[at + 1..at + 1 + length].to_owned();
                    at += length + 2;
                }
                _ => {
                    let value_start = at;
                    while at < bytes.len() && !bytes[at].is_ascii_whitespace() && bytes[at] != b'>'
                    {
                        at += 1;
                    }
                    value = text[value_start..at].to_owned();
                }
            }
        }
        attributes.push((name, value));
    }
}

/// Whether the browser needs a hash for an inline script of this `type` and `language`:
/// classic JavaScript, module and import map scripts do; data blocks such as JSON do not.
fn needs_hash(script_type: Option<&str>, language: Option<&str>) -> bool {
    let trim = |value: &str| {
        value
            .trim_matches(['\t', '\n', '\u{c}', '\r', ' '])
            .to_owned()
    };
    let value = match (script_type, language) {
        (None, Some(language)) if !language.is_empty() => trim(&format!("text/{language}")),
        _ => trim(script_type.unwrap_or("")),
    }
    .to_ascii_lowercase();
    matches!(
        value.as_str(),
        "" | "module"
            | "importmap"
            | "application/javascript"
            | "application/x-javascript"
            | "application/ecmascript"
            | "application/x-ecmascript"
            | "text/javascript"
            | "text/x-javascript"
            | "text/ecmascript"
            | "text/x-ecmascript"
            | "text/javascript1.0"
            | "text/javascript1.1"
            | "text/javascript1.2"
            | "text/javascript1.3"
            | "text/javascript1.4"
            | "text/javascript1.5"
            | "text/jscript"
            | "text/livescript"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scratch::Scratch;

    const EMPTY_HASH: &str = "47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU=";
    const ABC_HASH: &str = "ungWv48Bz+pBQUDeXa4iI7ADYaOWF3qctBD/YfIAFa0=";

    fn digest(bytes: &[u8]) -> String {
        base64(&Algorithm::Sha256.digest(bytes))
    }

    const INDEX: &str = "<!doctype html>\n<html><head><title>app</title>\n\
        <script>window.boot = 1;</script>\n\
        <script type=\"application/json\">{\"a\":1}</script>\n\
        <script src=\"/_nuxt/app.js\" type=\"module\"></script>\n\
        <!-- <script>ignored()</script> -->\n\
        </head><body></body></html>\n";

    /// Writes a good directory and returns the manifest as a mutable value.
    fn good(scratch: &Scratch) -> serde_json::Value {
        let dir = scratch.path();
        fs::create_dir_all(dir.join("_nuxt")).unwrap();
        fs::write(dir.join("index.html"), INDEX).unwrap();
        fs::write(dir.join("loading.css"), "body{}").unwrap();
        fs::write(dir.join("_nuxt/app.js"), "console.log(1)").unwrap();
        let entry =
            |bytes: &[u8]| serde_json::json!({"size": bytes.len(), "sha256": digest(bytes)});
        serde_json::json!({
            "version": 1,
            "files": {
                "index.html": entry(INDEX.as_bytes()),
                "loading.css": entry(b"body{}"),
                "_nuxt/app.js": entry(b"console.log(1)"),
            },
            "scripts": { "index.html": [format!("'sha256-{}'", digest(b"window.boot = 1;"))] },
        })
    }

    fn write_manifest(scratch: &Scratch, manifest: &serde_json::Value) {
        fs::write(scratch.path().join(MANIFEST), manifest.to_string()).unwrap();
    }

    #[test]
    fn hashing_and_base64_match_known_values() {
        assert_eq!(digest(b""), EMPTY_HASH);
        assert_eq!(digest(b"abc"), ABC_HASH);
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
    }

    #[test]
    fn a_good_directory_passes_and_yields_the_script_hash() {
        let scratch = Scratch::new("spa-good");
        let manifest = good(&scratch);
        write_manifest(&scratch, &manifest);
        let artifact = validate_artifact(scratch.path()).unwrap();
        assert_eq!(artifact.files, 3);
        assert_eq!(artifact.script_hashes.len(), 1);
    }

    #[test]
    fn missing_and_extra_files_are_refused() {
        let scratch = Scratch::new("spa-missing");
        assert_eq!(
            validate_artifact(&scratch.path().join("absent")),
            Err(SpaArtifactError::NotADirectory)
        );
        assert_eq!(
            validate_artifact(scratch.path()).unwrap_err(),
            SpaArtifactError::MissingFile(MANIFEST.to_owned())
        );
        let manifest = good(&scratch);
        write_manifest(&scratch, &manifest);
        fs::write(scratch.path().join("extra.txt"), "x").unwrap();
        assert_eq!(
            validate_artifact(scratch.path()).unwrap_err(),
            SpaArtifactError::NotInManifest("extra.txt".to_owned())
        );
        fs::remove_file(scratch.path().join("extra.txt")).unwrap();
        fs::remove_file(scratch.path().join("loading.css")).unwrap();
        assert_eq!(
            validate_artifact(scratch.path()).unwrap_err(),
            SpaArtifactError::MissingFile("loading.css".to_owned())
        );
        fs::write(scratch.path().join("loading.css"), "body{}").unwrap();
        fs::remove_file(scratch.path().join("_nuxt/app.js")).unwrap();
        assert_eq!(
            validate_artifact(scratch.path()).unwrap_err(),
            SpaArtifactError::MissingFromDisk("_nuxt/app.js".to_owned())
        );
    }

    #[test]
    fn changed_files_and_script_hashes_are_refused() {
        let scratch = Scratch::new("spa-mismatch");
        let mut manifest = good(&scratch);
        write_manifest(&scratch, &manifest);
        fs::write(scratch.path().join("_nuxt/app.js"), "console.log(2)").unwrap();
        assert_eq!(
            validate_artifact(scratch.path()).unwrap_err(),
            SpaArtifactError::FileMismatch("_nuxt/app.js".to_owned())
        );
        fs::write(scratch.path().join("_nuxt/app.js"), "console.log(1)").unwrap();
        manifest["scripts"]["index.html"] =
            serde_json::json!([format!("'sha256-{}'", digest(b"window.boot = 2;"))]);
        write_manifest(&scratch, &manifest);
        assert_eq!(
            validate_artifact(scratch.path()).unwrap_err(),
            SpaArtifactError::ScriptMismatch("index.html".to_owned())
        );
        manifest["scripts"] = serde_json::json!({});
        write_manifest(&scratch, &manifest);
        assert!(matches!(
            validate_artifact(scratch.path()),
            Err(SpaArtifactError::ScriptMismatch(_))
        ));
    }

    #[test]
    fn a_wrong_manifest_shape_or_an_oversized_manifest_is_refused() {
        let scratch = Scratch::new("spa-shape");
        let mut manifest = good(&scratch);
        manifest["version"] = serde_json::json!(2);
        write_manifest(&scratch, &manifest);
        assert_eq!(
            validate_artifact(scratch.path()).unwrap_err(),
            SpaArtifactError::InvalidManifest
        );
        let mut manifest = good(&scratch);
        manifest["extra"] = serde_json::json!(true);
        write_manifest(&scratch, &manifest);
        assert_eq!(
            validate_artifact(scratch.path()).unwrap_err(),
            SpaArtifactError::InvalidManifest
        );
        let huge = vec![b' '; usize::try_from(MAX_MANIFEST_BYTES).unwrap() + 1];
        fs::write(scratch.path().join(MANIFEST), huge).unwrap();
        assert!(matches!(
            validate_artifact(scratch.path()),
            Err(SpaArtifactError::TooLarge(_))
        ));
    }

    #[cfg(unix)]
    #[test]
    fn links_and_special_files_are_refused() {
        use std::os::unix::fs::symlink;
        let scratch = Scratch::new("spa-link");
        let manifest = good(&scratch);
        write_manifest(&scratch, &manifest);
        fs::remove_file(scratch.path().join("loading.css")).unwrap();
        fs::write(scratch.path().join("elsewhere.css"), "body{}").unwrap();
        symlink(
            scratch.path().join("elsewhere.css"),
            scratch.path().join("loading.css"),
        )
        .unwrap();
        assert_eq!(
            validate_artifact(scratch.path()).unwrap_err(),
            SpaArtifactError::NotRegularFile("loading.css".to_owned())
        );
        let linked_manifest = Scratch::new("spa-link-manifest");
        let source = good(&linked_manifest);
        let target = linked_manifest.path().join("real.json");
        fs::write(&target, source.to_string()).unwrap();
        symlink(&target, linked_manifest.path().join(MANIFEST)).unwrap();
        assert_eq!(
            validate_artifact(linked_manifest.path()).unwrap_err(),
            SpaArtifactError::NotRegularFile(MANIFEST.to_owned())
        );
    }

    #[test]
    fn the_scanner_hashes_only_inline_scripts_that_need_one() {
        let hashes = |html: &str| inline_script_hashes(html).unwrap();
        let abc = format!("'sha256-{ABC_HASH}'");
        assert_eq!(hashes("<script>abc</script>"), [abc.as_str()]);
        assert_eq!(hashes("<SCRIPT TYPE=module>abc</SCRIPT>"), [abc.as_str()]);
        assert_eq!(
            hashes("<script type=importmap>abc</script>"),
            [abc.as_str()]
        );
        assert_eq!(
            hashes("<script language=javascript>abc</script>"),
            [abc.as_str()]
        );
        assert!(hashes("<script src=\"a.js\">abc</script>").is_empty());
        assert!(hashes("<script type=\"application/json\">abc</script>").is_empty());
        assert!(hashes("<!-- <script>abc</script> -->").is_empty());
        assert!(hashes("<textarea><script>abc</script></textarea>").is_empty());
        assert_eq!(
            hashes("<script>abc</script><script>abc</script>"),
            [abc.as_str()]
        );
        // The browser hashes the text after newline normalisation.
        assert_eq!(
            hashes("<script>a\r\nb\rc</script>"),
            hashes("<script>a\nb\nc</script>")
        );
        assert_eq!(
            hashes("<div data-x=\"<script>abc</script>\"></div>"),
            Vec::<String>::new()
        );
    }

    #[test]
    fn a_tree_of_only_directories_over_the_entry_limit_is_refused() {
        let scratch = Scratch::new("spa-dirs");
        for index in 0..6 {
            fs::create_dir(scratch.path().join(format!("d{index}"))).unwrap();
        }
        assert!(list_files(scratch.path(), 6).unwrap().is_empty());
        assert!(matches!(
            list_files(scratch.path(), 5),
            Err(SpaArtifactError::TooLarge(_))
        ));
    }

    #[test]
    fn inline_styles_and_unterminated_scripts_are_refused() {
        for html in [
            "<style>a{}</style>",
            "<p STYLE=\"color:red\">x</p>",
            "<script>abc",
            "<div class=\"x",
        ] {
            assert_eq!(inline_script_hashes(html), Err(HtmlRefused), "{html}");
        }
    }
}
