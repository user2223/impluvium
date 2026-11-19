// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! The instance key file: exclusive creation, strict validation and derivation
//! of the public signing key metadata.
//!
//! The instance key is 32 random bytes and the root of all server signing keys.
//! This module never touches a database and never prints key material: the
//! private value has no `Debug` output, no `Display` and appears in no error.
//!
//! Creation is crash-safe and race-free. The key is written to a temporary file
//! in the same directory, flushed to disk and only then published with a hard
//! link, which fails when the target already exists. A reader therefore sees
//! either no file or a complete one, and two concurrent creators end up with the
//! same key. A crash leaves at most a stale temporary file, which is never read
//! as a key.
//!
//! Only an explicitly new context may create the file. An existing context with
//! a missing file is an error, so a lost key is never silently replaced.
//!
//! The strict reading of the file rules is Unix-only: owner permissions beyond
//! `0600`, symlinks and non-regular files are rejected. On other platforms the
//! functions compile but report `UnsupportedPlatform`, because the server runs
//! on Linux.

use std::error::Error;
use std::fmt;
use std::path::Path;

use impluvium_protocol::{ids, keys as protocol_keys};
use server_domain::identity::{KeyId, KeyPurpose, PublicKey, SigningKeyMetadata, TicketGeneration};
use zeroize::Zeroizing;

/// Length of the instance key in bytes.
pub const KEY_LEN: usize = 32;

/// Whether the instance is being created or already exists.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Context {
    /// A brand-new instance: a missing key file is created.
    New,
    /// An existing instance: a missing key file is an error.
    Existing,
}

/// Why the key file could not be provided. Never contains key bytes.
#[derive(Debug)]
pub enum KeyFileError {
    /// The file does not exist (and the context does not allow creating it).
    Missing,
    /// The path is a symbolic link; links are never followed.
    Symlink,
    /// The path exists but is not a regular file.
    NotRegularFile,
    /// The file mode is not exactly 0600 or 0400.
    PermissionsTooOpen,
    /// The file belongs to another user than the one running the server.
    WrongOwner,
    /// The file is shorter or longer than the key length.
    WrongSize,
    /// The content is not a usable key (all zero).
    InvalidKey,
    /// The file was replaced while it was being opened.
    ChangedWhileOpening,
    /// The path has no file name component.
    InvalidPath,
    /// The strict file checks are not implemented for this platform.
    UnsupportedPlatform,
    /// An operating-system call failed.
    Io {
        operation: &'static str,
        source: std::io::Error,
    },
}

impl fmt::Display for KeyFileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing => f.write_str("instance key file is missing"),
            Self::Symlink => f.write_str("instance key path is a symbolic link"),
            Self::NotRegularFile => f.write_str("instance key path is not a regular file"),
            Self::PermissionsTooOpen => f.write_str("instance key file mode must be 0600 or 0400"),
            Self::WrongOwner => {
                f.write_str("instance key file is not owned by the user running the server")
            }
            Self::WrongSize => f.write_str("instance key file has the wrong size"),
            Self::InvalidKey => f.write_str("instance key file does not contain a usable key"),
            Self::ChangedWhileOpening => {
                f.write_str("instance key file changed while it was being opened")
            }
            Self::InvalidPath => f.write_str("instance key path has no file name"),
            Self::UnsupportedPlatform => {
                f.write_str("instance key files are only supported on Unix platforms")
            }
            Self::Io { operation, source } => {
                write!(f, "instance key file: {operation} failed: {source}")
            }
        }
    }
}

impl Error for KeyFileError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// The secret instance key. Its `Debug` output hides the key bytes and its
/// memory is overwritten when it goes out of scope.
pub struct InstanceKey(Zeroizing<[u8; KEY_LEN]>);

impl fmt::Debug for InstanceKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("InstanceKey(..)")
    }
}

impl InstanceKey {
    /// The raw key bytes, only for copying the key into a backup set. Never log or print them.
    pub fn expose_bytes(&self) -> &[u8; KEY_LEN] {
        &self.0
    }

    /// Public metadata of the single content signing key.
    pub fn content_key(&self) -> SigningKeyMetadata {
        let public = protocol_keys::derive_content_key(&self.0).public_key();
        metadata(KeyPurpose::Content, public)
    }

    /// Public metadata of the depot ticket signing key of one generation.
    pub fn ticket_key(&self, generation: TicketGeneration) -> SigningKeyMetadata {
        let public = protocol_keys::derive_ticket_key(&self.0, generation.get()).public_key();
        metadata(KeyPurpose::DepotTicket(generation), public)
    }
}

fn metadata(purpose: KeyPurpose, public: [u8; 32]) -> SigningKeyMetadata {
    SigningKeyMetadata {
        purpose,
        public_key: PublicKey::new(public),
        key_id: KeyId::new(ids::key_id(&public)),
    }
}

/// Loads and validates the instance key file. In the new context a missing file
/// is created first; in every other case an existing file is only validated and
/// never changed. Concurrent callers all receive the same key.
pub fn load_or_create(path: &Path, context: Context) -> Result<InstanceKey, KeyFileError> {
    match sys::read_validated(path) {
        Ok(bytes) => Ok(InstanceKey(bytes)),
        Err(KeyFileError::Missing) if context == Context::New => {
            sys::create_exclusive(path)?;
            sys::read_validated(path).map(InstanceKey)
        }
        Err(error) => Err(error),
    }
}

#[cfg(unix)]
mod sys {
    use std::fs::{self, File, OpenOptions};
    use std::io::{ErrorKind, Read, Write};
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
    use std::path::Path;

    use zeroize::Zeroizing;

    use super::{KEY_LEN, KeyFileError};

    fn io(operation: &'static str) -> impl FnOnce(std::io::Error) -> KeyFileError {
        move |source| KeyFileError::Io { operation, source }
    }

    /// Reads the key file after checking type, rights and size.
    pub fn read_validated(path: &Path) -> Result<Zeroizing<[u8; KEY_LEN]>, KeyFileError> {
        let before = match fs::symlink_metadata(path) {
            Ok(meta) => meta,
            Err(error) if error.kind() == ErrorKind::NotFound => return Err(KeyFileError::Missing),
            Err(error) => return Err(io("inspect")(error)),
        };
        let kind = before.file_type();
        if kind.is_symlink() {
            return Err(KeyFileError::Symlink);
        }
        if !kind.is_file() {
            return Err(KeyFileError::NotRegularFile);
        }
        // O_NOFOLLOW refuses a link swapped in after the check above; O_NONBLOCK
        // keeps the open from blocking on a FIFO swapped in the same way.
        let mut file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(path)
            .map_err(|error| match error.raw_os_error() {
                Some(libc::ELOOP) => KeyFileError::Symlink,
                _ => io("open")(error),
            })?;
        let meta = file.metadata().map_err(io("inspect"))?;
        // The opened handle must be the very file that was inspected, so a
        // swap between the two steps cannot smuggle in a link or device.
        if !meta.is_file() || meta.dev() != before.dev() || meta.ino() != before.ino() {
            return Err(KeyFileError::ChangedWhileOpening);
        }
        check_mode(meta.mode())?;
        check_owner(meta.uid(), effective_uid())?;
        if meta.len() != KEY_LEN as u64 {
            return Err(KeyFileError::WrongSize);
        }
        let mut data = Zeroizing::new(Vec::<u8>::with_capacity(KEY_LEN + 1));
        (&mut file)
            .take(KEY_LEN as u64 + 1)
            .read_to_end(&mut data)
            .map_err(io("read"))?;
        match <[u8; KEY_LEN]>::try_from(data.as_slice()) {
            Ok(bytes) if bytes.iter().any(|byte| *byte != 0) => Ok(Zeroizing::new(bytes)),
            Ok(_) => Err(KeyFileError::InvalidKey),
            Err(_) => Err(KeyFileError::WrongSize),
        }
    }

    /// Exactly 0600 (read-write key) or 0400 (read-only key), no special bits.
    pub fn check_mode(mode: u32) -> Result<(), KeyFileError> {
        match mode & 0o7777 {
            0o600 | 0o400 => Ok(()),
            _ => Err(KeyFileError::PermissionsTooOpen),
        }
    }

    /// The key file must belong to the user the server runs as.
    pub fn check_owner(file_uid: u32, effective_uid: u32) -> Result<(), KeyFileError> {
        if file_uid == effective_uid {
            Ok(())
        } else {
            Err(KeyFileError::WrongOwner)
        }
    }

    fn effective_uid() -> u32 {
        // SAFETY: geteuid takes no arguments, has no preconditions and cannot fail.
        unsafe { libc::geteuid() }
    }

    fn fill_random(buffer: &mut [u8]) -> Result<(), KeyFileError> {
        File::open("/dev/urandom")
            .and_then(|mut source| source.read_exact(buffer))
            .map_err(io("read random bytes"))
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    /// Publishes a fresh random key at `path` unless a file is already there.
    /// Returns `Ok` both when this call created the file and when another
    /// creator was first; the caller reads the file back either way.
    pub fn create_exclusive(path: &Path) -> Result<(), KeyFileError> {
        let name = path.file_name().ok_or(KeyFileError::InvalidPath)?;
        let directory = match path.parent() {
            Some(parent) if !parent.as_os_str().is_empty() => parent,
            _ => Path::new("."),
        };
        let mut key = Zeroizing::new([0u8; KEY_LEN]);
        let mut suffix = [0u8; 8];
        fill_random(&mut key[..])?;
        fill_random(&mut suffix)?;
        let temporary = directory.join(format!(".{}.{}.tmp", name.to_string_lossy(), hex(&suffix)));

        // If the temporary name cannot be created exclusively, it belongs to
        // someone else and must be left alone.
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)
            .map_err(io("create temporary file"))?;
        let written = write_temporary(file, &key);
        // A hard link never replaces an existing target: of several creators
        // exactly one wins, the others see AlreadyExists and read the winner.
        let published = written.and_then(|()| match fs::hard_link(&temporary, path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == ErrorKind::AlreadyExists => Ok(()),
            Err(error) => Err(io("publish")(error)),
        });
        // Remove our temporary name on every outcome; the key stays reachable
        // through the published name.
        let _ = fs::remove_file(&temporary);
        published?;
        // Make the new directory entry durable.
        File::open(directory)
            .and_then(|handle| handle.sync_all())
            .map_err(io("sync directory"))
    }

    fn write_temporary(mut file: File, key: &[u8; KEY_LEN]) -> Result<(), KeyFileError> {
        file.write_all(key).map_err(io("write"))?;
        file.sync_all().map_err(io("sync"))
    }
}

#[cfg(not(unix))]
mod sys {
    use std::path::Path;

    use zeroize::Zeroizing;

    use super::{KEY_LEN, KeyFileError};

    pub fn read_validated(_path: &Path) -> Result<Zeroizing<[u8; KEY_LEN]>, KeyFileError> {
        Err(KeyFileError::UnsupportedPlatform)
    }

    pub fn create_exclusive(_path: &Path) -> Result<(), KeyFileError> {
        Err(KeyFileError::UnsupportedPlatform)
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::{PermissionsExt, symlink};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::{Arc, Barrier};
    use std::thread;

    /// A unique scratch directory; `with_scratch` removes it after the test body.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new() -> Self {
            static COUNTER: AtomicU32 = AtomicU32::new(0);
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |elapsed| elapsed.as_nanos());
            let path = std::env::temp_dir().join(format!(
                "impluvium-keys-{}-{}-{nanos}",
                std::process::id(),
                COUNTER.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).expect("create scratch directory");
            Self(path)
        }

        fn key(&self) -> PathBuf {
            self.0.join("instance.key")
        }
    }

    fn with_scratch(body: impl FnOnce(&Scratch)) {
        let scratch = Scratch::new();
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| body(&scratch)));
        let _ = fs::remove_dir_all(&scratch.0);
        if let Err(panic) = outcome {
            std::panic::resume_unwind(panic);
        }
    }

    fn write_key(path: &Path, bytes: &[u8], mode: u32) {
        fs::write(path, bytes).expect("write key file");
        fs::set_permissions(path, fs::Permissions::from_mode(mode)).expect("set mode");
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    fn entries(directory: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(directory)
            .expect("list directory")
            .map(|entry| {
                entry
                    .expect("entry")
                    .file_name()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        names.sort();
        names
    }

    #[test]
    fn new_context_creates_a_private_complete_file() {
        with_scratch(|scratch| {
            let key = load_or_create(&scratch.key(), Context::New).expect("create");
            let meta = fs::metadata(scratch.key()).expect("metadata");
            assert_eq!(meta.len(), KEY_LEN as u64);
            assert_eq!(meta.permissions().mode() & 0o7777, 0o600);
            assert_eq!(
                entries(&scratch.0),
                ["instance.key"],
                "no temporary file remains"
            );
            let again = load_or_create(&scratch.key(), Context::Existing).expect("reload");
            assert_eq!(key.content_key(), again.content_key());
            let generation = TicketGeneration::new(1).unwrap();
            assert_eq!(key.ticket_key(generation), again.ticket_key(generation));
            // A second call in the new context returns the existing key unchanged.
            let third = load_or_create(&scratch.key(), Context::New).expect("reload as new");
            assert_eq!(key.content_key(), third.content_key());
        });
    }

    #[test]
    fn concurrent_creation_yields_one_key() {
        with_scratch(|scratch| {
            let path = Arc::new(scratch.key());
            let barrier = Arc::new(Barrier::new(8));
            let handles: Vec<_> = (0..8)
                .map(|_| {
                    let path = Arc::clone(&path);
                    let barrier = Arc::clone(&barrier);
                    thread::spawn(move || {
                        barrier.wait();
                        load_or_create(&path, Context::New)
                            .expect("create or load")
                            .content_key()
                    })
                })
                .collect();
            let results: Vec<_> = handles
                .into_iter()
                .map(|h| h.join().expect("thread"))
                .collect();
            assert!(results.windows(2).all(|pair| pair[0] == pair[1]));
            let reloaded = load_or_create(&path, Context::Existing).expect("reload");
            assert_eq!(reloaded.content_key(), results[0]);
            assert_eq!(entries(&scratch.0), ["instance.key"]);
        });
    }

    #[test]
    fn missing_key_in_an_existing_context_is_an_error_and_creates_nothing() {
        with_scratch(|scratch| {
            let error = load_or_create(&scratch.key(), Context::Existing).unwrap_err();
            assert!(matches!(error, KeyFileError::Missing));
            assert!(entries(&scratch.0).is_empty());
        });
    }

    #[test]
    fn truncated_or_oversized_files_are_rejected_and_kept() {
        with_scratch(|scratch| {
            for length in [0usize, 31, 33, 64] {
                write_key(&scratch.key(), &vec![7u8; length], 0o600);
                for context in [Context::Existing, Context::New] {
                    let error = load_or_create(&scratch.key(), context).unwrap_err();
                    assert!(matches!(error, KeyFileError::WrongSize), "length {length}");
                }
                assert_eq!(fs::metadata(scratch.key()).unwrap().len(), length as u64);
            }
        });
    }

    #[test]
    fn all_zero_key_is_rejected() {
        with_scratch(|scratch| {
            write_key(&scratch.key(), &[0u8; KEY_LEN], 0o600);
            let error = load_or_create(&scratch.key(), Context::Existing).unwrap_err();
            assert!(matches!(error, KeyFileError::InvalidKey));
        });
    }

    #[test]
    fn readable_or_writable_by_others_is_rejected() {
        with_scratch(|scratch| {
            for mode in [0o644, 0o640, 0o660, 0o604, 0o700, 0o2600] {
                write_key(&scratch.key(), &[9u8; KEY_LEN], mode);
                let error = load_or_create(&scratch.key(), Context::Existing).unwrap_err();
                assert!(
                    matches!(error, KeyFileError::PermissionsTooOpen),
                    "mode {mode:o}"
                );
            }
            write_key(&scratch.key(), &[9u8; KEY_LEN], 0o400);
            assert!(load_or_create(&scratch.key(), Context::Existing).is_ok());
        });
    }

    #[test]
    fn symlinks_are_rejected_even_when_valid_or_dangling() {
        with_scratch(|scratch| {
            let target = scratch.0.join("real.key");
            write_key(&target, &[5u8; KEY_LEN], 0o600);
            symlink(&target, scratch.key()).expect("symlink");
            for context in [Context::Existing, Context::New] {
                let error = load_or_create(&scratch.key(), context).unwrap_err();
                assert!(matches!(error, KeyFileError::Symlink));
            }
            fs::remove_file(scratch.key()).unwrap();
            symlink(scratch.0.join("nowhere"), scratch.key()).expect("dangling symlink");
            let error = load_or_create(&scratch.key(), Context::New).unwrap_err();
            assert!(matches!(error, KeyFileError::Symlink));
            assert!(
                !scratch.0.join("nowhere").exists(),
                "creation must not follow the link"
            );
        });
    }

    #[test]
    fn owner_comparison_rejects_foreign_files() {
        assert!(sys::check_owner(1000, 1000).is_ok());
        assert!(matches!(
            sys::check_owner(0, 1000),
            Err(KeyFileError::WrongOwner)
        ));
        assert!(matches!(
            sys::check_owner(1000, 0),
            Err(KeyFileError::WrongOwner)
        ));
    }

    #[test]
    fn mode_check_accepts_only_0600_and_0400() {
        assert!(sys::check_mode(0o100600).is_ok());
        assert!(sys::check_mode(0o100400).is_ok());
        for mode in [0o100000, 0o100200, 0o100640, 0o102600, 0o100700] {
            assert!(matches!(
                sys::check_mode(mode),
                Err(KeyFileError::PermissionsTooOpen)
            ));
        }
    }

    #[test]
    fn losing_the_creation_race_keeps_the_winners_key_and_leaves_no_temporary_file() {
        with_scratch(|scratch| {
            let winner = [3u8; KEY_LEN];
            write_key(&scratch.key(), &winner, 0o600);
            sys::create_exclusive(&scratch.key()).expect("lost race is not an error");
            assert_eq!(fs::read(scratch.key()).unwrap(), winner);
            assert_eq!(entries(&scratch.0), ["instance.key"]);
        });
    }

    #[test]
    fn directories_are_rejected() {
        with_scratch(|scratch| {
            fs::create_dir(scratch.key()).unwrap();
            for context in [Context::Existing, Context::New] {
                let error = load_or_create(&scratch.key(), context).unwrap_err();
                assert!(matches!(error, KeyFileError::NotRegularFile));
            }
        });
    }

    #[test]
    fn leftover_partial_temporary_file_is_never_accepted_as_the_key() {
        with_scratch(|scratch| {
            // What a crash during creation leaves behind: a partial temporary file.
            write_key(
                &scratch.0.join(".instance.key.0123456789abcdef.tmp"),
                &[1u8; 10],
                0o600,
            );
            let error = load_or_create(&scratch.key(), Context::Existing).unwrap_err();
            assert!(matches!(error, KeyFileError::Missing));
            let key = load_or_create(&scratch.key(), Context::New).expect("create");
            assert_eq!(fs::metadata(scratch.key()).unwrap().len(), KEY_LEN as u64);
            assert!(
                key.content_key()
                    .public_key
                    .as_bytes()
                    .iter()
                    .any(|b| *b != 0)
            );
        });
    }

    #[test]
    fn debug_output_and_errors_do_not_show_the_key() {
        with_scratch(|scratch| {
            let bytes: Vec<u8> = (0u8..32).collect();
            write_key(&scratch.key(), &bytes, 0o600);
            let key = load_or_create(&scratch.key(), Context::Existing).expect("load");
            assert_eq!(format!("{key:?}"), "InstanceKey(..)");
            write_key(&scratch.key(), &bytes, 0o644);
            let error = load_or_create(&scratch.key(), Context::Existing).unwrap_err();
            let shown = format!("{error} {error:?}");
            assert!(!shown.contains(&hex(&bytes)));
        });
    }

    // Public synthetic known-answer values of the protocol vectors: the instance
    // key 00 01 .. 1f and the expected public keys and key identifiers.
    fn unhex<const N: usize>(text: &str) -> [u8; N] {
        let mut out = [0u8; N];
        assert_eq!(text.len(), N * 2);
        for (index, byte) in out.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&text[index * 2..index * 2 + 2], 16).expect("hex");
        }
        out
    }

    #[test]
    fn derivation_matches_the_protocol_vectors_after_a_file_round_trip() {
        with_scratch(|scratch| {
            let instance: Vec<u8> = (0u8..32).collect();
            write_key(&scratch.key(), &instance, 0o600);
            let key = load_or_create(&scratch.key(), Context::Existing).expect("load");

            let content = key.content_key();
            assert_eq!(content.purpose, KeyPurpose::Content);
            assert_eq!(
                content.public_key.as_bytes(),
                &unhex::<32>("5f5603361a832deb6ae7edfbafa292396c2158650ae6d267fd10416f8eb36e8d")
            );
            assert_eq!(content.key_id.as_bytes(), &unhex::<8>("e3176bf3bff6f8e7"));

            let tickets = [
                (
                    1,
                    "e43a9f966dbcdd79435f3cd09d46110c5af3a383e175765a61d19498043990f3",
                    "417b006e05e6fbee",
                ),
                (
                    2,
                    "8c138642ff45eb182117876cc7415e736225183f1f76b801726faf610ebce55e",
                    "176afab9d46ad842",
                ),
                (
                    u32::MAX,
                    "cf7e8b09b30fa9232b1d9f93d8f3bbc159019039cb2c40046ed59dd6bdd50c00",
                    "76c7064494fdca6a",
                ),
            ];
            for (generation, public, id) in tickets {
                let generation = TicketGeneration::new(generation).unwrap();
                let ticket = key.ticket_key(generation);
                assert_eq!(ticket.purpose, KeyPurpose::DepotTicket(generation));
                assert_eq!(ticket.public_key.as_bytes(), &unhex::<32>(public));
                assert_eq!(ticket.key_id.as_bytes(), &unhex::<8>(id));
            }
        });
    }
}
