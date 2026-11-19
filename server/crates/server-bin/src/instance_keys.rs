// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! The instance key file as the start guard sees it.
//!
//! [`FileKeys`] implements [`InstanceKeys`] on top of the strict key file rules in `keys`: an
//! existing file is validated and never changed, a missing file is reported as absent, and
//! creation fails when anything is at the path, also when a concurrent creator got there first.

use std::path::{Path, PathBuf};

use server_db::backup::INSTANCE_KEY_FILE;
use server_db::start_guard::{InstanceKeys, KeyError, LoadedKeys, PublicKeys};
use server_domain::identity::TicketGeneration;

use crate::keys::{self, Context, InstanceKey, KeyFileError};

/// The instance key file in a data directory.
#[derive(Debug)]
pub(crate) struct FileKeys {
    path: PathBuf,
}

impl FileKeys {
    pub(crate) fn new(data_dir: &Path) -> Self {
        Self {
            path: data_dir.join(INSTANCE_KEY_FILE),
        }
    }
}

fn public_keys(key: &InstanceKey) -> Result<PublicKeys, KeyError> {
    let first = TicketGeneration::new(1)
        .ok_or_else(|| KeyError::new("the first ticket key generation is not valid"))?;
    let content = key.content_key();
    let ticket = key.ticket_key(first);
    Ok(PublicKeys {
        content_public_key: *content.public_key.as_bytes(),
        content_key_id: *content.key_id.as_bytes(),
        ticket_key_id: *ticket.key_id.as_bytes(),
    })
}

fn key_error(error: &KeyFileError) -> KeyError {
    KeyError::new(error.to_string())
}

impl InstanceKeys for FileKeys {
    fn existing(&self) -> Result<Option<LoadedKeys>, KeyError> {
        match keys::load_or_create(&self.path, Context::Existing) {
            Ok(key) => {
                let public = public_keys(&key)?;
                // The guard needs the raw bytes for the backup copy. The copy is a plain array
                // that the guard drops after the start; it is not overwritten on drop.
                Ok(Some(LoadedKeys {
                    public,
                    instance_key: *key.expose_bytes(),
                }))
            }
            Err(KeyFileError::Missing) => Ok(None),
            Err(error) => Err(key_error(&error)),
        }
    }

    fn create(&self) -> Result<PublicKeys, KeyError> {
        let key = keys::create_new(&self.path).map_err(|error| key_error(&error))?;
        public_keys(&key)
    }
}

#[cfg(all(test, unix))]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use super::*;
    use crate::scratch::Scratch;

    #[test]
    fn a_missing_key_file_is_none_and_nothing_is_created() {
        let scratch = Scratch::new("keys-missing");
        let adapter = FileKeys::new(scratch.path());
        assert!(adapter.existing().unwrap().is_none());
        assert!(!scratch.path().join(INSTANCE_KEY_FILE).exists());
    }

    #[test]
    fn create_makes_the_file_and_a_second_create_fails() {
        let scratch = Scratch::new("keys-create");
        let adapter = FileKeys::new(scratch.path());
        let public = adapter.create().unwrap();
        let loaded = adapter.existing().unwrap().expect("the key exists now");
        assert_eq!(loaded.public, public);
        let bytes = std::fs::read(scratch.path().join(INSTANCE_KEY_FILE)).unwrap();
        assert_eq!(&loaded.instance_key[..], &bytes[..]);
        let error = adapter.create().unwrap_err();
        assert!(error.to_string().contains("exists already"), "{error}");
        assert_eq!(
            std::fs::read(scratch.path().join(INSTANCE_KEY_FILE)).unwrap(),
            bytes
        );
    }

    #[test]
    fn an_unusable_key_file_is_an_error_not_a_missing_key() {
        let scratch = Scratch::new("keys-bad");
        let path = scratch.path().join(INSTANCE_KEY_FILE);
        std::fs::write(&path, [7u8; 32]).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        let adapter = FileKeys::new(scratch.path());
        assert!(adapter.existing().is_err());
        assert!(adapter.create().is_err());
    }
}
