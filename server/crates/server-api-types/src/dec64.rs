// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! 64-bit integers as canonical decimal strings.
//!
//! JSON numbers are doubles in most clients and lose precision above 2^53, so
//! every 64-bit field of the web API travels as a string. The canonical form
//! is the shortest one: digits only, no leading zeros (except `"0"`), no `+`,
//! a single `-` for negative values only and never `-0`. Deserialisation
//! accepts nothing else: a JSON number, whitespace, an exponent, a fraction
//! or an out-of-range value is an error. In TypeScript the wire type is
//! `string`; `server/shared/api/dec64.ts` converts it to and from `bigint`
//! with the same rules.

use std::fmt;
use std::str::FromStr;

use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use ts_rs::TS;

/// Why a string is not a canonical 64-bit decimal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dec64Error {
    /// Not digits in canonical form (empty, sign, leading zero, other characters).
    NotCanonical,
    /// Canonical, but outside the range of the integer type.
    OutOfRange,
}

impl fmt::Display for Dec64Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NotCanonical => "not a canonical decimal string",
            Self::OutOfRange => "decimal string out of range",
        })
    }
}

impl std::error::Error for Dec64Error {}

/// Checks the canonical shape and returns the digits without the sign.
fn canonical_digits(text: &str, signed: bool) -> Result<(bool, &str), Dec64Error> {
    let (negative, digits) = match text.strip_prefix('-') {
        Some(rest) if signed && rest != "0" => (true, rest),
        Some(_) => return Err(Dec64Error::NotCanonical),
        None => (false, text),
    };
    let well_formed = !digits.is_empty()
        && digits.bytes().all(|byte| byte.is_ascii_digit())
        && (digits == "0" || !digits.starts_with('0'));
    if well_formed {
        Ok((negative, digits))
    } else {
        Err(Dec64Error::NotCanonical)
    }
}

macro_rules! dec_type {
    ($(#[$doc:meta])* $name:ident, $int:ty, $signed:expr) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, TS)]
        #[ts(type = "string")]
        pub struct $name(pub $int);

        impl $name {
            /// The wrapped integer.
            pub const fn get(self) -> $int {
                self.0
            }
        }

        impl From<$int> for $name {
            fn from(value: $int) -> Self {
                Self(value)
            }
        }

        impl From<$name> for $int {
            fn from(value: $name) -> Self {
                value.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                fmt::Display::fmt(&self.0, f)
            }
        }

        impl FromStr for $name {
            type Err = Dec64Error;

            fn from_str(text: &str) -> Result<Self, Dec64Error> {
                canonical_digits(text, $signed)?;
                text.parse::<$int>()
                    .map(Self)
                    .map_err(|_| Dec64Error::OutOfRange)
            }
        }

        impl Serialize for $name {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.collect_str(&self.0)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                struct Text;

                impl Visitor<'_> for Text {
                    type Value = $name;

                    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                        f.write_str(concat!("a canonical decimal string for ", stringify!($int)))
                    }

                    fn visit_str<E: de::Error>(self, text: &str) -> Result<$name, E> {
                        text.parse().map_err(E::custom)
                    }
                }

                deserializer.deserialize_str(Text)
            }
        }
    };
}

dec_type!(
    /// An unsigned 64-bit integer that travels as a canonical decimal string.
    DecU64, u64, false
);
dec_type!(
    /// A signed 64-bit integer that travels as a canonical decimal string.
    DecI64, i64, true
);

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Holder {
        size: DecU64,
        delta: DecI64,
    }

    fn u64_text(value: u64) -> String {
        serde_json::to_string(&DecU64(value)).unwrap()
    }

    #[test]
    fn unsigned_boundaries_round_trip() {
        for value in [0, 1, 9_007_199_254_740_993, u64::MAX] {
            let text = u64_text(value);
            assert_eq!(text, format!("\"{value}\""));
            assert_eq!(
                serde_json::from_str::<DecU64>(&text).unwrap(),
                DecU64(value)
            );
        }
    }

    #[test]
    fn signed_boundaries_round_trip() {
        for value in [i64::MIN, -9_007_199_254_740_993, -1, 0, 1, i64::MAX] {
            let text = serde_json::to_string(&DecI64(value)).unwrap();
            assert_eq!(text, format!("\"{value}\""));
            assert_eq!(
                serde_json::from_str::<DecI64>(&text).unwrap(),
                DecI64(value)
            );
        }
    }

    #[test]
    fn unsigned_refuses_everything_but_the_canonical_form() {
        let not_canonical = [
            "", "-", "-1", "-0", "00", "01", "+1", "1e3", "1E3", "1.0", " 1", "1 ", "1\n", "0x1",
            "1_000", "\u{ff11}", "NaN", "null",
        ];
        for text in not_canonical {
            assert_eq!(
                text.parse::<DecU64>(),
                Err(Dec64Error::NotCanonical),
                "{text:?}"
            );
            assert!(
                serde_json::from_value::<DecU64>(json!(text)).is_err(),
                "{text:?}"
            );
        }
        for text in ["18446744073709551616", "99999999999999999999999"] {
            assert_eq!(
                text.parse::<DecU64>(),
                Err(Dec64Error::OutOfRange),
                "{text:?}"
            );
            assert!(
                serde_json::from_value::<DecU64>(json!(text)).is_err(),
                "{text:?}"
            );
        }
    }

    #[test]
    fn signed_refuses_everything_but_the_canonical_form() {
        let not_canonical = [
            "", "-", "--1", "-0", "-00", "-01", "00", "01", "+1", "+0", "1e3", "-1e3", " -1",
            "-1 ", "- 1",
        ];
        for text in not_canonical {
            assert_eq!(
                text.parse::<DecI64>(),
                Err(Dec64Error::NotCanonical),
                "{text:?}"
            );
            assert!(
                serde_json::from_value::<DecI64>(json!(text)).is_err(),
                "{text:?}"
            );
        }
        for text in ["9223372036854775808", "-9223372036854775809"] {
            assert_eq!(
                text.parse::<DecI64>(),
                Err(Dec64Error::OutOfRange),
                "{text:?}"
            );
        }
    }

    #[test]
    fn json_numbers_and_other_types_are_refused() {
        for body in [
            "1",
            "0",
            "1.0",
            "1e3",
            "-1",
            "9007199254740993",
            "null",
            "true",
            "[]",
            "{}",
        ] {
            assert!(serde_json::from_str::<DecU64>(body).is_err(), "{body}");
            assert!(serde_json::from_str::<DecI64>(body).is_err(), "{body}");
        }
    }

    #[test]
    fn fields_round_trip_inside_a_struct() {
        let holder = Holder {
            size: DecU64(u64::MAX),
            delta: DecI64(i64::MIN),
        };
        let text = serde_json::to_string(&holder).unwrap();
        assert_eq!(
            text,
            r#"{"size":"18446744073709551615","delta":"-9223372036854775808"}"#
        );
        assert_eq!(serde_json::from_str::<Holder>(&text).unwrap(), holder);
        assert!(serde_json::from_str::<Holder>(r#"{"size":1,"delta":"1"}"#).is_err());
    }

    #[test]
    fn display_and_from_str_agree() {
        for value in [0, 7, u64::MAX] {
            assert_eq!(
                DecU64(value).to_string().parse::<DecU64>(),
                Ok(DecU64(value))
            );
        }
        for value in [i64::MIN, -7, 0, i64::MAX] {
            assert_eq!(
                DecI64(value).to_string().parse::<DecI64>(),
                Ok(DecI64(value))
            );
        }
        assert_eq!(u64::from(DecU64::from(5)), 5);
        assert_eq!(DecI64(-5).get(), -5);
    }
}
