// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: 0BSD

use super::*;

/// Exact decimal arithmetic, with no exponent-sized buffer or floating point.
pub(super) fn integer(s: &str, kind: Kind, field: &'static str) -> Result<Value> {
    let raw = s.as_bytes();
    if !token::number_grammar(raw) {
        return Err(error(Reason::invalid_number, field));
    }
    let negative = raw.first() == Some(&b'-');
    let start = usize::from(negative);
    let exp_at = raw
        .iter()
        .position(|b| matches!(b, b'e' | b'E'))
        .unwrap_or(raw.len());
    let mantissa = &raw[start..exp_at];
    let mut exponent = 0i64;
    if exp_at < raw.len() {
        let tail = &raw[exp_at + 1..];
        let sign = tail.first() == Some(&b'-');
        for b in tail
            .iter()
            .skip(usize::from(matches!(tail.first(), Some(b'+' | b'-'))))
        {
            exponent = (exponent * 10 + i64::from(b - b'0')).min(1_000_000_000);
        }
        if sign {
            exponent = -exponent;
        }
    }
    let fractional = mantissa
        .iter()
        .position(|b| *b == b'.')
        .map_or(0, |p| mantissa.len() - p - 1) as i64;
    let scale = exponent - fractional;
    let mut digits = 0i64;
    let mut trailing = 0i64;
    let mut nonzero = false;
    for b in mantissa.iter().filter(|b| **b != b'.') {
        if *b != b'0' {
            nonzero = true;
            trailing = 0;
        } else {
            trailing += 1;
        }
        if nonzero {
            digits += 1;
        }
    }
    let mut value = 0u64;
    if nonzero {
        if scale < 0 && trailing < -scale {
            return Err(error(Reason::invalid_number, field));
        }
        let length = digits + scale;
        if length > 20 {
            return Err(error(Reason::integer_out_of_range, field));
        }
        let keep = if scale < 0 { digits + scale } else { digits };
        let mut seen = false;
        let mut used = 0;
        for b in mantissa.iter().filter(|b| **b != b'.') {
            if *b != b'0' {
                seen = true;
            }
            if seen && used < keep {
                value = value
                    .checked_mul(10)
                    .and_then(|v| v.checked_add(u64::from(b - b'0')))
                    .ok_or(error(Reason::integer_out_of_range, field))?;
                used += 1;
            }
        }
        for _ in 0..scale.max(0) {
            value = value
                .checked_mul(10)
                .ok_or(error(Reason::integer_out_of_range, field))?;
        }
    }
    let signed = matches!(kind, Kind::Enum(_) | Kind::Int32 | Kind::Int64);
    let max = match kind {
        Kind::Uint32 => u64::from(u32::MAX),
        Kind::Int32 | Kind::Enum(_) => {
            if negative {
                2_147_483_648
            } else {
                2_147_483_647
            }
        }
        Kind::Int64 => {
            if negative {
                1 << 63
            } else {
                i64::MAX as u64
            }
        }
        _ => u64::MAX,
    };
    if value > max || (negative && value != 0 && !signed) {
        return Err(error(Reason::integer_out_of_range, field));
    }
    let signed_value = if negative {
        value.wrapping_neg() as i64
    } else {
        value as i64
    };
    Ok(match kind {
        Kind::Uint32 => Value::Uint32(value as u32),
        Kind::Uint64 => Value::Uint64(value),
        Kind::Int32 => Value::Int32(signed_value as i32),
        Kind::Int64 => Value::Int64(signed_value),
        _ => Value::Enum(signed_value as i32),
    })
}

pub(super) fn base64(
    s: &str,
    limit: usize,
    field: &'static str,
    collect: bool,
) -> Result<Option<Value>> {
    let raw = s.as_bytes();
    let unpadded = raw.iter().rposition(|b| *b != b'=').map_or(0, |p| p + 1);
    let decoded = unpadded * 6 / 8;
    if limit != 0 && decoded > limit {
        return Err(error(Reason::byte_limit_exceeded, field));
    }
    let padding = raw.len() - unpadded;
    if unpadded % 4 == 1
        || padding > 2
        || (padding != 0 && (!raw.len().is_multiple_of(4) || padding != (4 - unpadded % 4) % 4))
    {
        return Err(error(Reason::invalid_base64, field));
    }
    let mut standard = false;
    let mut url = false;
    let mut bits = 0u32;
    let mut count = 0;
    let mut bytes = Vec::new();
    for b in &raw[..unpadded] {
        let value = match b {
            b'A'..=b'Z' => b - b'A',
            b'a'..=b'z' => b - b'a' + 26,
            b'0'..=b'9' => b - b'0' + 52,
            b'+' => {
                standard = true;
                62
            }
            b'/' => {
                standard = true;
                63
            }
            b'-' => {
                url = true;
                62
            }
            b'_' => {
                url = true;
                63
            }
            _ => return Err(error(Reason::invalid_base64, field)),
        };
        if standard && url {
            return Err(error(Reason::invalid_base64, field));
        }
        bits = (bits << 6) | u32::from(value);
        count += 6;
        if count >= 8 {
            count -= 8;
            if collect {
                bytes.push((bits >> count) as u8);
            }
        }
    }
    Ok(if collect {
        Some(Value::Bytes(bytes))
    } else {
        None
    })
}

pub(super) fn date_time(s: &str, field: &'static str) -> Result<Projection> {
    fn parse(s: &[u8], at: usize, len: usize) -> Option<i64> {
        s.get(at..at + len)?.iter().try_fold(0, |n, b| {
            if b.is_ascii_digit() {
                Some(n * 10 + i64::from(b - b'0'))
            } else {
                None
            }
        })
    }
    let fail = error(Reason::invalid_timestamp, field);
    let b = s.as_bytes();
    let (year, month, day, hour, minute, second) = (
        parse(b, 0, 4).ok_or(fail)?,
        parse(b, 5, 2).ok_or(fail)?,
        parse(b, 8, 2).ok_or(fail)?,
        parse(b, 11, 2).ok_or(fail)?,
        parse(b, 14, 2).ok_or(fail)?,
        parse(b, 17, 2).ok_or(fail)?,
    );
    if b.get(4) != Some(&b'-')
        || b.get(7) != Some(&b'-')
        || !matches!(b.get(10), Some(b'T' | b't'))
        || b.get(13) != Some(&b':')
        || b.get(16) != Some(&b':')
        || !(1..=12).contains(&month)
        || hour > 23
        || minute > 59
        || second > 59
    {
        return Err(fail);
    }
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    if day < 1 || day > days[month as usize - 1] {
        return Err(fail);
    }
    let mut pos = 19;
    let mut nanos = 0i32;
    if b.get(pos) == Some(&b'.') {
        pos += 1;
        let start = pos;
        while matches!(b.get(pos), Some(b'0'..=b'9')) {
            if pos - start == 9 {
                return Err(fail);
            }
            nanos = nanos * 10 + i32::from(b[pos] - b'0');
            pos += 1;
        }
        if pos == start {
            return Err(fail);
        }
        for _ in pos - start..9 {
            nanos *= 10;
        }
    }
    let offset = match b.get(pos) {
        Some(b'Z' | b'z') if pos + 1 == b.len() => 0,
        Some(b'+' | b'-') if pos + 6 == b.len() && b.get(pos + 3) == Some(&b':') => {
            let h = parse(b, pos + 1, 2).ok_or(fail)?;
            let m = parse(b, pos + 4, 2).ok_or(fail)?;
            if h > 23 || m > 59 {
                return Err(fail);
            }
            (h * 3600 + m * 60) * if b[pos] == b'-' { -1 } else { 1 }
        }
        _ => return Err(fail),
    };
    // Gregorian civil date to days relative to 1970-01-01, including year zero.
    let y = year - i64::from(month <= 2);
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let m = month + if month > 2 { -3 } else { 9 };
    let doy = (153 * m + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let seconds =
        (era * 146097 + doe - 719468) * 86400 + hour * 3600 + minute * 60 + second - offset;
    timestamp(seconds, nanos, field)?;
    Ok(Projection {
        message: MessageKind::Timestamp,
        fields: vec![(1, Value::Int64(seconds)), (2, Value::Int32(nanos))],
    })
}
