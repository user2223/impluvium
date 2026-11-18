// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: 0BSD

use super::*;
use core::cmp::Ordering;

/// Borrowed names let duplicate detection retain spans, rather than allocating
/// decoded strings for arbitrary unknown names. The body's bound also bounds
/// the number and total span lengths of names retained in the active objects.
#[derive(Clone, Copy)]
pub(super) struct Text<'a> {
    pub raw: &'a [u8],
    pub len: usize,
}
impl<'a> Text<'a> {
    pub fn decoded(self) -> String {
        let mut out = String::with_capacity(self.len);
        let mut pos = 0;
        while pos < self.raw.len() {
            if let Ok(c) = character(self.raw, &mut pos) {
                out.push(c);
            } else {
                break;
            }
        }
        out
    }
    pub fn chars(self) -> impl Iterator<Item = char> + 'a {
        let mut pos = 0;
        std::iter::from_fn(move || {
            if pos == self.raw.len() {
                None
            } else {
                character(self.raw, &mut pos).ok()
            }
        })
    }
    pub fn eq_str(self, s: &str) -> bool {
        self.chars().eq(s.chars())
    }
}
impl PartialEq for Text<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other).is_eq()
    }
}
impl Eq for Text<'_> {}
impl PartialOrd for Text<'_> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Text<'_> {
    fn cmp(&self, other: &Self) -> Ordering {
        self.chars().cmp(other.chars())
    }
}

#[derive(Clone, Copy)]
pub(super) enum Token<'a> {
    String(Text<'a>),
    Number(&'a str),
    Bool(bool),
    Null,
    Object,
    Array,
}

pub(super) struct Reader<'a> {
    pub bytes: &'a [u8],
    pub pos: usize,
}
impl<'a> Reader<'a> {
    pub fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }
    pub fn space(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\n' | b'\r' | b'\t')) {
            self.pos += 1;
        }
    }
    pub fn eat(&mut self, b: u8) -> bool {
        self.space();
        if self.peek() == Some(b) {
            self.pos += 1;
            true
        } else {
            false
        }
    }
    pub fn need(&mut self, b: u8, field: &'static str) -> Result<()> {
        if self.eat(b) {
            Ok(())
        } else {
            Err(error(Reason::invalid_json, field))
        }
    }
    pub fn text(&mut self, field: &'static str) -> Result<Text<'a>> {
        self.space();
        if self.peek() != Some(b'"') {
            return Err(error(Reason::invalid_json, field));
        }
        self.pos += 1;
        let start = self.pos;
        let mut len = 0;
        while self.peek() != Some(b'"') {
            let c = character(self.bytes, &mut self.pos)
                .map_err(|_| error(Reason::invalid_json, field))?;
            len += c.len_utf8();
        }
        let raw = &self.bytes[start..self.pos];
        self.pos += 1;
        Ok(Text { raw, len })
    }
    pub fn token(&mut self, field: &'static str) -> Result<Token<'a>> {
        self.space();
        let token = match self.peek() {
            Some(b'{') => {
                self.pos += 1;
                return Ok(Token::Object);
            }
            Some(b'[') => {
                self.pos += 1;
                return Ok(Token::Array);
            }
            Some(b'"') => Token::String(self.text(field)?),
            Some(b't') => {
                self.literal(b"true", field)?;
                Token::Bool(true)
            }
            Some(b'f') => {
                self.literal(b"false", field)?;
                Token::Bool(false)
            }
            Some(b'n') => {
                self.literal(b"null", field)?;
                Token::Null
            }
            Some(b'-' | b'0'..=b'9') => {
                let start = self.pos;
                while matches!(
                    self.peek(),
                    Some(b'0'..=b'9' | b'.' | b'+' | b'-' | b'e' | b'E')
                ) {
                    self.pos += 1;
                }
                let s = core::str::from_utf8(&self.bytes[start..self.pos])
                    .map_err(|_| error(Reason::invalid_json, field))?;
                if !number_grammar(s.as_bytes()) {
                    return Err(error(Reason::invalid_json, field));
                }
                Token::Number(s)
            }
            _ => return Err(error(Reason::invalid_json, field)),
        };
        if !matches!(
            self.peek(),
            None | Some(b' ' | b'\t' | b'\r' | b'\n' | b',' | b'}' | b']')
        ) {
            return Err(error(Reason::invalid_json, field));
        }
        Ok(token)
    }
    fn literal(&mut self, literal: &[u8], field: &'static str) -> Result<()> {
        if !self.bytes[self.pos..].starts_with(literal) {
            return Err(error(Reason::invalid_json, field));
        }
        self.pos += literal.len();
        Ok(())
    }
}

fn hex(bytes: &[u8], pos: &mut usize) -> core::result::Result<u32, ()> {
    let mut value = 0;
    for _ in 0..4 {
        let b = *bytes.get(*pos).ok_or(())?;
        *pos += 1;
        let digit = match b {
            b'0'..=b'9' => b - b'0',
            b'a'..=b'f' => b - b'a' + 10,
            b'A'..=b'F' => b - b'A' + 10,
            _ => return Err(()),
        };
        value = value * 16 + u32::from(digit);
    }
    Ok(value)
}
fn character(bytes: &[u8], pos: &mut usize) -> core::result::Result<char, ()> {
    let b = *bytes.get(*pos).ok_or(())?;
    *pos += 1;
    if b == b'\\' {
        let escape = *bytes.get(*pos).ok_or(())?;
        *pos += 1;
        return Ok(match escape {
            b'"' => '"',
            b'\\' => '\\',
            b'/' => '/',
            b'b' => '\u{8}',
            b'f' => '\u{c}',
            b'n' => '\n',
            b'r' => '\r',
            b't' => '\t',
            b'u' => {
                let mut value = hex(bytes, pos)?;
                if (0xd800..=0xdbff).contains(&value) {
                    if bytes.get(*pos..*pos + 2) != Some(b"\\u") {
                        return Err(());
                    }
                    *pos += 2;
                    let low = hex(bytes, pos)?;
                    if !(0xdc00..=0xdfff).contains(&low) {
                        return Err(());
                    }
                    value = 0x10000 + ((value - 0xd800) << 10) + low - 0xdc00;
                }
                char::from_u32(value).ok_or(())?
            }
            _ => return Err(()),
        });
    }
    if b < 0x20 || b == b'"' {
        return Err(());
    }
    if b < 0x80 {
        return Ok(char::from(b));
    }
    let width = match b {
        0xc2..=0xdf => 2,
        0xe0..=0xef => 3,
        0xf0..=0xf4 => 4,
        _ => return Err(()),
    };
    let start = *pos - 1;
    let raw = bytes.get(start..start + width).ok_or(())?;
    let c = core::str::from_utf8(raw)
        .map_err(|_| ())?
        .chars()
        .next()
        .ok_or(())?;
    *pos = start + width;
    Ok(c)
}

pub(super) fn number_grammar(s: &[u8]) -> bool {
    let mut p = usize::from(s.first() == Some(&b'-'));
    match s.get(p) {
        Some(b'0') => p += 1,
        Some(b'1'..=b'9') => {
            p += 1;
            while matches!(s.get(p), Some(b'0'..=b'9')) {
                p += 1;
            }
        }
        _ => return false,
    }
    if s.get(p) == Some(&b'.') {
        p += 1;
        let start = p;
        while matches!(s.get(p), Some(b'0'..=b'9')) {
            p += 1;
        }
        if p == start {
            return false;
        }
    }
    if matches!(s.get(p), Some(b'e' | b'E')) {
        p += 1;
        if matches!(s.get(p), Some(b'+' | b'-')) {
            p += 1;
        }
        let start = p;
        while matches!(s.get(p), Some(b'0'..=b'9')) {
            p += 1;
        }
        if p == start {
            return false;
        }
    }
    p == s.len()
}
