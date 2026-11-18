// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: 0BSD

//! RFC 9110 single byte ranges. Decimal overflow is saturated only after syntax
//! checking, so arbitrarily large suffixes and upper bounds clip correctly.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ByteRange {
    pub first: u64,
    pub last: u64,
}
fn decimal(text: &str) -> Option<u64> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some(text.bytes().fold(0u64, |n, b| {
        n.saturating_mul(10).saturating_add(u64::from(b - b'0'))
    }))
}
pub fn normalize(header: &str, size: u64) -> Option<ByteRange> {
    if size == 0 {
        return None;
    }
    let (unit, value) = header.trim_matches([' ', '\t']).split_once('=')?;
    if !unit.eq_ignore_ascii_case("bytes") {
        return None;
    }
    let (first, last) = value.trim_matches([' ', '\t']).split_once('-')?;
    if first.is_empty() {
        let suffix = decimal(last)?;
        if suffix == 0 {
            return None;
        }
        return Some(ByteRange {
            first: size.saturating_sub(suffix),
            last: size - 1,
        });
    }
    let first = decimal(first)?;
    let last = if last.is_empty() {
        size - 1
    } else {
        decimal(last)?.min(size - 1)
    };
    (first <= last && first < size).then_some(ByteRange { first, last })
}
