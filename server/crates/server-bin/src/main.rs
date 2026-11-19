// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Executable composition; startup stays unavailable until its guards exist.

mod backup;
mod config;
mod logging;
mod routes;
mod shutdown;

use std::process::ExitCode;

// Not yet reachable from the startup path; the module is exercised by its tests.
#[allow(dead_code)]
mod keys;

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args_os()
        .skip(1)
        .map(|argument| argument.to_string_lossy().into_owned())
        .collect();
    let words: Vec<&str> = arguments.iter().map(String::as_str).collect();
    match words.as_slice() {
        ["backup"] => backup::run(),
        ["config", "check"] => config::run_check(),
        ["config", "reference"] => config::run_reference(),
        ["routes", "--json"] => routes::run_json(),
        _ => {
            eprintln!("impluvium-server: startup is not available yet");
            eprintln!("available commands: backup, config check, config reference, routes --json");
            ExitCode::FAILURE
        }
    }
}
