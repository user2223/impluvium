// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! The process environment as seen by the configuration.
//!
//! Every variable is read by name at one literal call. The only enumeration of
//! the environment is `present_names`, which keeps names and drops values; it
//! finds product variables that the configuration does not know.

use std::collections::BTreeMap;
use std::env;

use super::Issue;
use super::fields::FIELDS;

/// Prefix of every variable owned by the project.
const PREFIX: &str = "IMPLUVIUM_";

/// Product variables that exist for builds only. A container image may carry
/// them at run time; they are ignored rather than reported.
pub const BUILD_ONLY: [&str; 3] = [
    "IMPLUVIUM_VERSION",
    "IMPLUVIUM_GIT_REF",
    "IMPLUVIUM_BUILD_PROFILE",
];

/// Values of the known variables plus the names that are present.
#[derive(Default, Clone)]
pub struct Source {
    values: BTreeMap<String, String>,
    present: Vec<String>,
    not_unicode: Vec<String>,
}

impl std::fmt::Debug for Source {
    /// Shows names only: the values may hold secrets.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let names: Vec<&String> = self.values.keys().collect();
        formatter
            .debug_struct("Source")
            .field("names", &names)
            .finish_non_exhaustive()
    }
}

impl Source {
    /// A source made of the given pairs, for tests and tools.
    pub fn from_pairs(pairs: &[(&str, &str)]) -> Self {
        let mut source = Self::default();
        for (name, value) in pairs {
            source.present.push((*name).to_owned());
            source
                .values
                .insert((*name).to_owned(), (*value).to_owned());
        }
        source
    }

    /// Reads every known variable from the process environment.
    pub fn from_process() -> Self {
        let mut s = Self {
            present: present_names(),
            ..Self::default()
        };
        s.record("IMPLUVIUM_EXTERNAL_URL", env::var("IMPLUVIUM_EXTERNAL_URL"));
        s.record("IMPLUVIUM_DATA_DIR", env::var("IMPLUVIUM_DATA_DIR"));
        s.record("IMPLUVIUM_LIBRARY_DIR", env::var("IMPLUVIUM_LIBRARY_DIR"));
        s.record(
            "IMPLUVIUM_TRUSTED_PROXIES",
            env::var("IMPLUVIUM_TRUSTED_PROXIES"),
        );
        s.record("IMPLUVIUM_INSECURE_LAN", env::var("IMPLUVIUM_INSECURE_LAN"));
        s.record("DATABASE_URL", env::var("DATABASE_URL"));
        s.record("DATABASE_URL_FILE", env::var("DATABASE_URL_FILE"));
        s.record("IMPLUVIUM_DB_POOL_MAX", env::var("IMPLUVIUM_DB_POOL_MAX"));
        s.record("IMPLUVIUM_DB_POOL_MIN", env::var("IMPLUVIUM_DB_POOL_MIN"));
        s.record(
            "IMPLUVIUM_DB_TIMEOUT_SECS",
            env::var("IMPLUVIUM_DB_TIMEOUT_SECS"),
        );
        s.record("IMPLUVIUM_SPA_DIR", env::var("IMPLUVIUM_SPA_DIR"));
        s.record("IMPLUVIUM_LOG", env::var("IMPLUVIUM_LOG"));
        s.record("IMPLUVIUM_LOG_FORMAT", env::var("IMPLUVIUM_LOG_FORMAT"));
        s.record("IMPLUVIUM_BACKUP_MODE", env::var("IMPLUVIUM_BACKUP_MODE"));
        s.record(
            "IMPLUVIUM_BACKUP_COMMAND",
            env::var("IMPLUVIUM_BACKUP_COMMAND"),
        );
        s.record(
            "IMPLUVIUM_BACKUP_TIMEOUT_SECS",
            env::var("IMPLUVIUM_BACKUP_TIMEOUT_SECS"),
        );
        s.record("IMPLUVIUM_LISTEN", env::var("IMPLUVIUM_LISTEN"));
        s.record(
            "IMPLUVIUM_ACCEPT_NEW_DATABASE",
            env::var("IMPLUVIUM_ACCEPT_NEW_DATABASE"),
        );
        s
    }

    fn record(&mut self, name: &'static str, value: Result<String, env::VarError>) {
        match value {
            Ok(text) => {
                self.values.insert(name.to_owned(), text);
            }
            Err(env::VarError::NotUnicode(_)) => self.not_unicode.push(name.to_owned()),
            Err(env::VarError::NotPresent) => {}
        }
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        self.values.get(name).map(String::as_str)
    }

    pub fn present(&self) -> &[String] {
        &self.present
    }

    /// Known variables whose value is not valid UTF-8.
    pub fn not_unicode(&self) -> &[String] {
        &self.not_unicode
    }
}

pub(super) fn present_names() -> Vec<String> {
    env::vars_os()
        .filter_map(|(name, _)| name.into_string().ok())
        .collect()
}

fn known_names() -> Vec<&'static str> {
    let mut names = Vec::new();
    for field in FIELDS {
        names.push(field.variable);
        if let Some(file) = field.file_variable {
            names.push(file);
        }
    }
    names
}

/// Reports every product variable that is not a known name. The comparison
/// ignores case so that a lower-case spelling is reported too.
pub fn unknown_variables(present: &[String]) -> Vec<Issue> {
    let known = known_names();
    let candidates: Vec<&str> = known
        .iter()
        .copied()
        .filter(|name| name.starts_with(PREFIX))
        .collect();
    let mut issues = Vec::new();
    for name in present {
        let upper = name.to_ascii_uppercase();
        if !upper.starts_with(PREFIX) {
            continue;
        }
        if known.contains(&name.as_str()) || BUILD_ONLY.contains(&name.as_str()) {
            continue;
        }
        let mut best: Option<(usize, &str)> = None;
        for &candidate in &candidates {
            let gap = distance(&upper, candidate);
            if best.is_none_or(|(smallest, _)| gap < smallest) {
                best = Some((gap, candidate));
            }
        }
        let message = match best {
            Some((gap, candidate)) if gap <= 3 => {
                format!("is not a known configuration variable; did you mean {candidate}?")
            }
            _ => "is not a known configuration variable".to_owned(),
        };
        issues.push(Issue::new(name, message));
    }
    issues
}

/// Edit distance between two names.
fn distance(left: &str, right: &str) -> usize {
    let left = left.as_bytes();
    let right = right.as_bytes();
    let mut previous: Vec<usize> = (0..=right.len()).collect();
    for (i, a) in left.iter().enumerate() {
        let mut current = vec![i + 1];
        for (j, b) in right.iter().enumerate() {
            let substitution = previous[j] + usize::from(a != b);
            let insertion = current[j] + 1;
            let deletion = previous[j + 1] + 1;
            current.push(substitution.min(insertion).min(deletion));
        }
        previous = current;
    }
    previous[right.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distance_counts_edits() {
        assert_eq!(distance("", ""), 0);
        assert_eq!(distance("abc", "abc"), 0);
        assert_eq!(distance("abc", "abd"), 1);
        assert_eq!(distance("abc", "ab"), 1);
        assert_eq!(distance("", "abc"), 3);
    }

    #[test]
    fn every_known_variable_has_exactly_one_literal_read() {
        let text = include_str!("env.rs");
        let production = text.split("#[cfg(test)]").next().unwrap();
        let known = known_names();
        for name in &known {
            let read = format!("env::var(\"{name}\")");
            assert_eq!(production.matches(&read).count(), 1, "read of {name}");
        }
        let all_reads = production.matches("env::var(\"").count();
        assert_eq!(
            all_reads,
            known.len(),
            "reads that are not in the field table"
        );
    }

    #[test]
    fn debug_output_shows_names_and_no_values() {
        let source = Source::from_pairs(&[("DATABASE_URL", "postgres://u:debug-marker@db/x")]);
        let text = format!("{source:?}");
        assert!(text.contains("DATABASE_URL"));
        assert!(!text.contains("debug-marker"));
    }

    #[test]
    fn each_name_is_paired_with_its_own_read() {
        let text = include_str!("env.rs");
        let production = text.split("#[cfg(test)]").next().unwrap();
        let compact: String = production.split_whitespace().collect();
        let compact = compact.replace(",)", ")");
        let known = known_names();
        for name in &known {
            let pair = format!("s.record(\"{name}\",env::var(\"{name}\"));");
            assert_eq!(compact.matches(&pair).count(), 1, "pairing of {name}");
        }
        assert_eq!(compact.matches("s.record(").count(), known.len());
    }

    #[test]
    fn unknown_product_variables_are_named_with_a_hint() {
        let typo = FIELDS[0].variable.replace("EXTERNAL", "EXTRNAL");
        let lower = FIELDS[1].variable.to_ascii_lowercase();
        let present = vec![
            typo.clone(),
            lower.clone(),
            "IMPLUVIUM_OIDC_CLIENT_ID".to_owned(),
            FIELDS[0].variable.to_owned(),
            "PATH".to_owned(),
            "IMPLUVIUM_VERSION".to_owned(),
        ];
        let issues = unknown_variables(&present);
        let names: Vec<&str> = issues.iter().map(|i| i.variable.as_str()).collect();
        assert_eq!(
            names,
            [typo.as_str(), lower.as_str(), "IMPLUVIUM_OIDC_CLIENT_ID"]
        );
        assert!(issues[0].message.contains(FIELDS[0].variable));
        assert!(issues[1].message.contains(FIELDS[1].variable));
    }
}
