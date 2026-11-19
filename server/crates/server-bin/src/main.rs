// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Executable composition; startup stays unavailable until its guards exist.

use std::process::ExitCode;

fn main() -> ExitCode {
    eprintln!("impluvium-server: startup is not available yet");
    ExitCode::FAILURE
}
