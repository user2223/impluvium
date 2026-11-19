// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! `impluvium-server backup`: writes one manual backup set and stops.
//!
//! The command loads the configuration like `config check`, validates the instance key with the
//! same rules the server applies, and always uses the built-in `pg_dump` backup, whatever
//! `IMPLUVIUM_BACKUP_MODE` says. It prints the name of the set on success and a message without
//! secrets on failure.

use std::process::ExitCode;
use std::time::Duration;

use server_db::DatabaseUrl;
use server_db::backup::{BackupContext, BackupKind, BackupMode, INSTANCE_KEY_FILE, backup_manual};

use crate::config::{self, Source};
use crate::keys::{self, Context};

/// Runs the backup for a configuration source and returns the success line.
fn execute(source: &Source) -> Result<String, String> {
    let config =
        config::load(source).map_err(|error| format!("configuration is invalid\n{error}"))?;
    let key = keys::load_or_create(&config.data_dir.join(INSTANCE_KEY_FILE), Context::Existing)
        .map_err(|error| format!("the instance key cannot be used: {error}"))?;
    let url = DatabaseUrl::new(config.database_url.expose());
    let context = BackupContext {
        database_url: &url,
        data_dir: &config.data_dir,
        instance_key: key.expose_bytes(),
        kind: BackupKind::Manual,
        mode: BackupMode::Auto,
        timeout: Duration::from_secs(u64::from(config.backup_timeout_secs)),
        tools: None,
    };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| "the backup could not start its runtime".to_owned())?;
    match runtime.block_on(backup_manual(&context)) {
        Ok(set) => Ok(format!("backup written: {}", set.name)),
        Err(error) => Err(format!("backup failed: {error}")),
    }
}

/// `impluvium-server backup`.
pub fn run() -> ExitCode {
    match execute(&Source::from_process()) {
        Ok(text) => {
            println!("{text}");
            ExitCode::SUCCESS
        }
        Err(text) => {
            eprintln!("{text}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MARKER: &str = "zz-backup-marker";

    #[test]
    fn an_invalid_configuration_fails_without_repeating_values() {
        let source = Source::from_pairs(&[
            ("IMPLUVIUM_EXTERNAL_URL", "https://impluvium.example"),
            ("IMPLUVIUM_DATA_DIR", "/srv/impluvium/data"),
            ("DATABASE_URL", MARKER),
        ]);
        let text = execute(&source).unwrap_err();
        assert!(text.starts_with("configuration is invalid"), "{text}");
        assert!(!text.contains(MARKER));
    }

    #[test]
    fn a_missing_instance_key_stops_before_any_database_access() {
        let url = format!("postgres://u:{MARKER}@db.example.test:1/x");
        let source = Source::from_pairs(&[
            ("IMPLUVIUM_EXTERNAL_URL", "https://impluvium.example"),
            ("IMPLUVIUM_DATA_DIR", "/nonexistent-impluvium-dir/data"),
            ("DATABASE_URL", url.as_str()),
        ]);
        let text = execute(&source).unwrap_err();
        assert!(text.contains("instance key"), "{text}");
        assert!(!text.contains(MARKER) && !text.contains("db.example"));
    }
}
