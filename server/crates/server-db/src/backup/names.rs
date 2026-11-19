// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Names of backup sets, UTC time stamps and the manifest text.

use std::time::SystemTime;

/// Longest label of a migration version inside a set name.
const MAX_LABEL: usize = 40;

/// What a set was made for.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BackupKind {
    /// Made on request by the operator.
    Manual,
    /// Made before migrating from one schema version to another. The labels are
    /// version names; characters outside `[0-9A-Za-z_.-]` become `_` in file names.
    PreMigration { from: String, to: String },
}

/// `(year, month, day, hour, minute, second)` in UTC.
fn utc_parts(seconds: u64) -> (i64, i64, i64, u64, u64, u64) {
    let days = (seconds / 86_400) as i64;
    let rest = seconds % 86_400;
    // Days since 0000-03-01 (civil-from-days algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day, rest / 3600, rest % 3600 / 60, rest % 60)
}

fn seconds_since_epoch(time: SystemTime) -> u64 {
    time.duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

/// `yyyymmddThhmmssZ`, the form used in set names.
pub(crate) fn compact_stamp(time: SystemTime) -> String {
    let (y, mo, d, h, mi, s) = utc_parts(seconds_since_epoch(time));
    format!("{y:04}{mo:02}{d:02}T{h:02}{mi:02}{s:02}Z")
}

/// `yyyy-mm-ddThh:mm:ssZ`, the form used in the manifest.
pub(crate) fn iso_stamp(time: SystemTime) -> String {
    let (y, mo, d, h, mi, s) = utc_parts(seconds_since_epoch(time));
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z")
}

/// Keeps `[0-9A-Za-z_.-]`, replaces everything else with `_` and bounds the length.
pub(crate) fn sanitise_label(label: &str) -> String {
    let mut clean: String = label
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-') {
                c
            } else {
                '_'
            }
        })
        .take(MAX_LABEL)
        .collect();
    if clean.is_empty() {
        clean.push('_');
    }
    clean
}

/// The name of a set before a counter is added for a second set in the same second.
pub(crate) fn base_name(kind: &BackupKind, stamp: &str) -> String {
    match kind {
        BackupKind::Manual => format!("manual-{stamp}"),
        BackupKind::PreMigration { from, to } => format!(
            "pre-{}-{}-{stamp}",
            sanitise_label(from),
            sanitise_label(to)
        ),
    }
}

/// A set name taken apart. Newer sets sort higher.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct ParsedName {
    pub stamp: String,
    pub counter: u32,
    pub pre_migration: bool,
}

fn is_stamp(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() == 16
        && bytes[..8].iter().all(u8::is_ascii_digit)
        && bytes[8] == b'T'
        && bytes[9..15].iter().all(u8::is_ascii_digit)
        && bytes[15] == b'Z'
}

/// Recognises `manual-…` and `pre-…` names, with an optional `-<n>` counter (n >= 2)
/// after the time stamp. Anything else is not a set name.
pub(crate) fn parse_name(name: &str) -> Option<ParsedName> {
    let pre_migration = if name.starts_with("pre-") {
        true
    } else if name.starts_with("manual-") {
        false
    } else {
        return None;
    };
    let positions: Vec<usize> = name.match_indices('Z').map(|(index, _)| index).collect();
    for index in positions.into_iter().rev() {
        if index < 15 {
            continue;
        }
        let Some(stamp) = name.get(index - 15..=index) else {
            continue;
        };
        if !is_stamp(stamp) {
            continue;
        }
        let tail = name.get(index + 1..)?;
        let counter = if tail.is_empty() {
            1
        } else {
            match tail.strip_prefix('-').map(str::parse::<u32>) {
                Some(Ok(number)) if number >= 2 => number,
                _ => continue,
            }
        };
        return Some(ParsedName {
            stamp: stamp.to_owned(),
            counter,
            pre_migration,
        });
    }
    None
}

/// A JSON string literal.
pub(crate) fn json_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn at(seconds: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(seconds)
    }

    #[test]
    fn time_stamps_match_known_instants() {
        assert_eq!(compact_stamp(at(0)), "19700101T000000Z");
        assert_eq!(compact_stamp(at(1_700_000_000)), "20231114T221320Z");
        assert_eq!(compact_stamp(at(951_782_400)), "20000229T000000Z");
        assert_eq!(compact_stamp(at(4_102_444_800)), "21000101T000000Z");
        assert_eq!(iso_stamp(at(1_700_000_000)), "2023-11-14T22:13:20Z");
    }

    #[test]
    fn labels_are_sanitised_and_bounded() {
        assert_eq!(sanitise_label("0001_baseline.v2-x"), "0001_baseline.v2-x");
        assert_eq!(sanitise_label("a b/c\\d\u{e4}\n"), "a_b_c_d__");
        assert_eq!(sanitise_label(""), "_");
        assert_eq!(sanitise_label(&"x".repeat(500)).len(), MAX_LABEL);
        let kind = BackupKind::PreMigration {
            from: "../..".to_owned(),
            to: "2".to_owned(),
        };
        let name = base_name(&kind, "20250101T000000Z");
        assert_eq!(name, "pre-.._..-2-20250101T000000Z");
        assert!(!name.contains('/'));
    }

    #[test]
    fn set_names_are_parsed_back() {
        let parsed = parse_name("manual-20250101T000000Z").unwrap();
        assert_eq!((parsed.counter, parsed.pre_migration), (1, false));
        let parsed = parse_name("pre-0001-0002-20250101T000000Z-3").unwrap();
        assert_eq!(parsed.stamp, "20250101T000000Z");
        assert_eq!((parsed.counter, parsed.pre_migration), (3, true));
        // A label that itself ends in a stamp-like text still yields the real stamp.
        let parsed = parse_name("pre-a-20240101T000000Z-20250101T000000Z").unwrap();
        assert_eq!(parsed.stamp, "20250101T000000Z");
        for other in [
            "database.dump",
            ".partial-manual-20250101T000000Z-1",
            "manual-2025",
            "manual-20250101T000000Z-1",
            "manual-20250101T000000Z-x",
            "manual-20250101T000000Zx",
        ] {
            assert!(parse_name(other).is_none(), "{other}");
        }
        assert!(parse_name("pre-a-b-20250101T000001Z") > parse_name("pre-a-b-20250101T000000Z-9"));
        assert!(parse_name("pre-a-b-20250101T000000Z-2") > parse_name("pre-a-b-20250101T000000Z"));
    }
}
