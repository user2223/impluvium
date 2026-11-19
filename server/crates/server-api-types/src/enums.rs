// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! String enums that serde and TypeScript agree on.
//!
//! `api_enums!` declares the Rust enums and their lists of variants from one
//! variant list. serde fixes the wire values (renames included); the
//! TypeScript union is derived from the same attributes, and the export adds a
//! runtime constant array of the wire values for the UI (see
//! [`crate::export`]). Every enum declared in `api_enums!` is in the runtime list.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// The wire values and name of an enum, for the runtime constants.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeEnum {
    /// The Rust and TypeScript type name.
    pub name: &'static str,
    /// The serde wire values in declaration order.
    pub values: Vec<String>,
    /// The TypeScript union that ts-rs derives for the enum.
    pub ts_union: String,
}

/// Declares the enums and, from the same list, [`runtime_enums`], so no enum can be left out of
/// the runtime list or of the test that compares the TypeScript union with the serde values.
macro_rules! api_enums {
    ($(
        $(#[$meta:meta])*
        $name:ident { $($(#[$variant_meta:meta])* $variant:ident),+ $(,)? }
    )+) => {
        $(
            $(#[$meta])*
            #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
            pub enum $name {
                $($(#[$variant_meta])* $variant,)+
            }

            impl $name {
                /// Every variant in declaration order.
                pub const ALL: &'static [$name] = &[$($name::$variant),+];
                /// The type name in Rust and TypeScript.
                pub const NAME: &'static str = stringify!($name);

                /// The serde wire value of the variant.
                pub fn wire_value(self) -> String {
                    match serde_json::to_value(self) {
                        Ok(serde_json::Value::String(text)) => text,
                        _ => unreachable!("api enums serialise as strings"),
                    }
                }
            }
        )+

        /// The enums that get a runtime constant array in TypeScript.
        pub fn runtime_enums() -> Vec<RuntimeEnum> {
            let config = ts_rs::Config::new();
            vec![$(RuntimeEnum {
                name: $name::NAME,
                values: $name::ALL.iter().map(|value| value.wire_value()).collect(),
                ts_union: <$name as TS>::inline(&config),
            }),+]
        }
    };
}

api_enums!(
    /// The platform a game is built for.
    Platform {
        Windows,
        Linux,
        #[serde(rename = "macOS")]
        MacOs,
    }
);

#[cfg(test)]
pub(crate) mod tests {
    use std::collections::HashSet;

    use super::*;

    /// The quoted members of a ts-rs string union such as `"A" | "B"`.
    pub(crate) fn union_members(union: &str) -> HashSet<String> {
        union
            .split('|')
            .map(|part| part.trim().to_owned())
            .collect()
    }

    #[test]
    fn platform_serde_values_are_the_ones_of_the_ui() {
        assert_eq!(
            Platform::ALL
                .iter()
                .map(|p| p.wire_value())
                .collect::<Vec<_>>(),
            ["Windows", "Linux", "macOS"]
        );
        assert_eq!(
            serde_json::from_str::<Platform>(r#""macOS""#).unwrap(),
            Platform::MacOs
        );
        assert!(serde_json::from_str::<Platform>(r#""MacOs""#).is_err());
        assert!(serde_json::from_str::<Platform>(r#""windows""#).is_err());
    }

    #[test]
    fn every_api_enum_has_a_typescript_union_with_its_serde_values() {
        let listed = runtime_enums();
        assert!(!listed.is_empty());
        for entry in listed {
            let expected: HashSet<String> =
                entry.values.iter().map(|v| format!("\"{v}\"")).collect();
            assert_eq!(expected.len(), entry.values.len(), "{}", entry.name);
            assert_eq!(union_members(&entry.ts_union), expected, "{}", entry.name);
        }
    }

    #[test]
    fn runtime_list_matches_the_enum() {
        let listed = runtime_enums();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].name, "Platform");
        assert_eq!(listed[0].values, ["Windows", "Linux", "macOS"]);
    }
}
