// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! `cargo xtask sqlx <arguments>` runs the sqlx-cli that this workspace pins.
//!
//! Everything after `sqlx` goes to sqlx-cli unchanged:
//!
//! ```text
//! cargo xtask sqlx migrate run --source server/migrations
//! cargo xtask sqlx prepare --workspace -- --all-targets --locked -p server-db
//! cargo xtask sqlx prepare --workspace --check -- --all-targets --locked -p server-db
//! ```
//!
//! `cargo xtask api-types [--check]` writes the TypeScript declarations of the wire shapes of
//! `server-api-types` to `server/shared/api/shapes/`, or with `--check` fails when that directory
//! differs from what the Rust types produce (changed, missing or stray files).
//!
//! TLS is not compiled in, so a database URL with `sslmode=require` or `verify-*` fails.
//!
//! The arguments after the second `--` reach the Cargo build that `prepare` starts, so that build is
//! locked too. The alias `xtask` in `.cargo/config.toml` runs this package with `--locked`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use clap::Parser;

#[derive(Parser, Debug)]
#[command(bin_name = "cargo xtask")]
enum Cli {
    /// Run the pinned sqlx-cli; everything after `sqlx` is passed through.
    Sqlx {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Write the TypeScript declarations of the wire shapes, or check that they are up to date.
    ApiTypes {
        /// Fail on any difference instead of writing.
        #[arg(long)]
        check: bool,
    },
}

/// The repository root: this package lives in `tools/server-xtask`.
fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// All `.ts` files directly in `directory`, by file name. A missing directory is empty.
fn read_directory(directory: &Path) -> Result<BTreeMap<String, String>, String> {
    let mut files = BTreeMap::new();
    let Ok(entries) = std::fs::read_dir(directory) else {
        return Ok(files);
    };
    for entry in entries {
        let entry = entry.map_err(|error| error.to_string())?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let content =
            std::fs::read_to_string(entry.path()).map_err(|error| format!("{name}: {error}"))?;
        files.insert(name, content);
    }
    Ok(files)
}

/// The files the Rust types produce: ts-rs writes to a scratch directory, then the set is assembled.
fn expected_files() -> Result<BTreeMap<String, String>, String> {
    server_api_types::export::declared_names()?;
    let root_names = server_api_types::export::root_names();
    let mut seen = std::collections::BTreeSet::new();
    if let Some(name) = root_names.iter().find(|name| !seen.insert(name.as_str())) {
        return Err(format!("duplicate shape root {name}"));
    }
    let scratch = repository_root().join("target/api-types-export");
    let _ = std::fs::remove_dir_all(&scratch);
    server_api_types::export::export_roots(&scratch)?;
    let written = read_directory(&scratch);
    let _ = std::fs::remove_dir_all(&scratch);
    server_api_types::export::assemble(written?)
}

fn api_types(check: bool) -> Result<(), String> {
    let expected = expected_files()?;
    let directory = repository_root().join("server/shared/api/shapes");
    let actual = read_directory(&directory)?;
    if check {
        let missing: Vec<&String> = expected
            .keys()
            .filter(|name| !actual.contains_key(*name))
            .collect();
        let stray: Vec<&String> = actual
            .keys()
            .filter(|name| !expected.contains_key(*name))
            .collect();
        let changed: Vec<&String> = expected
            .iter()
            .filter(|(name, content)| actual.get(*name).is_some_and(|found| found != *content))
            .map(|(name, _)| name)
            .collect();
        if missing.is_empty() && stray.is_empty() && changed.is_empty() {
            return Ok(());
        }
        return Err(format!(
            "server/shared/api/shapes is out of date (missing: {missing:?}, stray: {stray:?}, changed: {changed:?}); run cargo xtask api-types and commit the result"
        ));
    }
    std::fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    for name in actual.keys().filter(|name| !expected.contains_key(*name)) {
        std::fs::remove_file(directory.join(name)).map_err(|error| format!("{name}: {error}"))?;
    }
    for (name, content) in &expected {
        std::fs::write(directory.join(name), content)
            .map_err(|error| format!("{name}: {error}"))?;
    }
    println!("wrote {} files to server/shared/api/shapes", expected.len());
    Ok(())
}

#[tokio::main]
async fn main() {
    let args = match Cli::parse() {
        Cli::Sqlx { args } => args,
        Cli::ApiTypes { check } => {
            if let Err(error) = api_types(check) {
                eprintln!("error: {error}");
                std::process::exit(1);
            }
            return;
        }
    };
    sqlx_cli::maybe_apply_dotenv();
    sqlx::any::install_default_drivers();
    let options = sqlx_cli::Opt::parse_from(std::iter::once("sqlx".to_owned()).chain(args));
    if let Err(error) = sqlx_cli::run(options).await {
        eprintln!("error: {error:#}");
        std::process::exit(1);
    }
}
