// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: 0BSD

//! Data-only test JSON reader. Keeping this small reader here avoids adding
//! runtime or development dependencies to the client crate. The protocol's
//! existing lexer checks the entire profile before this reader constructs data.

use std::{collections::BTreeMap, ops::Index};
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Value {
    Null,
    Bool(bool),
    Number(String),
    String(String),
    Array(Vec<Value>),
    Object(BTreeMap<String, Value>),
}
impl Value {
    pub fn as_str(&self) -> Option<&str> {
        if let Self::String(s) = self {
            Some(s)
        } else {
            None
        }
    }
    pub fn as_u64(&self) -> Option<u64> {
        if let Self::Number(s) = self {
            s.parse().ok()
        } else {
            None
        }
    }
    pub fn as_bool(&self) -> Option<bool> {
        if let Self::Bool(b) = self {
            Some(*b)
        } else {
            None
        }
    }
    pub fn as_array(&self) -> Option<&Vec<Value>> {
        if let Self::Array(a) = self {
            Some(a)
        } else {
            None
        }
    }
    pub fn as_object(&self) -> Option<&BTreeMap<String, Value>> {
        if let Self::Object(a) = self {
            Some(a)
        } else {
            None
        }
    }
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.as_object()?.get(key)
    }
}
impl Index<&str> for Value {
    type Output = Value;
    fn index(&self, key: &str) -> &Value {
        self.get(key).unwrap_or(&Value::Null)
    }
}
impl Index<usize> for Value {
    type Output = Value;
    fn index(&self, key: usize) -> &Value {
        self.as_array()
            .and_then(|a| a.get(key))
            .unwrap_or(&Value::Null)
    }
}
impl PartialEq<&str> for Value {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == Some(*other)
    }
}
impl PartialEq<String> for Value {
    fn eq(&self, other: &String) -> bool {
        self.as_str() == Some(other.as_str())
    }
}
impl std::fmt::Display for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
pub fn s(s: impl Into<String>) -> Value {
    Value::String(s.into())
}
pub fn n(n: impl ToString) -> Value {
    Value::Number(n.to_string())
}
pub fn b(b: bool) -> Value {
    Value::Bool(b)
}
pub fn o<const N: usize>(fields: [(&str, Value); N]) -> Value {
    Value::Object(fields.into_iter().map(|(k, v)| (k.into(), v)).collect())
}

pub fn from_slice(bytes: &[u8]) -> Result<Value, String> {
    impluvium_protocol::decode::scan_json_document(
        bytes,
        impluvium_protocol::decode::InputLimit::body(1_048_576),
    )
    .map_err(|d| format!("{d:?}"))?;
    struct Reader<'a> {
        text: &'a str,
        pos: usize,
    }
    impl Reader<'_> {
        fn space(&mut self) {
            while self
                .text
                .as_bytes()
                .get(self.pos)
                .is_some_and(u8::is_ascii_whitespace)
            {
                self.pos += 1;
            }
        }
        fn eat(&mut self, byte: u8) -> bool {
            self.space();
            if self.text.as_bytes().get(self.pos) == Some(&byte) {
                self.pos += 1;
                true
            } else {
                false
            }
        }
        fn string(&mut self) -> String {
            assert!(self.eat(b'"'));
            let mut out = String::new();
            loop {
                let c = self.text[self.pos..].chars().next().unwrap();
                self.pos += c.len_utf8();
                if c == '"' {
                    return out;
                }
                if c != '\\' {
                    out.push(c);
                    continue;
                }
                let escaped = self.text.as_bytes()[self.pos];
                self.pos += 1;
                out.push(match escaped {
                    b'"' => '"',
                    b'\\' => '\\',
                    b'/' => '/',
                    b'b' => '\u{8}',
                    b'f' => '\u{c}',
                    b'n' => '\n',
                    b'r' => '\r',
                    b't' => '\t',
                    b'u' => {
                        let mut code =
                            u32::from_str_radix(&self.text[self.pos..self.pos + 4], 16).unwrap();
                        self.pos += 4;
                        if (0xd800..=0xdbff).contains(&code) {
                            self.pos += 2;
                            let low = u32::from_str_radix(&self.text[self.pos..self.pos + 4], 16)
                                .unwrap();
                            self.pos += 4;
                            code = 0x10000 + ((code - 0xd800) << 10) + low - 0xdc00;
                        }
                        char::from_u32(code).unwrap()
                    }
                    _ => unreachable!("profile checked escape"),
                });
            }
        }
        fn value(&mut self) -> Value {
            self.space();
            match self.text.as_bytes()[self.pos] {
                b'"' => Value::String(self.string()),
                b'{' => {
                    self.pos += 1;
                    let mut fields = BTreeMap::new();
                    if !self.eat(b'}') {
                        loop {
                            let name = self.string();
                            assert!(self.eat(b':'));
                            fields.insert(name, self.value());
                            if self.eat(b'}') {
                                break;
                            }
                            assert!(self.eat(b','));
                        }
                    }
                    Value::Object(fields)
                }
                b'[' => {
                    self.pos += 1;
                    let mut values = Vec::new();
                    if !self.eat(b']') {
                        loop {
                            values.push(self.value());
                            if self.eat(b']') {
                                break;
                            }
                            assert!(self.eat(b','));
                        }
                    }
                    Value::Array(values)
                }
                b't' => {
                    self.pos += 4;
                    Value::Bool(true)
                }
                b'f' => {
                    self.pos += 5;
                    Value::Bool(false)
                }
                b'n' => {
                    self.pos += 4;
                    Value::Null
                }
                _ => {
                    let start = self.pos;
                    while self.text.as_bytes().get(self.pos).is_some_and(|b| {
                        matches!(b, b'0'..=b'9' | b'-' | b'+' | b'.' | b'e' | b'E')
                    }) {
                        self.pos += 1;
                    }
                    Value::Number(self.text[start..self.pos].into())
                }
            }
        }
    }
    let mut reader = Reader {
        text: std::str::from_utf8(bytes).map_err(|e| e.to_string())?,
        pos: 0,
    };
    Ok(reader.value())
}
