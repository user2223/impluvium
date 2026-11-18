// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: 0BSD

use super::*;
mod content;
mod token;
use token::{Reader, Token};

pub(super) fn scan(
    message: MessageKind,
    bytes: &[u8],
    limit: InputLimit,
    collect: bool,
) -> Result<Projection> {
    let mut parser = Parser {
        reader: Reader { bytes, pos: 0 },
        work: Work::default(),
        limit,
        collect,
    };
    if !matches!(parser.reader.token("")?, Token::Object) {
        return Err(error(Reason::invalid_json, ""));
    }
    let output = parser.object(message, 1)?;
    parser.reader.space();
    if parser.reader.pos != bytes.len() {
        return Err(error(Reason::invalid_json, ""));
    }
    Ok(output)
}

struct Parser<'a> {
    reader: Reader<'a>,
    work: Work,
    limit: InputLimit,
    collect: bool,
}
impl<'a> Parser<'a> {
    fn depth(&self, depth: usize, field: &'static str) -> Result<()> {
        if depth > limits::MAX_DEPTH {
            Err(error(Reason::nesting_too_deep, field))
        } else {
            Ok(())
        }
    }
    fn object(&mut self, message: MessageKind, depth: usize) -> Result<Projection> {
        let rules = message.fields();
        let mut names = BTreeSet::new();
        let mut seen = [false; 16];
        let mut oneof = false;
        let mut output = Projection {
            message,
            fields: Vec::new(),
        };
        if self.reader.eat(b'}') {
            return Ok(output);
        }
        loop {
            let name = self.reader.text("")?;
            self.reader.need(b':', "")?;
            // A member name and its colon are read before the repetition check;
            // the value is read afterwards. This fixes the duplicate-name versus
            // malformed-value ambiguity in encoding.md §3.10's token wording.
            if !names.insert(name) {
                return Err(error(Reason::duplicate_json_key, ""));
            }
            let known = rules
                .iter()
                .enumerate()
                .find(|(_, r)| name.eq_str(r.name) || name.eq_str(r.json_name));
            if let Some((index, rule)) = known {
                let prior = seen.get_mut(index).ok_or(error(Reason::invalid_json, ""))?;
                if rule.occurrence == Occurrence::Oneof {
                    if oneof {
                        return Err(error(Reason::oneof_conflict, ""));
                    }
                    oneof = true;
                } else if *prior {
                    return Err(error(Reason::duplicate_field, rule.diagnostic));
                }
                *prior = true;
                self.reader.space();
                let start = self.reader.pos;
                let tok = self.reader.token(rule.diagnostic)?;
                if !matches!(tok, Token::Null) {
                    match rule.occurrence {
                        Occurrence::Repeated => {
                            if !matches!(tok, Token::Array) {
                                return Err(error(Reason::json_type_mismatch, rule.diagnostic));
                            }
                            self.depth(depth + 1, rule.diagnostic)?;
                            self.array(rule, depth + 1, &mut output)?;
                        }
                        Occurrence::Map => {
                            if !matches!(tok, Token::Object) {
                                return Err(error(Reason::json_type_mismatch, rule.diagnostic));
                            }
                            self.depth(depth + 1, rule.diagnostic)?;
                            self.map(rule, depth + 1, &mut output)?;
                        }
                        _ => {
                            self.type_and_depth(rule.kind, tok, depth, rule.diagnostic)?;
                            let value_limit = if rule.diagnostic == "Envelope.payload" {
                                self.limit.payload_bytes
                            } else {
                                rule.value_limit
                            };
                            if let Some(value) =
                                self.value(rule.kind, tok, depth, rule.diagnostic, value_limit)?
                            {
                                output.fields.push((rule.number, value));
                            }
                        }
                    }
                }
                if rule.encoded_limit != 0 && self.reader.pos - start > rule.encoded_limit {
                    return Err(error(Reason::byte_limit_exceeded, rule.diagnostic));
                }
            } else {
                let tok = self.reader.token("")?;
                self.unknown(tok, depth)?;
            }
            if self.reader.eat(b'}') {
                break;
            }
            self.reader.need(b',', "")?;
        }
        Ok(output)
    }
    fn array(&mut self, rule: &FieldRule, depth: usize, output: &mut Projection) -> Result<()> {
        if self.reader.eat(b']') {
            return Ok(());
        }
        let mut count = 0;
        loop {
            let tok = self.reader.token(rule.diagnostic)?;
            self.type_and_depth(rule.kind, tok, depth, rule.diagnostic)?;
            self.work.element(rule, &mut count)?;
            if let Some(value) =
                self.value(rule.kind, tok, depth, rule.diagnostic, rule.value_limit)?
            {
                output.fields.push((rule.number, value));
            }
            if self.reader.eat(b']') {
                break;
            }
            self.reader.need(b',', rule.diagnostic)?;
        }
        Ok(())
    }
    fn map(&mut self, rule: &FieldRule, depth: usize, output: &mut Projection) -> Result<()> {
        let Kind::Map { key, value } = rule.kind else {
            return Err(error(Reason::json_type_mismatch, rule.diagnostic));
        };
        let mut names = BTreeSet::new();
        let mut count = 0;
        if self.reader.eat(b'}') {
            return Ok(());
        }
        loop {
            let name = self.reader.text(rule.diagnostic)?;
            self.reader.need(b':', rule.diagnostic)?;
            if !names.insert(name) {
                return Err(error(Reason::duplicate_json_key, ""));
            }
            let tok = self.reader.token(rule.diagnostic)?;
            self.type_and_depth(Kind::String, tok, depth, rule.diagnostic)?;
            self.work.element(rule, &mut count)?;
            if name.len > key {
                return Err(error(Reason::byte_limit_exceeded, rule.diagnostic));
            }
            if let Some(Value::String(v)) =
                self.value(Kind::String, tok, depth, rule.diagnostic, value)?
            {
                output
                    .fields
                    .push((rule.number, Value::MapEntry(name.decoded(), v)));
            }
            if self.reader.eat(b'}') {
                break;
            }
            self.reader.need(b',', rule.diagnostic)?;
        }
        Ok(())
    }
    fn type_and_depth(
        &self,
        kind: Kind,
        tok: Token<'a>,
        depth: usize,
        field: &'static str,
    ) -> Result<()> {
        let fits = matches!(
            (kind, tok),
            (
                Kind::String | Kind::Bytes | Kind::Message(MessageKind::Timestamp),
                Token::String(_)
            ) | (
                Kind::Uint32 | Kind::Uint64 | Kind::Int32 | Kind::Int64 | Kind::Enum(_),
                Token::String(_) | Token::Number(_)
            ) | (Kind::Bool, Token::Bool(_))
        ) || matches!((kind, tok), (Kind::Message(m), Token::Object) if m != MessageKind::Timestamp);
        if !fits {
            return Err(error(Reason::json_type_mismatch, field));
        }
        if matches!(tok, Token::Object | Token::Array) {
            self.depth(depth + 1, field)?;
        }
        Ok(())
    }
    fn value(
        &mut self,
        kind: Kind,
        tok: Token<'a>,
        depth: usize,
        field: &'static str,
        limit: usize,
    ) -> Result<Option<Value>> {
        match (kind, tok) {
            (Kind::String, Token::String(s)) => {
                if limit != 0 && s.len > limit {
                    return Err(error(Reason::byte_limit_exceeded, field));
                }
                Ok(if self.collect {
                    Some(Value::String(s.decoded()))
                } else {
                    None
                })
            }
            (Kind::Bytes, Token::String(s)) => {
                // Check decoded base64 length before allocating even its resolved text.
                let mut padding = 0;
                for c in s.chars() {
                    if c == '=' {
                        padding += 1;
                    } else {
                        padding = 0;
                    }
                }
                if limit != 0 && (s.len - padding) * 6 / 8 > limit {
                    return Err(error(Reason::byte_limit_exceeded, field));
                }
                content::base64(&s.decoded(), limit, field, self.collect)
            }
            (Kind::Bool, Token::Bool(b)) => Ok(if self.collect {
                Some(Value::Bool(b))
            } else {
                None
            }),
            (Kind::Enum(values), Token::String(s)) => {
                let known = values.iter().find(|(name, _)| s.eq_str(name));
                Ok(if self.collect {
                    Some(match known {
                        Some((_, n)) => Value::Enum(*n),
                        None => Value::UnknownEnumName(s.decoded()),
                    })
                } else {
                    None
                })
            }
            (
                kind @ (Kind::Uint32 | Kind::Uint64 | Kind::Int32 | Kind::Int64),
                Token::String(s),
            ) => {
                let value = content::integer(&s.decoded(), kind, field)?;
                Ok(if self.collect { Some(value) } else { None })
            }
            (kind, Token::Number(s)) => {
                let value = content::integer(s, kind, field)?;
                Ok(if self.collect { Some(value) } else { None })
            }
            (Kind::Message(MessageKind::Timestamp), Token::String(s)) => {
                let timestamp = content::date_time(&s.decoded(), field)?;
                Ok(if self.collect {
                    Some(Value::Message(timestamp))
                } else {
                    None
                })
            }
            (Kind::Message(message), Token::Object) => {
                let message = self.object(message, depth + 1)?;
                Ok(if self.collect {
                    Some(Value::Message(message))
                } else {
                    None
                })
            }
            _ => Err(error(Reason::json_type_mismatch, field)),
        }
    }
    fn unknown(&mut self, tok: Token<'a>, depth: usize) -> Result<()> {
        match tok {
            Token::Object => {
                self.depth(depth + 1, "")?;
                let mut names = BTreeSet::new();
                if self.reader.eat(b'}') {
                    return Ok(());
                }
                loop {
                    let name = self.reader.text("")?;
                    self.reader.need(b':', "")?;
                    if !names.insert(name) {
                        return Err(error(Reason::duplicate_json_key, ""));
                    }
                    let tok = self.reader.token("")?;
                    self.unknown(tok, depth + 1)?;
                    if self.reader.eat(b'}') {
                        break;
                    }
                    self.reader.need(b',', "")?;
                }
            }
            Token::Array => {
                self.depth(depth + 1, "")?;
                if self.reader.eat(b']') {
                    return Ok(());
                }
                loop {
                    let tok = self.reader.token("")?;
                    self.unknown(tok, depth + 1)?;
                    if self.reader.eat(b']') {
                        break;
                    }
                    self.reader.need(b',', "")?;
                }
            }
            _ => {}
        }
        Ok(())
    }
}
