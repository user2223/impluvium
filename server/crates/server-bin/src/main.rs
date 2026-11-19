// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Executable composition: the commands and the guarded start of the server.

mod backup;
mod config;
mod data_dir;
mod emoji;
mod instance_keys;
mod logging;
mod readiness;
mod routes;
#[cfg(test)]
mod scratch;
mod shutdown;
mod spa;
mod start;

use std::process::ExitCode;

// The strict and the lenient key file functions; the start path uses only part of them.
#[allow(dead_code)]
mod keys;

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args_os()
        .skip(1)
        .map(|argument| argument.to_string_lossy().into_owned())
        .collect();
    let words: Vec<&str> = arguments.iter().map(String::as_str).collect();
    match words.as_slice() {
        ["start"] => start::run(),
        ["backup"] => backup::run(),
        ["config", "check"] => config::run_check(),
        ["config", "reference"] => config::run_reference(),
        ["routes", "--json"] => routes::run_json(),
        _ => {
            eprintln!("impluvium-server: unknown command");
            eprintln!(
                "usage: impluvium-server start | backup | config check | config reference | routes --json"
            );
            ExitCode::FAILURE
        }
    }
}
