// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! RFC 3339 UTC timestamps with millisecond precision.
//!
//! The wire form is always `YYYY-MM-DDTHH:MM:SS.mmmZ`: UTC, the letter `Z`,
//! exactly three fractional digits, years 0000 to 9999. Reading accepts exactly
//! this form and nothing else: no offsets (`+00:00`), no lower-case letters,
//! no missing or longer fractions, no leap seconds, no spaces. Finer
//! precision of the source is cut by the caller before it gets here.
//! In TypeScript the field is a `string`. The parser and formatter are small
//! and strict on purpose, so the server needs no date/time crate for them.

use std::fmt;
use std::str::FromStr;

use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use ts_rs::TS;

const MILLIS_PER_DAY: i64 = 86_400_000;
/// Days from 0000-01-01 to 1970-01-01 in the proleptic Gregorian calendar.
const DAYS_TO_EPOCH: i64 = 719_528;
/// Length of the wire form.
const TEXT_LEN: usize = 24;

/// Smallest timestamp: `0000-01-01T00:00:00.000Z`.
pub const MIN_UNIX_MILLIS: i64 = -DAYS_TO_EPOCH * MILLIS_PER_DAY;
/// Largest timestamp: `9999-12-31T23:59:59.999Z`.
pub const MAX_UNIX_MILLIS: i64 = 253_402_300_799_999;

/// Why a string is not a timestamp of the wire form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimestampError;

impl fmt::Display for TimestampError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("not a timestamp of the form YYYY-MM-DDTHH:MM:SS.mmmZ")
    }
}

impl std::error::Error for TimestampError {}

/// A point in time, in milliseconds since the Unix epoch, within the years 0000 to 9999.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, TS)]
#[ts(type = "string")]
pub struct Timestamp(i64);

impl Timestamp {
    /// The timestamp for a number of milliseconds since the epoch, if it is in range.
    pub fn from_unix_millis(millis: i64) -> Option<Self> {
        (MIN_UNIX_MILLIS..=MAX_UNIX_MILLIS)
            .contains(&millis)
            .then_some(Self(millis))
    }

    /// Milliseconds since the Unix epoch.
    pub const fn unix_millis(self) -> i64 {
        self.0
    }
}

/// Day number (1970-01-01 is 0) of a calendar date.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let shifted_month = if month > 2 { month - 3 } else { month + 9 };
    let day_of_year = (153 * shifted_month + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// Calendar date (year, month, day) of a day number.
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let days = days + 719_468;
    let era = days.div_euclid(146_097);
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

fn is_leap(year: i64) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

fn days_in_month(year: i64, month: i64) -> i64 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ if is_leap(year) => 29,
        _ => 28,
    }
}

impl fmt::Display for Timestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (year, month, day) = civil_from_days(self.0.div_euclid(MILLIS_PER_DAY));
        let millis_of_day = self.0.rem_euclid(MILLIS_PER_DAY);
        let (seconds_of_day, millis) = (millis_of_day / 1_000, millis_of_day % 1_000);
        write!(
            f,
            "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{millis:03}Z",
            seconds_of_day / 3_600,
            seconds_of_day % 3_600 / 60,
            seconds_of_day % 60
        )
    }
}

/// The number written by `len` ASCII digits starting at `start`.
fn digits(bytes: &[u8], start: usize, len: usize) -> Result<i64, TimestampError> {
    bytes[start..start + len]
        .iter()
        .try_fold(0_i64, |sum, &byte| {
            if byte.is_ascii_digit() {
                Ok(sum * 10 + i64::from(byte - b'0'))
            } else {
                Err(TimestampError)
            }
        })
}

impl FromStr for Timestamp {
    type Err = TimestampError;

    fn from_str(text: &str) -> Result<Self, TimestampError> {
        let bytes = text.as_bytes();
        let separators = [
            (4, b'-'),
            (7, b'-'),
            (10, b'T'),
            (13, b':'),
            (16, b':'),
            (19, b'.'),
            (23, b'Z'),
        ];
        if bytes.len() != TEXT_LEN || separators.iter().any(|&(at, byte)| bytes[at] != byte) {
            return Err(TimestampError);
        }
        let year = digits(bytes, 0, 4)?;
        let month = digits(bytes, 5, 2)?;
        let day = digits(bytes, 8, 2)?;
        let hour = digits(bytes, 11, 2)?;
        let minute = digits(bytes, 14, 2)?;
        let second = digits(bytes, 17, 2)?;
        let millis = digits(bytes, 20, 3)?;
        let valid = (1..=12).contains(&month)
            && (1..=days_in_month(year, month)).contains(&day)
            && hour < 24
            && minute < 60
            && second < 60;
        if !valid {
            return Err(TimestampError);
        }
        let seconds_of_day = hour * 3_600 + minute * 60 + second;
        Ok(Self(
            days_from_civil(year, month, day) * MILLIS_PER_DAY + seconds_of_day * 1_000 + millis,
        ))
    }
}

impl Serialize for Timestamp {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Timestamp {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Text;

        impl Visitor<'_> for Text {
            type Value = Timestamp;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a timestamp of the form YYYY-MM-DDTHH:MM:SS.mmmZ")
            }

            fn visit_str<E: de::Error>(self, text: &str) -> Result<Timestamp, E> {
                text.parse().map_err(E::custom)
            }
        }

        deserializer.deserialize_str(Text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(millis: i64) -> String {
        Timestamp::from_unix_millis(millis).unwrap().to_string()
    }

    #[test]
    fn known_values_format_as_expected() {
        assert_eq!(at(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(at(1_700_000_000_123), "2023-11-14T22:13:20.123Z");
        assert_eq!(at(-1), "1969-12-31T23:59:59.999Z");
        assert_eq!(at(951_782_400_000), "2000-02-29T00:00:00.000Z");
        assert_eq!(at(MIN_UNIX_MILLIS), "0000-01-01T00:00:00.000Z");
        assert_eq!(at(MAX_UNIX_MILLIS), "9999-12-31T23:59:59.999Z");
    }

    #[test]
    fn range_limits_are_enforced() {
        assert!(Timestamp::from_unix_millis(MIN_UNIX_MILLIS - 1).is_none());
        assert!(Timestamp::from_unix_millis(MAX_UNIX_MILLIS + 1).is_none());
        assert!(Timestamp::from_unix_millis(i64::MAX).is_none());
        assert_eq!(Timestamp::from_unix_millis(5).unwrap().unix_millis(), 5);
    }

    #[test]
    fn parsing_reverses_formatting() {
        for millis in [
            MIN_UNIX_MILLIS,
            -86_400_001,
            -1,
            0,
            1,
            951_782_400_000,
            1_709_164_800_000,
            1_700_000_000_123,
            MAX_UNIX_MILLIS,
        ] {
            let text = at(millis);
            assert_eq!(
                text.parse::<Timestamp>().unwrap().unix_millis(),
                millis,
                "{text}"
            );
        }
    }

    #[test]
    fn every_day_converts_both_ways() {
        let first = MIN_UNIX_MILLIS / MILLIS_PER_DAY;
        let last = MAX_UNIX_MILLIS / MILLIS_PER_DAY;
        let mut previous = None;
        for day in first..=last {
            let (year, month, date) = civil_from_days(day);
            assert_eq!(days_from_civil(year, month, date), day);
            assert!((1..=days_in_month(year, month)).contains(&date));
            if let Some(before) = previous {
                assert!((year, month, date) > before);
            }
            previous = Some((year, month, date));
        }
    }

    #[test]
    fn leap_years_follow_the_gregorian_rule() {
        for (year, leap) in [
            (2024, true),
            (2000, true),
            (1900, false),
            (2023, false),
            (0, true),
        ] {
            let text = format!("{year:04}-02-29T00:00:00.000Z");
            assert_eq!(text.parse::<Timestamp>().is_ok(), leap, "{text}");
        }
    }

    #[test]
    fn everything_but_the_wire_form_is_refused() {
        let refused = [
            "",
            "2024-01-01",
            "2024-01-01T00:00:00Z",
            "2024-01-01T00:00:00.0Z",
            "2024-01-01T00:00:00.00Z",
            "2024-01-01T00:00:00.0000Z",
            "2024-01-01T00:00:00.000z",
            "2024-01-01t00:00:00.000Z",
            "2024-01-01 00:00:00.000Z",
            "2024-01-01T00:00:00.000+00:00",
            "2024-01-01T00:00:00.000",
            "2024-01-01T00:00:00,000Z",
            " 2024-01-01T00:00:00.000Z",
            "2024-01-01T00:00:00.000Z ",
            "+024-01-01T00:00:00.000Z",
            "-001-01-01T00:00:00.000Z",
            "2024-13-01T00:00:00.000Z",
            "2024-00-01T00:00:00.000Z",
            "2024-01-00T00:00:00.000Z",
            "2024-01-32T00:00:00.000Z",
            "2023-02-29T00:00:00.000Z",
            "2024-04-31T00:00:00.000Z",
            "2024-01-01T24:00:00.000Z",
            "2024-01-01T00:60:00.000Z",
            "2024-01-01T00:00:60.000Z",
            "2024-01-01T00:00:00.00\u{e9}Z",
            "\u{ff12}024-01-01T00:00:00.000Z",
        ];
        for text in refused {
            assert!(text.parse::<Timestamp>().is_err(), "{text:?}");
        }
    }

    #[test]
    fn serde_uses_the_string_form_only() {
        let value: Timestamp = serde_json::from_str(r#""2023-11-14T22:13:20.123Z""#).unwrap();
        assert_eq!(value.unix_millis(), 1_700_000_000_123);
        assert_eq!(
            serde_json::to_string(&value).unwrap(),
            r#""2023-11-14T22:13:20.123Z""#
        );
        for body in ["1700000000123", "null", "true", r#""2023-11-14""#] {
            assert!(serde_json::from_str::<Timestamp>(body).is_err(), "{body}");
        }
    }
}
