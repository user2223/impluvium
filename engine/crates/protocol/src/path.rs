// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: 0BSD

//! Host-independent base paths, lexical symlinks and the explicit Windows profile.

use crate::proto::impluvium::manifest::v3::Entry;
use crate::{
    decode::{Diagnostic, Reason, error},
    manifest::entry_model_limits,
};
use std::collections::HashMap;

#[path = "path_unicode16.rs"]
mod unicode16;

pub const MAX_PATH_BYTES: usize = 4096;
pub const MAX_COMPONENT_BYTES: usize = 255;

fn common(value: &str, field: &'static str) -> Result<(), Diagnostic> {
    use Reason::*;
    let failure = if value.is_empty() {
        Some(path_empty)
    } else if value.len() > MAX_PATH_BYTES {
        Some(byte_limit_exceeded)
    } else if value
        .chars()
        .any(|c| c <= '\u{1f}' || ('\u{7f}'..='\u{9f}').contains(&c) || c == '\\')
    {
        Some(path_forbidden_character)
    } else if value.starts_with('/') {
        Some(path_absolute)
    } else if value.len() >= 2
        && value.as_bytes()[0].is_ascii_alphabetic()
        && value.as_bytes()[1] == b':'
    {
        Some(path_drive_prefix)
    } else if value.split('/').any(str::is_empty) {
        Some(path_empty_component)
    } else {
        None
    };
    match failure {
        Some(reason) => Err(error(reason, field)),
        None => Ok(()),
    }
}

/// Pass an empty diagnostic field for a path outside a schema message.
pub fn validate_base(value: &str, field: &'static str) -> Result<(), Diagnostic> {
    common(value, field)?;
    if value.split('/').any(|c| c == "." || c == "..") {
        return Err(error(Reason::path_dot_component, field));
    }
    component_lengths(value, field)
}

fn component_lengths(value: &str, field: &'static str) -> Result<(), Diagnostic> {
    if value.split('/').any(|c| c.len() > MAX_COMPONENT_BYTES) {
        Err(error(Reason::path_component_too_long, field))
    } else {
        Ok(())
    }
}

/// The link's own path must have passed the base profile. No filesystem is consulted.
pub fn validate_symlink(link_path: &str, target: &str) -> Result<(), Diagnostic> {
    const FIELD: &str = "Entry.symlink_target";
    common(target, FIELD)?;
    component_lengths(target, FIELD)?;
    let mut depth = link_path.bytes().filter(|b| *b == b'/').count();
    for component in target.split('/') {
        match component {
            "." => (),
            ".." => {
                depth = depth
                    .checked_sub(1)
                    .ok_or(error(Reason::symlink_escape, FIELD))?;
            }
            _ => depth += 1,
        }
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WindowsVariant {
    Publication,
    Installation,
}

/// Unicode 16.0.0 simple uppercase followed by C/S simple folding, with no normalization.
pub fn collision_key(value: &str) -> String {
    fn map(table: &[(u32, u32)], c: u32) -> u32 {
        table
            .binary_search_by_key(&c, |&(from, _)| from)
            .map(|i| table[i].1)
            .unwrap_or(c)
    }
    value
        .chars()
        .map(|c| {
            char::from_u32(map(unicode16::FOLD, map(unicode16::UPPER, c as u32)))
                .expect("Unicode mappings are scalar values")
        })
        .collect()
}

pub fn validate_windows_component(component: &str, field: &'static str) -> Result<(), Diagnostic> {
    use Reason::*;
    if component.contains(['<', '>', ':', '"', '|', '?', '*']) {
        return Err(error(windows_forbidden_character, field));
    }
    if component.ends_with(['.', ' ']) {
        return Err(error(windows_trailing_dot_or_space, field));
    }
    let stem = component
        .split('.')
        .next()
        .unwrap_or("")
        .trim_end_matches(' ');
    let upper = stem.to_ascii_uppercase();
    let device = ["CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$"].contains(&upper.as_str());
    let port = upper
        .strip_prefix("COM")
        .or_else(|| upper.strip_prefix("LPT"))
        .is_some_and(|suffix| {
            let mut chars = suffix.chars();
            matches!(chars.next(), Some('0'..='9' | '¹' | '²' | '³')) && chars.next().is_none()
        });
    if device || port {
        Err(error(windows_reserved_name, field))
    } else {
        Ok(())
    }
}

/// Run after base manifest validation. Ancestor nodes share a component trie, so
/// collision detection is linear in path bytes rather than pairwise in paths.
/// Publication returns one warning per symlink; installation rejects symlinks.
pub fn validate_windows(
    entries: &[Entry],
    variant: WindowsVariant,
) -> Result<Vec<Diagnostic>, Diagnostic> {
    entry_model_limits(entries)?;
    for entry in entries {
        validate_base(&entry.path, "Entry.path")?;
    }
    let mut nodes: HashMap<(usize, String), (&str, usize)> = HashMap::new();
    let mut warnings = Vec::new();
    for entry in entries {
        for component in entry.path.split('/') {
            validate_windows_component(component, "Entry.path")?;
        }
        let mut parent = 0;
        for component in entry.path.split('/') {
            let key = (parent, collision_key(component));
            if let Some(&(original, id)) = nodes.get(&key) {
                if original != component {
                    return Err(error(Reason::windows_case_collision, "Entry.path"));
                }
                parent = id;
            } else {
                let id = nodes.len() + 1;
                nodes.insert(key, (component, id));
                parent = id;
            }
        }
        if entry.r#type.to_i32() == 2 {
            if variant == WindowsVariant::Installation {
                return Err(error(Reason::unsupported_symlink, "Entry.type"));
            }
            warnings.push(error(Reason::windows_symlink, "Entry.type"));
        }
    }
    Ok(warnings)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_model_entry_limits_precede_profile_checks() {
        let mut entries = vec![Entry {
            path: "NUL".into(),
            hash: vec![0; 129],
            ..Default::default()
        }];
        for variant in [WindowsVariant::Publication, WindowsVariant::Installation] {
            assert_eq!(
                validate_windows(&entries, variant),
                Err(error(Reason::byte_limit_exceeded, "Entry.hash"))
            );
        }
        entries[0].hash.clear();
        entries[0].symlink_target = "a".repeat(MAX_PATH_BYTES + 1);
        assert_eq!(
            validate_windows(&entries, WindowsVariant::Publication),
            Err(error(Reason::byte_limit_exceeded, "Entry.symlink_target"))
        );
    }

    #[test]
    fn unicode_tables_match_published_asset() {
        let data: serde_json::Value =
            serde_json::from_str(include_str!("../../../protocol/testdata/unicode16.json"))
                .unwrap();
        assert_eq!(data["version"], "16.0.0");
        let (upper, fold) = (unicode16::UPPER, unicode16::FOLD);
        for (key, table) in [("upper", upper), ("fold", fold)] {
            let expected: Vec<_> = data[key]
                .as_array()
                .unwrap()
                .iter()
                .map(|pair| {
                    (
                        pair[0].as_u64().unwrap() as u32,
                        pair[1].as_u64().unwrap() as u32,
                    )
                })
                .collect();
            assert_eq!(table, expected);
            assert!(table.windows(2).all(|w| w[0].0 < w[1].0));
            assert!(
                table
                    .iter()
                    .all(|&(a, b)| char::from_u32(a).is_some() && char::from_u32(b).is_some())
            );
        }
    }
}
