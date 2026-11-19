// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! The check of the emoji picture directory.
//!
//! The directory holds SVG files named `<code point>.svg`, as the emoji package of the web
//! application ships them. The check runs before the server listens and refuses anything that is
//! not exactly that: a subdirectory, a link, a special file, a name that is not a lowercase code
//! point name with the suffix `.svg`, a file that is too large or does not look like an SVG
//! document, or more files or bytes than the fixed limits allow. All files are read into memory;
//! a request never touches the directory.

use std::fmt;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::Path;

use server_http::emoji::{EmojiFiles, is_codepoint};

/// Largest single picture.
const MAX_FILE_BYTES: u64 = 256 * 1024;
/// Largest total size of all pictures.
const MAX_TOTAL_BYTES: u64 = 64 * 1024 * 1024;
/// Largest number of pictures.
const MAX_FILES: usize = 20_000;
const SUFFIX: &str = ".svg";

/// Why a directory is not a usable set of emoji pictures. Names are relative to the directory.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum EmojiDirError {
    /// The directory does not exist or is not a directory.
    NotADirectory,
    /// An entry is a link, a subdirectory or a special file.
    NotRegularFile(String),
    /// An entry name is not a code point name with the suffix `.svg`.
    BadName(String),
    /// A size or count limit was exceeded.
    TooLarge(String),
    /// A file does not look like an SVG document.
    NotSvg(String),
    /// Reading failed.
    Io(io::ErrorKind),
}

impl fmt::Display for EmojiDirError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotADirectory => f.write_str("the emoji directory does not exist"),
            Self::NotRegularFile(name) => {
                write!(
                    f,
                    "{name:?} is not a plain file (a link, directory or special file)"
                )
            }
            Self::BadName(name) => {
                write!(
                    f,
                    "{name:?} is not named like <code point>.svg in lowercase hexadecimal"
                )
            }
            Self::TooLarge(what) => write!(f, "the emoji directory exceeds a limit: {what}"),
            Self::NotSvg(name) => write!(f, "{name:?} does not look like an SVG document"),
            Self::Io(kind) => write!(f, "the emoji directory could not be read: {kind}"),
        }
    }
}

impl std::error::Error for EmojiDirError {}

impl From<io::Error> for EmojiDirError {
    fn from(error: io::Error) -> Self {
        Self::Io(error.kind())
    }
}

/// Opens a plain file without following a link and returns it with its size.
fn open_plain(path: &Path, name: &str) -> Result<(File, u64), EmojiDirError> {
    let before = fs::symlink_metadata(path)?;
    if !before.file_type().is_file() {
        return Err(EmojiDirError::NotRegularFile(name.to_owned()));
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
            Some(libc::ELOOP) => EmojiDirError::NotRegularFile(name.to_owned()),
            _ => EmojiDirError::Io(error.kind()),
        })?;
    let meta = file.metadata()?;
    if !meta.is_file() {
        return Err(EmojiDirError::NotRegularFile(name.to_owned()));
    }
    if meta.len() > MAX_FILE_BYTES {
        return Err(EmojiDirError::TooLarge(format!("size of {name:?}")));
    }
    Ok((file, meta.len()))
}

/// Whether the bytes look like an SVG document: UTF-8 text with an `<svg` element start.
fn looks_like_svg(content: &[u8]) -> bool {
    std::str::from_utf8(content).is_ok_and(|text| text.contains("<svg"))
}

/// Checks a directory of emoji pictures and holds its files in memory. The directory itself
/// has to be a directory; it may be empty.
pub(crate) fn load(dir: &Path) -> Result<EmojiFiles, EmojiDirError> {
    if !fs::metadata(dir).is_ok_and(|meta| meta.is_dir()) {
        return Err(EmojiDirError::NotADirectory);
    }
    let mut entries = Vec::new();
    let mut total: u64 = 0;
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let file_name = entry.file_name();
        let Some(name) = file_name.to_str() else {
            return Err(EmojiDirError::BadName(
                file_name.to_string_lossy().into_owned(),
            ));
        };
        if entries.len() >= MAX_FILES {
            return Err(EmojiDirError::TooLarge("number of files".to_owned()));
        }
        let stem = name.strip_suffix(SUFFIX).filter(|stem| is_codepoint(stem));
        let Some(stem) = stem else {
            return Err(EmojiDirError::BadName(name.to_owned()));
        };
        let (file, size) = open_plain(&entry.path(), name)?;
        if total.saturating_add(size) > MAX_TOTAL_BYTES {
            return Err(EmojiDirError::TooLarge("total size".to_owned()));
        }
        let mut content = Vec::with_capacity(usize::try_from(size).unwrap_or(0));
        file.take(MAX_FILE_BYTES + 1).read_to_end(&mut content)?;
        if u64::try_from(content.len()).is_ok_and(|read| read > MAX_FILE_BYTES) {
            return Err(EmojiDirError::TooLarge(format!("size of {name:?}")));
        }
        // Count what was read, not what the directory entry said, in case the file grew.
        total = total.saturating_add(u64::try_from(content.len()).unwrap_or(u64::MAX));
        if total > MAX_TOTAL_BYTES {
            return Err(EmojiDirError::TooLarge("total size".to_owned()));
        }
        if !looks_like_svg(&content) {
            return Err(EmojiDirError::NotSvg(name.to_owned()));
        }
        entries.push((stem.to_owned(), content));
    }
    // The names were checked above and are unique within one directory.
    EmojiFiles::new(entries).map_err(|error| EmojiDirError::BadName(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scratch::Scratch;

    const SVG: &str = "<?xml version=\"1.0\"?><svg xmlns=\"http://www.w3.org/2000/svg\"/>";

    fn write(dir: &Path, name: &str, content: &str) {
        fs::write(dir.join(name), content).unwrap();
    }

    #[test]
    fn a_directory_of_pictures_is_loaded() {
        let scratch = Scratch::new("emoji-ok");
        write(scratch.path(), "1f600.svg", SVG);
        write(scratch.path(), "1f1e9-1f1ea.svg", SVG);
        let files = load(scratch.path()).unwrap();
        assert_eq!(files.len(), 2);
        let empty = Scratch::new("emoji-empty");
        assert!(load(empty.path()).unwrap().is_empty());
    }

    #[test]
    fn a_missing_directory_is_refused() {
        let scratch = Scratch::new("emoji-missing");
        assert_eq!(
            load(&scratch.path().join("absent")).unwrap_err(),
            EmojiDirError::NotADirectory
        );
        write(scratch.path(), "file", SVG);
        assert_eq!(
            load(&scratch.path().join("file")).unwrap_err(),
            EmojiDirError::NotADirectory
        );
    }

    #[test]
    fn names_that_are_not_code_points_are_refused() {
        for name in [
            "1F600.svg",
            "1f600.png",
            "1f600",
            "readme.txt",
            "1f600.svg.gz",
            ".svg",
            "x.svg",
        ] {
            let scratch = Scratch::new("emoji-name");
            write(scratch.path(), name, SVG);
            assert!(
                matches!(load(scratch.path()), Err(EmojiDirError::BadName(_))),
                "{name}"
            );
        }
    }

    #[test]
    fn subdirectories_are_refused() {
        let scratch = Scratch::new("emoji-subdir");
        fs::create_dir(scratch.path().join("1f600.svg")).unwrap();
        assert!(matches!(
            load(scratch.path()),
            Err(EmojiDirError::NotRegularFile(_))
        ));
    }

    #[cfg(unix)]
    #[test]
    fn links_are_refused() {
        let scratch = Scratch::new("emoji-link");
        let target = Scratch::new("emoji-link-target");
        write(target.path(), "real.svg", SVG);
        std::os::unix::fs::symlink(
            target.path().join("real.svg"),
            scratch.path().join("1f600.svg"),
        )
        .unwrap();
        assert!(matches!(
            load(scratch.path()),
            Err(EmojiDirError::NotRegularFile(_))
        ));
    }

    #[test]
    fn large_and_non_svg_files_are_refused() {
        let scratch = Scratch::new("emoji-large");
        let big = format!(
            "{SVG}{}",
            " ".repeat(usize::try_from(MAX_FILE_BYTES).unwrap())
        );
        write(scratch.path(), "1f600.svg", &big);
        assert!(matches!(
            load(scratch.path()),
            Err(EmojiDirError::TooLarge(_))
        ));
        let other = Scratch::new("emoji-notsvg");
        write(other.path(), "1f600.svg", "<html></html>");
        assert!(matches!(load(other.path()), Err(EmojiDirError::NotSvg(_))));
        let binary = Scratch::new("emoji-binary");
        fs::write(binary.path().join("1f600.svg"), [0xff, 0xfe, 0x00]).unwrap();
        assert!(matches!(load(binary.path()), Err(EmojiDirError::NotSvg(_))));
    }
}
