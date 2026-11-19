// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Executable composition; startup stays unavailable until its guards exist.

mod config;

use std::process::ExitCode;

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args_os()
        .skip(1)
        .map(|argument| argument.to_string_lossy().into_owned())
        .collect();
    let words: Vec<&str> = arguments.iter().map(String::as_str).collect();
    match words.as_slice() {
        ["config", "check"] => config::run_check(),
        ["config", "reference"] => config::run_reference(),
        _ => {
            eprintln!("impluvium-server: startup is not available yet");
            eprintln!("available commands: config check, config reference");
            ExitCode::FAILURE
        }
    }
}
