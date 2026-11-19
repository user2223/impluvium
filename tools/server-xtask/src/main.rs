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
//! TLS is not compiled in, so a database URL with `sslmode=require` or `verify-*` fails.
//!
//! The arguments after the second `--` reach the Cargo build that `prepare` starts, so that build is
//! locked too. The alias `xtask` in `.cargo/config.toml` runs this package with `--locked`.

use clap::Parser;

#[derive(Parser, Debug)]
#[command(bin_name = "cargo xtask")]
enum Cli {
    /// Run the pinned sqlx-cli; everything after `sqlx` is passed through.
    Sqlx {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
}

#[tokio::main]
async fn main() {
    sqlx_cli::maybe_apply_dotenv();
    sqlx::any::install_default_drivers();
    let Cli::Sqlx { args } = Cli::parse();
    let options = sqlx_cli::Opt::parse_from(std::iter::once("sqlx".to_owned()).chain(args));
    if let Err(error) = sqlx_cli::run(options).await {
        eprintln!("error: {error:#}");
        std::process::exit(1);
    }
}
