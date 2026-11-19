// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! The identity of a server instance on the data directory side: the instance identifier and the
//! marker file that binds a data directory to one database.
//!
//! # The marker
//!
//! `<data>/.impluvium-instance` is a small text file of exactly three lines:
//!
//! ```text
//! impluvium-instance-marker 1
//! id 0b3c1e9a-0000-4000-8000-000000000001
//! layout 1
//! ```
//!
//! It is written once, never replaced. The content goes to a private temporary file in the same
//! directory, is flushed, and is then published with a hard link, which fails when the marker
//! already exists; the temporary name is removed and the directory is flushed. A crash can leave a
//! stale `.impluvium-instance.tmp-*` file, which is never read as a marker. Reading accepts only a
//! regular file (no link), at most [`MAX_MARKER_BYTES`] long, in exactly this form.

use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};

use crate::backup::DATA_MARKER_FILE;

/// The layout version of the data (database tables and data directory) this build writes and reads.
pub const LAYOUT_VERSION: i32 = 1;

/// Largest accepted marker file.
pub const MAX_MARKER_BYTES: u64 = 1024;

const MAGIC: &str = "impluvium-instance-marker 1";

/// The identifier of an instance: a UUID in its canonical lowercase text form.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct InstanceId(String);

impl InstanceId {
    /// Accepts the canonical form `8-4-4-4-12` lowercase hexadecimal digits and nothing else.
    pub fn parse(text: &str) -> Option<Self> {
        let bytes = text.as_bytes();
        if bytes.len() != 36 {
            return None;
        }
        let valid = bytes.iter().enumerate().all(|(index, byte)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                *byte == b'-'
            } else {
                matches!(byte, b'0'..=b'9' | b'a'..=b'f')
            }
        });
        valid.then(|| Self(text.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for InstanceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// The content of the marker file.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DataMarker {
    pub instance_id: InstanceId,
    pub layout_version: i32,
}

/// Why the marker could not be read or written.
#[derive(Debug)]
pub enum MarkerError {
    /// A file operation failed; the kind is the only detail of the system error that is kept.
    Io {
        step: &'static str,
        kind: io::ErrorKind,
    },
    /// The data directory is not a directory.
    NotADirectory,
    /// The marker is not a regular file (a link, a directory, a device).
    NotRegular,
    /// The marker is longer than [`MAX_MARKER_BYTES`].
    TooLarge,
    /// The marker is not in the exact form this version writes.
    Invalid,
    /// A marker exists already and is never replaced.
    AlreadyExists,
}

impl fmt::Display for MarkerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { step, kind } => {
                write!(
                    f,
                    "the data directory marker failed while trying to {step}: {kind}"
                )
            }
            Self::NotADirectory => f.write_str("the data directory is not a directory"),
            Self::NotRegular => f.write_str("the data directory marker is not a regular file"),
            Self::TooLarge => f.write_str("the data directory marker is too large"),
            Self::Invalid => f.write_str("the data directory marker is not valid"),
            Self::AlreadyExists => f.write_str("the data directory marker exists already"),
        }
    }
}

impl std::error::Error for MarkerError {}

fn io_error(step: &'static str) -> impl FnOnce(io::Error) -> MarkerError {
    move |error| MarkerError::Io {
        step,
        kind: error.kind(),
    }
}

impl DataMarker {
    /// The exact bytes of the file.
    pub fn to_text(&self) -> String {
        format!(
            "{MAGIC}\nid {}\nlayout {}\n",
            self.instance_id, self.layout_version
        )
    }

    /// Parses the exact form of [`to_text`](Self::to_text).
    pub fn parse(text: &str) -> Result<Self, MarkerError> {
        let mut lines = text.split_inclusive('\n');
        if next_line(&mut lines, "")? != MAGIC {
            return Err(MarkerError::Invalid);
        }
        let instance_id =
            InstanceId::parse(next_line(&mut lines, "id ")?).ok_or(MarkerError::Invalid)?;
        let layout = next_line(&mut lines, "layout ")?;
        let canonical = !layout.is_empty()
            && layout.bytes().all(|byte| byte.is_ascii_digit())
            && !(layout.len() > 1 && layout.starts_with('0'));
        let layout_version: i32 = layout
            .parse()
            .ok()
            .filter(|value| canonical && *value >= 1)
            .ok_or(MarkerError::Invalid)?;
        if lines.next().is_some() {
            return Err(MarkerError::Invalid);
        }
        Ok(Self {
            instance_id,
            layout_version,
        })
    }
}

fn next_line<'a>(
    lines: &mut impl Iterator<Item = &'a str>,
    prefix: &str,
) -> Result<&'a str, MarkerError> {
    lines
        .next()
        .and_then(|line| line.strip_suffix('\n'))
        .and_then(|line| line.strip_prefix(prefix))
        .ok_or(MarkerError::Invalid)
}

/// Reads the marker of a data directory, or `None` when there is none.
pub fn read_marker(data_dir: &Path) -> Result<Option<DataMarker>, MarkerError> {
    let path = data_dir.join(DATA_MARKER_FILE);
    let meta = match fs::symlink_metadata(&path) {
        Ok(meta) => meta,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(io_error("inspect the marker")(error)),
    };
    if !meta.file_type().is_file() {
        return Err(MarkerError::NotRegular);
    }
    if meta.len() > MAX_MARKER_BYTES {
        return Err(MarkerError::TooLarge);
    }
    let file = File::open(&path).map_err(io_error("open the marker"))?;
    // The opened handle must be the file that was inspected: a link swapped in between is refused.
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let opened = file.metadata().map_err(io_error("inspect the marker"))?;
        if !opened.is_file() || opened.dev() != meta.dev() || opened.ino() != meta.ino() {
            return Err(MarkerError::NotRegular);
        }
    }
    let mut bytes = Vec::new();
    file.take(MAX_MARKER_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(io_error("read the marker"))?;
    if bytes.len() as u64 > MAX_MARKER_BYTES {
        return Err(MarkerError::TooLarge);
    }
    let text = std::str::from_utf8(&bytes).map_err(|_| MarkerError::Invalid)?;
    DataMarker::parse(text).map(Some)
}

static TEMP_COUNTER: AtomicU32 = AtomicU32::new(0);

/// Sixteen hexadecimal digits for the temporary name: eight random bytes. When the system source
/// is unavailable, the time, the process and a counter stand in.
fn temporary_suffix() -> String {
    let mut bytes = [0u8; 8];
    let random = File::open("/dev/urandom").and_then(|mut source| source.read_exact(&mut bytes));
    if random.is_err() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_nanos() as u64);
        let mixed = nanos
            ^ u64::from(std::process::id()).rotate_left(32)
            ^ u64::from(TEMP_COUNTER.fetch_add(1, Ordering::SeqCst));
        bytes = mixed.to_le_bytes();
    }
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Writes the marker of a data directory. Fails with [`MarkerError::AlreadyExists`] when one
/// exists; an existing marker is never replaced.
pub fn write_marker(data_dir: &Path, marker: &DataMarker) -> Result<(), MarkerError> {
    match fs::metadata(data_dir) {
        Ok(meta) if meta.is_dir() => {}
        Ok(_) => return Err(MarkerError::NotADirectory),
        Err(error) => return Err(io_error("inspect the data directory")(error)),
    }
    let target = data_dir.join(DATA_MARKER_FILE);
    if fs::symlink_metadata(&target).is_ok() {
        return Err(MarkerError::AlreadyExists);
    }
    let temporary = data_dir.join(format!("{DATA_MARKER_FILE}.tmp-{}", temporary_suffix()));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&temporary)
        .map_err(io_error("create the temporary marker"))?;
    let written = file
        .write_all(marker.to_text().as_bytes())
        .and_then(|()| file.sync_all());
    drop(file);
    let published =
        written
            .map_err(io_error("write the marker"))
            .and_then(|()| match fs::hard_link(&temporary, &target) {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                    Err(MarkerError::AlreadyExists)
                }
                Err(error) => Err(io_error("publish the marker")(error)),
            });
    let _ = fs::remove_file(&temporary);
    published?;
    sync_dir(data_dir).map_err(io_error("flush the data directory"))
}

fn sync_dir(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        File::open(path)?.sync_all()
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    pub struct Scratch(pub PathBuf);

    impl Scratch {
        pub fn new() -> Self {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |elapsed| elapsed.as_nanos());
            let path = std::env::temp_dir().join(format!(
                "impluvium-marker-test-{}-{nanos:x}-{}",
                std::process::id(),
                COUNTER.fetch_add(1, Ordering::SeqCst)
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::Scratch;
    use super::*;

    const ID: &str = "0b3c1e9a-0000-4000-8000-000000000001";

    fn marker() -> DataMarker {
        DataMarker {
            instance_id: InstanceId::parse(ID).unwrap(),
            layout_version: LAYOUT_VERSION,
        }
    }

    #[test]
    fn instance_ids_are_canonical_lowercase_uuids_only() {
        assert!(InstanceId::parse(ID).is_some());
        for bad in [
            "",
            "0B3C1E9A-0000-4000-8000-000000000001",
            "0b3c1e9a00004000800000000000000001",
            "0b3c1e9a-0000-4000-8000-00000000000g",
            "0b3c1e9a-0000-4000-8000-0000000000012",
            "{0b3c1e9a-0000-4000-8000-000000000001}",
        ] {
            assert!(InstanceId::parse(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn the_marker_text_round_trips_and_nothing_else_parses() {
        let text = marker().to_text();
        assert_eq!(DataMarker::parse(&text).unwrap(), marker());
        let bad: Vec<String> = vec![
            String::new(),
            "garbage".into(),
            text.replace("marker 1", "marker 2"),
            text.replace("id ", "ID "),
            text.replace("layout 1", "layout 01"),
            text.replace("layout 1", "layout 0"),
            text.replace("layout 1", "layout -1"),
            text.replace("layout 1", "layout +1"),
            text.replace("layout 1", "layout 99999999999"),
            format!("{text}extra\n"),
            text[..text.len() - 1].to_owned(),
            text.replace('\n', "\r\n"),
        ];
        for bad in &bad {
            assert!(DataMarker::parse(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn temporary_names_are_random_hexadecimal() {
        let first = temporary_suffix();
        let second = temporary_suffix();
        assert_ne!(first, second);
        for name in [&first, &second] {
            assert_eq!(name.len(), 16);
            assert!(name.bytes().all(|byte| byte.is_ascii_hexdigit()), "{name}");
        }
    }

    #[test]
    fn a_missing_marker_reads_as_none() {
        let scratch = Scratch::new();
        assert_eq!(read_marker(&scratch.0).unwrap(), None);
    }

    #[test]
    fn writing_is_private_exclusive_and_leaves_nothing_behind() {
        use std::os::unix::fs::PermissionsExt;
        let scratch = Scratch::new();
        write_marker(&scratch.0, &marker()).unwrap();
        let path = scratch.0.join(DATA_MARKER_FILE);
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(read_marker(&scratch.0).unwrap(), Some(marker()));
        let names: Vec<_> = fs::read_dir(&scratch.0)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(names, [DATA_MARKER_FILE]);

        let other = DataMarker {
            instance_id: InstanceId::parse("0b3c1e9a-0000-4000-8000-000000000002").unwrap(),
            layout_version: 1,
        };
        assert!(matches!(
            write_marker(&scratch.0, &other),
            Err(MarkerError::AlreadyExists)
        ));
        assert_eq!(read_marker(&scratch.0).unwrap(), Some(marker()));
    }

    #[test]
    fn a_symlink_is_refused_for_reading_and_for_writing() {
        let scratch = Scratch::new();
        let real = scratch.0.join("real");
        fs::write(&real, marker().to_text()).unwrap();
        std::os::unix::fs::symlink(&real, scratch.0.join(DATA_MARKER_FILE)).unwrap();
        assert!(matches!(
            read_marker(&scratch.0),
            Err(MarkerError::NotRegular)
        ));
        assert!(matches!(
            write_marker(&scratch.0, &marker()),
            Err(MarkerError::AlreadyExists)
        ));
        assert_eq!(fs::read_to_string(&real).unwrap(), marker().to_text());
    }

    #[test]
    fn oversized_garbage_and_directories_are_refused() {
        let scratch = Scratch::new();
        let path = scratch.0.join(DATA_MARKER_FILE);
        fs::write(&path, vec![b'a'; MAX_MARKER_BYTES as usize + 1]).unwrap();
        assert!(matches!(
            read_marker(&scratch.0),
            Err(MarkerError::TooLarge)
        ));
        fs::write(&path, b"\xff\xfe").unwrap();
        assert!(matches!(read_marker(&scratch.0), Err(MarkerError::Invalid)));
        fs::remove_file(&path).unwrap();
        fs::create_dir(&path).unwrap();
        assert!(matches!(
            read_marker(&scratch.0),
            Err(MarkerError::NotRegular)
        ));
    }

    #[test]
    fn a_missing_data_directory_is_an_error_and_a_stale_temporary_file_is_ignored() {
        let scratch = Scratch::new();
        assert!(write_marker(&scratch.0.join("absent"), &marker()).is_err());
        fs::write(
            scratch.0.join(format!("{DATA_MARKER_FILE}.tmp-1-1")),
            marker().to_text(),
        )
        .unwrap();
        assert_eq!(read_marker(&scratch.0).unwrap(), None);
        write_marker(&scratch.0, &marker()).unwrap();
    }
}
