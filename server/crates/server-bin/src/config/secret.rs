// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Reading a secret from the file named by a `_FILE` variable.
//!
//! Messages name the problem and never the path or the content.

use std::fs::File;
use std::io::{ErrorKind, Read};
use std::path::Path;

/// Largest accepted secret file in bytes.
pub const MAX_BYTES: u64 = 64 * 1024;

pub fn read_file(path: &str) -> Result<String, String> {
    if path.is_empty() || path.contains('\0') {
        return Err("must name a file".to_owned());
    }
    // Opening a FIFO or a device can block; check the type first. Symlinks to
    // regular files stay allowed, as container platforms mount secrets that way.
    precheck(Path::new(path))?;
    let mut file = match File::open(Path::new(path)) {
        Ok(file) => file,
        Err(error) => return Err(unreadable(error.kind())),
    };
    let metadata = match file.metadata() {
        Ok(metadata) => metadata,
        Err(error) => return Err(unreadable(error.kind())),
    };
    if !metadata.is_file() {
        return Err("must name a regular file".to_owned());
    }
    if metadata.len() > MAX_BYTES {
        return Err(too_large());
    }
    // The file may grow after the size check: never read beyond the limit.
    let mut bytes = Vec::new();
    if let Err(error) = (&mut file).take(MAX_BYTES + 1).read_to_end(&mut bytes) {
        return Err(unreadable(error.kind()));
    }
    if bytes.len() as u64 > MAX_BYTES {
        return Err(too_large());
    }
    let Ok(text) = String::from_utf8(bytes) else {
        return Err("names a file that is not valid UTF-8".to_owned());
    };
    // One trailing line break is the usual way files are written.
    let trimmed = match text.strip_suffix('\n') {
        Some(rest) => rest.strip_suffix('\r').unwrap_or(rest),
        None => text.as_str(),
    };
    if trimmed.is_empty() {
        return Err("names an empty file".to_owned());
    }
    Ok(trimmed.to_owned())
}

/// Rejects everything that is not a regular file, without opening it.
fn precheck(path: &Path) -> Result<(), String> {
    match std::fs::metadata(path) {
        Ok(metadata) if metadata.is_file() => Ok(()),
        Ok(_) => Err("must name a regular file".to_owned()),
        Err(error) => Err(unreadable(error.kind())),
    }
}

fn too_large() -> String {
    format!("names a file larger than {MAX_BYTES} bytes")
}

fn unreadable(kind: ErrorKind) -> String {
    match kind {
        ErrorKind::NotFound => "names a file that does not exist".to_owned(),
        ErrorKind::PermissionDenied => "names a file that is not readable".to_owned(),
        _ => "names a file that cannot be read".to_owned(),
    }
}

/// A scratch directory for tests, removed when dropped.
#[cfg(test)]
pub struct TempDir(std::path::PathBuf);

#[cfg(test)]
impl TempDir {
    pub fn new(label: &str) -> Self {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let number = COUNTER.fetch_add(1, Ordering::Relaxed);
        let name = format!("impluvium-config-{}-{label}-{number}", std::process::id());
        let path = std::env::temp_dir().join(name);
        std::fs::create_dir_all(&path).expect("create the test directory");
        Self(path)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }

    /// Writes a file and returns its path as text.
    pub fn write(&self, name: &str, content: &[u8]) -> String {
        let path = self.0.join(name);
        std::fs::write(&path, content).expect("write the test file");
        path.to_string_lossy().into_owned()
    }
}

#[cfg(test)]
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_value_and_drops_one_trailing_line_break() {
        let dir = TempDir::new("secret-ok");
        assert_eq!(read_file(&dir.write("plain", b"abc")).unwrap(), "abc");
        assert_eq!(read_file(&dir.write("lf", b"abc\n")).unwrap(), "abc");
        assert_eq!(read_file(&dir.write("crlf", b"abc\r\n")).unwrap(), "abc");
        assert_eq!(read_file(&dir.write("two", b"abc\n\n")).unwrap(), "abc\n");
    }

    #[test]
    fn non_regular_files_are_rejected_before_they_are_opened() {
        let dir = TempDir::new("secret-kind");
        let directory = dir.path().to_string_lossy().into_owned();
        assert!(precheck(dir.path()).is_err());
        let expected = "must name a regular file";
        assert_eq!(read_file(&directory).unwrap_err(), expected);
        #[cfg(unix)]
        {
            let device = Path::new("/dev/null");
            let expected = "must name a regular file";
            assert_eq!(precheck(device).unwrap_err(), expected);
            assert_eq!(read_file("/dev/null").unwrap_err(), expected);
            let target = dir.write("target", b"abc\n");
            let link = dir.path().join("link");
            std::os::unix::fs::symlink(&target, &link).expect("create a symlink");
            assert_eq!(read_file(&link.to_string_lossy()).unwrap(), "abc");
        }
        assert!(precheck(&dir.path().join("missing")).is_err());
    }

    #[test]
    fn rejects_unusable_files_without_naming_path_or_content() {
        let dir = TempDir::new("secret-bad");
        let marker = "marker-content";
        let big = vec![b'x'; usize::try_from(MAX_BYTES).unwrap() + 1];
        let cases = [
            read_file(""),
            read_file("/nonexistent-impluvium-dir/secret"),
            read_file(&dir.path().to_string_lossy()),
            read_file(&dir.write("empty", b"")),
            read_file(&dir.write("newline", b"\n")),
            read_file(&dir.write("big", &big)),
            read_file(&dir.write("binary", &[0xff, 0xfe, marker.as_bytes()[0]])),
        ];
        for result in cases {
            let message = result.unwrap_err();
            assert!(
                !message.contains("impluvium-config"),
                "path leaked: {message}"
            );
            assert!(!message.contains("nonexistent"), "path leaked: {message}");
        }
        let exactly = vec![b'x'; usize::try_from(MAX_BYTES).unwrap()];
        assert!(read_file(&dir.write("exact", &exactly)).is_ok());
    }
}
