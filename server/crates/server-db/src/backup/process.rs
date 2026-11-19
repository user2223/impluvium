// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Running external programs with an argument list, a cleared environment and a deadline.

use std::ffi::OsString;
use std::io::{ErrorKind, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use super::error::ToolFailure;

/// How often a running program is checked.
const POLL: Duration = Duration::from_millis(25);
/// Output of a version probe is cut at this many bytes.
const MAX_PROBE_OUTPUT: u64 = 4096;

/// A point in time after which running programs are killed.
pub(crate) struct Deadline(Instant);

impl Deadline {
    pub(crate) fn after(limit: Duration) -> Self {
        Self(Instant::now() + limit)
    }

    fn remaining(&self) -> Option<Duration> {
        self.0
            .checked_duration_since(Instant::now())
            .filter(|left| !left.is_zero())
    }
}

/// A command for a PostgreSQL tool: no shell, a cleared environment with only `PATH` and
/// `LANG=C`, no input and no output. Connection settings are added by the caller
/// as environment variables.
pub(crate) fn tool_command(program: &Path) -> Command {
    let path =
        std::env::var_os("PATH").unwrap_or_else(|| OsString::from("/usr/local/bin:/usr/bin:/bin"));
    let mut command = Command::new(program);
    command
        .env_clear()
        .env("PATH", path)
        .env("LANG", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    command
}

fn spawn(command: &mut Command) -> Result<Child, ToolFailure> {
    // A script that was written a moment ago can be reported as busy while another thread
    // forks; trying again a few times is the established remedy.
    let mut attempt = 0;
    loop {
        match command.spawn() {
            Ok(child) => return Ok(child),
            Err(error) if error.kind() == ErrorKind::ExecutableFileBusy && attempt < 20 => {
                attempt += 1;
                std::thread::sleep(Duration::from_millis(25));
            }
            Err(error) if error.kind() == ErrorKind::NotFound => return Err(ToolFailure::Missing),
            Err(error) => return Err(ToolFailure::CannotStart(error.kind())),
        }
    }
}

fn status_result(status: ExitStatus) -> Result<(), ToolFailure> {
    if status.success() {
        Ok(())
    } else {
        match status.code() {
            Some(code) => Err(ToolFailure::Exit(code)),
            None => Err(ToolFailure::Signal),
        }
    }
}

fn stop(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

/// Runs the command to its end or kills it at the deadline.
pub(crate) fn run(mut command: Command, deadline: &Deadline) -> Result<(), ToolFailure> {
    let mut child = spawn(&mut command)?;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status_result(status),
            Ok(None) => {}
            Err(error) => {
                stop(&mut child);
                return Err(ToolFailure::CannotStart(error.kind()));
            }
        }
        match deadline.remaining() {
            Some(left) => std::thread::sleep(left.min(POLL)),
            None => {
                stop(&mut child);
                return Err(ToolFailure::Timeout);
            }
        }
    }
}

/// Runs a command whose standard output is collected through a scratch file. The file is
/// created new and removed again.
pub(crate) fn run_capture(
    mut command: Command,
    scratch: &Path,
    deadline: &Deadline,
) -> Result<String, ToolFailure> {
    let file = super::files::create_new_private(scratch)
        .map_err(|error| ToolFailure::CannotStart(error.kind()))?;
    command.stdout(Stdio::from(file));
    let result = run(command, deadline);
    let output = std::fs::File::open(scratch).and_then(|file| {
        let mut text = Vec::new();
        file.take(MAX_PROBE_OUTPUT).read_to_end(&mut text)?;
        Ok(text)
    });
    let _ = std::fs::remove_file(scratch);
    result?;
    match output {
        Ok(bytes) => Ok(String::from_utf8_lossy(&bytes).into_owned()),
        Err(error) => Err(ToolFailure::CannotStart(error.kind())),
    }
}

/// The major version from the output of `--version`: the first word that starts with a
/// digit, for example `18` from `pg_dump (PostgreSQL) 18.7`.
pub(crate) fn parse_major(output: &str) -> Option<u32> {
    output.split_whitespace().find_map(|word| {
        let digits: String = word.chars().take_while(char::is_ascii_digit).collect();
        digits.parse().ok()
    })
}

/// The first line of the version output, reduced to printable ASCII and bounded, for the manifest.
pub(crate) fn version_line(output: &str) -> String {
    output
        .lines()
        .next()
        .unwrap_or("")
        .chars()
        .filter(|c| c.is_ascii() && !c.is_ascii_control())
        .take(120)
        .collect()
}

/// Looks a program up in the absolute directories of `PATH`.
pub(crate) fn find_in_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for directory in std::env::split_paths(&path) {
        if !directory.is_absolute() {
            continue;
        }
        let candidate = directory.join(name);
        if is_executable_file(&candidate) {
            return Some(candidate);
        }
    }
    None
}

#[cfg(unix)]
fn is_executable_file(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable_file(path: &Path) -> bool {
    path.is_file()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_versions_are_read_from_the_version_text() {
        assert_eq!(
            parse_major("pg_dump (PostgreSQL) 18.7 (Debian 18.7-1.pgdg13+1)\n"),
            Some(18)
        );
        assert_eq!(parse_major("pg_dump (PostgreSQL) 17beta1"), Some(17));
        assert_eq!(parse_major("pg_dump (PostgreSQL) 9.6.24"), Some(9));
        assert_eq!(parse_major("no version here"), None);
        assert_eq!(version_line("pg_dump 18.7\nsecond"), "pg_dump 18.7");
    }
}
