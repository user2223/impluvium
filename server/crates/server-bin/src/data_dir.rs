// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! The data directory of a running server.
//!
//! The directory must exist, belong to the user that runs the server and not be writable by its
//! group or by others. The server takes an exclusive advisory lock on `.impluvium-lock` inside it
//! and keeps it until the process ends. The database lock keeps two servers off one database; this
//! lock keeps two servers (with two different databases) off one data directory.
//!
//! The lock belongs to the open file, so the operating system drops it when the process dies, also
//! after a crash. Messages name no path.

use std::fmt;
use std::fs::{File, OpenOptions, TryLockError};
use std::io;
use std::path::Path;

/// The name of the lock file inside the data directory.
pub(crate) const LOCK_FILE: &str = ".impluvium-lock";

/// Why the data directory cannot be used.
#[derive(Debug)]
pub(crate) enum DataDirError {
    /// The directory does not exist.
    Missing,
    /// The path is not a directory.
    NotADirectory,
    /// The directory belongs to another user than the one running the server.
    WrongOwner,
    /// The group or others may write to the directory.
    WritableByOthers,
    /// The lock file is not a plain file of this user (a link, a device, another owner).
    BadLockFile,
    /// Another process holds the lock.
    Locked,
    /// The checks are not implemented for this platform.
    #[cfg_attr(unix, allow(dead_code))]
    UnsupportedPlatform,
    /// An operating-system call failed.
    Io {
        operation: &'static str,
        source: io::Error,
    },
}

impl fmt::Display for DataDirError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing => f.write_str("the data directory does not exist"),
            Self::NotADirectory => f.write_str("the data directory path is not a directory"),
            Self::WrongOwner => {
                f.write_str("the data directory is not owned by the user running the server")
            }
            Self::WritableByOthers => f.write_str(
                "the data directory can be written by its group or by other users; \
                 remove those permissions (for example with chmod go-w)",
            ),
            Self::BadLockFile => f.write_str(
                "the lock file in the data directory is not a plain file of the user running the server",
            ),
            Self::Locked => f.write_str("another server uses this data directory"),
            Self::UnsupportedPlatform => {
                f.write_str("the data directory checks are only supported on Unix platforms")
            }
            Self::Io { operation, source } => {
                write!(f, "data directory: {operation} failed: {source}")
            }
        }
    }
}

impl std::error::Error for DataDirError {}

/// A checked data directory whose lock this process holds. Dropping it releases the lock.
#[derive(Debug)]
pub(crate) struct DataDirectory {
    _lock: File,
}

fn io_error(operation: &'static str) -> impl FnOnce(io::Error) -> DataDirError {
    move |source| DataDirError::Io { operation, source }
}

/// Checks the directory and takes the exclusive lock.
#[cfg(unix)]
pub(crate) fn open(path: &Path) -> Result<DataDirectory, DataDirError> {
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};

    // SAFETY: geteuid takes no arguments, has no preconditions and cannot fail.
    let user = unsafe { libc::geteuid() };
    // The path itself may be a link that the operator set up; its target is what is checked.
    let meta = match std::fs::metadata(path) {
        Ok(meta) => meta,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err(DataDirError::Missing);
        }
        Err(error) => return Err(io_error("inspect")(error)),
    };
    if !meta.is_dir() {
        return Err(DataDirError::NotADirectory);
    }
    check_directory(meta.uid(), meta.mode(), user)?;
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path.join(LOCK_FILE))
        .map_err(|error| match error.raw_os_error() {
            Some(libc::ELOOP) => DataDirError::BadLockFile,
            _ => io_error("open the lock file")(error),
        })?;
    let lock_meta = file.metadata().map_err(io_error("inspect the lock file"))?;
    if !lock_meta.is_file() || lock_meta.uid() != user {
        return Err(DataDirError::BadLockFile);
    }
    match file.try_lock() {
        Ok(()) => Ok(DataDirectory { _lock: file }),
        Err(TryLockError::WouldBlock) => Err(DataDirError::Locked),
        Err(TryLockError::Error(source)) => Err(DataDirError::Io {
            operation: "lock",
            source,
        }),
    }
}

/// Not supported off Unix: the server runs on Linux.
#[cfg(not(unix))]
pub(crate) fn open(_path: &Path) -> Result<DataDirectory, DataDirError> {
    Err(DataDirError::UnsupportedPlatform)
}

/// Owner and write permissions of the directory itself.
#[cfg(unix)]
fn check_directory(owner: u32, mode: u32, user: u32) -> Result<(), DataDirError> {
    if owner != user {
        return Err(DataDirError::WrongOwner);
    }
    if mode & 0o022 != 0 {
        return Err(DataDirError::WritableByOthers);
    }
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use std::os::unix::fs::{PermissionsExt, symlink};

    use super::*;
    use crate::scratch::Scratch;

    #[test]
    fn a_second_lock_is_refused_until_the_first_is_dropped() {
        let scratch = Scratch::new("lock");
        let first = open(scratch.path()).expect("the first lock");
        let error = open(scratch.path()).unwrap_err();
        assert!(matches!(error, DataDirError::Locked), "{error}");
        assert_eq!(error.to_string(), "another server uses this data directory");
        drop(first);
        open(scratch.path()).expect("the lock is free again");
    }

    #[test]
    fn owner_and_write_permissions_of_the_directory_are_checked() {
        assert!(check_directory(1000, 0o40755, 1000).is_ok());
        assert!(check_directory(1000, 0o40700, 1000).is_ok());
        assert!(matches!(
            check_directory(0, 0o40755, 1000),
            Err(DataDirError::WrongOwner)
        ));
        for mode in [0o40775, 0o40757, 0o40777] {
            assert!(
                matches!(
                    check_directory(1000, mode, 1000),
                    Err(DataDirError::WritableByOthers)
                ),
                "{mode:o}"
            );
        }
        let scratch = Scratch::new("mode");
        std::fs::set_permissions(scratch.path(), std::fs::Permissions::from_mode(0o770)).unwrap();
        assert!(matches!(
            open(scratch.path()),
            Err(DataDirError::WritableByOthers)
        ));
    }

    #[test]
    fn a_missing_directory_a_file_and_a_linked_lock_file_are_refused() {
        let scratch = Scratch::new("kinds");
        let missing = scratch.path().join("absent");
        assert!(matches!(open(&missing), Err(DataDirError::Missing)));
        let file = scratch.path().join("plain");
        std::fs::write(&file, b"x").unwrap();
        assert!(matches!(open(&file), Err(DataDirError::NotADirectory)));
        let data = scratch.path().join("data");
        std::fs::create_dir(&data).unwrap();
        std::fs::set_permissions(&data, std::fs::Permissions::from_mode(0o700)).unwrap();
        symlink(&file, data.join(LOCK_FILE)).unwrap();
        assert!(matches!(open(&data), Err(DataDirError::BadLockFile)));
    }
}
