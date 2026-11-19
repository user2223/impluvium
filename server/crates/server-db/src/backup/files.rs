// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! File system steps of a backup: private files and directories, durable writes and the
//! retention of old sets.

use std::fs::{self, DirBuilder, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use super::error::{BackupError, io as io_error};
use super::names;

/// Name of the directory inside the data directory that holds the sets.
pub(crate) const BACKUPS_DIR: &str = "backups";
/// Name of the manifest; a set without it is not a finished set.
pub(crate) const MANIFEST: &str = "manifest.json";
/// Prefix of directories that are being built or being removed.
pub(crate) const PARTIAL_PREFIX: &str = ".partial-";
/// How many successful pre-migration sets are kept.
pub(crate) const KEEP_PRE_MIGRATION: usize = 3;

/// Creates a new file that only its owner can access. Fails when the file exists.
pub(crate) fn create_new_private(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}

/// Writes a new private file and flushes it to disk.
pub(crate) fn write_new_file(path: &Path, bytes: &[u8]) -> io::Result<()> {
    #[cfg(test)]
    test_hook::before_write()?;
    let mut file = create_new_private(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

/// Creates a directory that only its owner can access. Fails when it exists.
pub(crate) fn create_private_dir(path: &Path) -> io::Result<()> {
    let mut builder = DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path)
}

/// Flushes a directory so that its entries survive a crash.
pub(crate) fn sync_dir(path: &Path) -> io::Result<()> {
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

/// Restricts a file to its owner and flushes it.
pub(crate) fn seal_file(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    OpenOptions::new().read(true).open(path)?.sync_all()
}

/// Reads a small regular file, or `None` when it does not exist. Links are refused.
pub(crate) fn read_optional_small_file(
    path: &Path,
    limit: u64,
    description: &'static str,
) -> Result<Option<Vec<u8>>, BackupError> {
    let meta = match fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(io_error("inspect a data file")(error)),
    };
    if !meta.file_type().is_file() || meta.len() > limit {
        return Err(BackupError::InvalidInput(description));
    }
    let mut bytes = Vec::new();
    File::open(path)
        .and_then(|file| file.take(limit + 1).read_to_end(&mut bytes))
        .map_err(io_error("read a data file"))?;
    if bytes.len() as u64 > limit {
        return Err(BackupError::InvalidInput(description));
    }
    Ok(Some(bytes))
}

/// Makes sure `<data>/backups` exists and is private, and returns its path.
pub(crate) fn prepare_backups_dir(data_dir: &Path) -> Result<PathBuf, BackupError> {
    match fs::metadata(data_dir) {
        Ok(meta) if meta.is_dir() => {}
        Ok(_) => {
            return Err(BackupError::InvalidInput(
                "the data directory is not a directory",
            ));
        }
        Err(error) => return Err(io_error("inspect the data directory")(error)),
    }
    let directory = data_dir.join(BACKUPS_DIR);
    match fs::symlink_metadata(&directory) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            create_private_dir(&directory).map_err(io_error("create the backups directory"))?;
        }
        Err(error) => return Err(io_error("inspect the backups directory")(error)),
        Ok(meta) if !meta.file_type().is_dir() => {
            return Err(BackupError::InvalidInput(
                "the backups path exists but is not a directory",
            ));
        }
        Ok(_meta) => {
            #[cfg(unix)]
            {
                use std::os::unix::fs::{MetadataExt, PermissionsExt};
                if _meta.mode() & 0o077 != 0 {
                    fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))
                        .map_err(io_error("restrict the backups directory"))?;
                }
            }
        }
    }
    Ok(directory)
}

/// Removes directories left behind by runs that did not finish. Errors are ignored: a
/// stale directory is harmless and never counts as a set.
pub(crate) fn remove_stale_partials(backups: &Path) {
    let Ok(entries) = fs::read_dir(backups) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        if !name.to_string_lossy().starts_with(PARTIAL_PREFIX) {
            continue;
        }
        let path = entry.path();
        match fs::symlink_metadata(&path) {
            Ok(meta) if meta.file_type().is_dir() => {
                let _ = fs::remove_dir_all(&path);
            }
            Ok(_) => {
                let _ = fs::remove_file(&path);
            }
            Err(_) => {}
        }
    }
}

static COUNTER: AtomicU32 = AtomicU32::new(0);

/// Creates the private directory in which a set is built.
pub(crate) fn create_partial(backups: &Path, name: &str) -> Result<PathBuf, BackupError> {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.subsec_nanos());
    for _ in 0..16 {
        let suffix = format!(
            "{:x}-{:x}-{:x}",
            std::process::id(),
            nanos,
            COUNTER.fetch_add(1, Ordering::Relaxed)
        );
        let path = backups.join(format!("{PARTIAL_PREFIX}{name}-{suffix}"));
        match create_private_dir(&path) {
            Ok(()) => return Ok(path),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(io_error("create the working directory")(error)),
        }
    }
    Err(BackupError::Io {
        step: "create the working directory",
        kind: io::ErrorKind::AlreadyExists,
    })
}

/// A name for a new set: `base` when no set of this kind has the same time stamp, otherwise
/// `base-<n>` with `n` above every counter in use for that stamp. A later set therefore always
/// sorts after an earlier one, also when an earlier one was removed meanwhile.
pub(crate) fn unique_name(
    backups: &Path,
    base: &str,
    stamp: &str,
    pre_migration: bool,
) -> Result<String, BackupError> {
    let mut highest = 0u32;
    if let Ok(entries) = fs::read_dir(backups) {
        for entry in entries.flatten() {
            if let Ok(name) = entry.file_name().into_string()
                && let Some(parsed) = names::parse_name(&name)
                && parsed.stamp == stamp
                && parsed.pre_migration == pre_migration
            {
                highest = highest.max(parsed.counter);
            }
        }
    }
    for number in (highest + 1)..=(highest + 1000) {
        let name = if number == 1 {
            base.to_owned()
        } else {
            format!("{base}-{number}")
        };
        match fs::symlink_metadata(backups.join(&name)) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(name),
            Err(error) => return Err(io_error("choose a name for the set")(error)),
            Ok(_) => {}
        }
    }
    Err(BackupError::Io {
        step: "choose a name for the set",
        kind: io::ErrorKind::AlreadyExists,
    })
}

/// A directory that is removed with everything in it unless it was released.
pub(crate) struct PartialGuard {
    path: PathBuf,
    armed: bool,
}

impl PartialGuard {
    pub(crate) fn new(path: PathBuf) -> Self {
        Self { path, armed: true }
    }

    pub(crate) fn release(&mut self) {
        self.armed = false;
    }
}

impl Drop for PartialGuard {
    fn drop(&mut self) {
        if self.armed {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

/// Deletes older successful pre-migration sets so that `KEEP_PRE_MIGRATION` remain in total,
/// counting the set `keep` that was just made. That set is never removed, whatever its time stamp
/// says (the clock may be behind older sets): the newest `KEEP_PRE_MIGRATION - 1` other sets stay
/// next to it. Returns the removed names and whether a removal failed. Sets of other kinds, sets
/// without a manifest and everything else in the directory are never touched.
pub(crate) fn apply_retention(backups: &Path, keep: &str) -> (Vec<String>, bool) {
    let Ok(entries) = fs::read_dir(backups) else {
        return (Vec::new(), true);
    };
    let mut sets = Vec::new();
    for entry in entries.flatten() {
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        if name == keep {
            continue;
        }
        let Some(parsed) = names::parse_name(&name) else {
            continue;
        };
        if !parsed.pre_migration {
            continue;
        }
        let path = entry.path();
        let is_dir = fs::symlink_metadata(&path).is_ok_and(|meta| meta.file_type().is_dir());
        let has_manifest =
            fs::symlink_metadata(path.join(MANIFEST)).is_ok_and(|meta| meta.file_type().is_file());
        if is_dir && has_manifest {
            sets.push((parsed, name));
        }
    }
    sets.sort();
    let surplus = sets.len().saturating_sub(KEEP_PRE_MIGRATION - 1);
    let mut removed = Vec::new();
    let mut failed = false;
    for (_, name) in sets.into_iter().take(surplus) {
        // Renaming first turns the set into a working directory at once, so a removal that
        // stops half way never leaves something that looks like a complete set.
        let doomed = backups.join(format!("{PARTIAL_PREFIX}removed-{name}"));
        let renamed = fs::rename(backups.join(&name), &doomed);
        match renamed.and_then(|()| fs::remove_dir_all(&doomed)) {
            Ok(()) => removed.push(name),
            Err(_) => failed = true,
        }
    }
    (removed, failed)
}

/// Name of the lock file in the backups directory. Cleanup and retention never touch it.
pub(crate) const LOCK_FILE: &str = ".lock";

/// Takes the exclusive lock that serialises backups. The lock lasts as long as the returned file
/// is alive. Fails with [`BackupError::AlreadyRunning`] when another backup holds it.
pub(crate) fn acquire_lock(backups: &Path) -> Result<File, BackupError> {
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options
        .open(backups.join(LOCK_FILE))
        .map_err(io_error("open the backup lock"))?;
    match file.try_lock() {
        Ok(()) => Ok(file),
        Err(fs::TryLockError::WouldBlock) => Err(BackupError::AlreadyRunning),
        Err(fs::TryLockError::Error(error)) => Err(io_error("take the backup lock")(error)),
    }
}

/// Failure injection for tests: the n-th call of [`write_new_file`] on this thread fails.
#[cfg(test)]
pub(crate) mod test_hook {
    use std::cell::Cell;
    use std::io::{self, ErrorKind};

    thread_local! {
        static FAIL: Cell<Option<(u32, ErrorKind)>> = const { Cell::new(None) };
    }

    pub(crate) fn fail_write(number: u32, kind: ErrorKind) {
        FAIL.with(|cell| cell.set(Some((number, kind))));
    }

    pub(crate) fn clear() {
        FAIL.with(|cell| cell.set(None));
    }

    pub(super) fn before_write() -> io::Result<()> {
        FAIL.with(|cell| match cell.get() {
            Some((1, kind)) => {
                cell.set(None);
                Err(io::Error::from(kind))
            }
            Some((number, kind)) => {
                cell.set(Some((number - 1, kind)));
                Ok(())
            }
            None => Ok(()),
        })
    }
}
