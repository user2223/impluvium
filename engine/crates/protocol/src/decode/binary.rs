// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: 0BSD

use super::*;

pub(super) struct Cursor<'a> {
    pub bytes: &'a [u8],
    pub position: usize,
}
pub(super) struct Field<'a> {
    pub number: u32,
    pub wire: u8,
    pub integer: u64,
    pub body: &'a [u8],
    pub encoded: usize,
    pub rule: Option<&'static FieldRule>,
}
impl<'a> Cursor<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }
    pub fn varint(&mut self, field: &'static str) -> Result<u64> {
        let mut value = 0;
        for i in 0..10 {
            let byte = *self
                .bytes
                .get(self.position)
                .ok_or(error(Reason::malformed_wire, field))?;
            self.position += 1;
            if i == 9 && byte > 1 {
                return Err(error(Reason::malformed_wire, field));
            }
            value |= u64::from(byte & 0x7f) << (7 * i);
            if byte & 0x80 == 0 {
                return Ok(value);
            }
        }
        Err(error(Reason::malformed_wire, field))
    }
    fn take(&mut self, length: u64, field: &'static str) -> Result<&'a [u8]> {
        if length > (self.bytes.len() - self.position) as u64 {
            return Err(error(Reason::malformed_wire, field));
        }
        let end = self.position + length as usize;
        let body = &self.bytes[self.position..end];
        self.position = end;
        Ok(body)
    }
    // Steps 1–3 run even for unknown fields; known-wire validation precedes framing.
    pub fn next(&mut self, rules: &'static [FieldRule]) -> Result<Option<Field<'a>>> {
        if self.position == self.bytes.len() {
            return Ok(None);
        }
        let start = self.position;
        let tag = self.varint("")?;
        let number = tag >> 3;
        if number == 0 || number > 536_870_911 {
            return Err(error(Reason::malformed_wire, ""));
        }
        let number = number as u32;
        let rule = rules.iter().find(|r| r.number == number);
        let name = rule.map_or("", |r| r.diagnostic);
        let wire = (tag & 7) as u8;
        if !matches!(wire, 0 | 1 | 2 | 5) {
            return Err(error(Reason::malformed_wire, name));
        }
        if let Some(r) = rule {
            let scalar = matches!(
                r.kind,
                Kind::Uint32
                    | Kind::Uint64
                    | Kind::Int32
                    | Kind::Int64
                    | Kind::Bool
                    | Kind::Enum(_)
            );
            let packed = r.occurrence == Occurrence::Repeated
                && matches!(r.kind, Kind::Uint32 | Kind::Enum(_));
            if wire != if scalar { 0 } else { 2 } && !(packed && wire == 2) {
                return Err(error(Reason::wrong_wire_type, name));
            }
        }
        let mut integer = 0;
        let body = match wire {
            0 => {
                integer = self.varint(name)?;
                &self.bytes[self.position..self.position]
            }
            1 => self.take(8, name)?,
            2 => {
                let len = self.varint(name)?;
                self.take(len, name)?
            }
            5 => self.take(4, name)?,
            _ => return Err(error(Reason::malformed_wire, name)),
        };
        Ok(Some(Field {
            number,
            wire,
            integer,
            body,
            encoded: self.position - start,
            rule,
        }))
    }
}

pub(super) fn scan(
    message: MessageKind,
    bytes: &[u8],
    limit: InputLimit,
    collect: bool,
) -> Result<Projection> {
    message_scan(message, bytes, limit, collect, &mut Work::default(), 1, "")
}

fn message_scan(
    message: MessageKind,
    bytes: &[u8],
    limit: InputLimit,
    collect: bool,
    work: &mut Work,
    depth: usize,
    parent: &'static str,
) -> Result<Projection> {
    let rules = message.fields();
    // The inventory test also checks this fixed scratch capacity. It is unrelated
    // to the number of unknown fields, which are never entered into these arrays.
    let mut counts = [0usize; 16];
    let mut budgets = [0usize; 16];
    let mut maps: [BTreeSet<&[u8]>; 16] = std::array::from_fn(|_| BTreeSet::new());
    let mut oneof = false;
    let mut metadata = 0;
    let mut seconds = 0;
    let mut nanos = 0;
    let mut output = Projection {
        message,
        fields: Vec::new(),
    };
    let mut cursor = Cursor::new(bytes);
    while let Some(field) = cursor.next(rules)? {
        let Some(rule) = field.rule else {
            if message == MessageKind::Envelope {
                metadata += field.encoded;
                if metadata > limits::METADATA_BYTES {
                    return Err(error(Reason::byte_limit_exceeded, ""));
                }
            }
            continue;
        };
        let index = rules
            .iter()
            .position(|r| r.number == field.number)
            .ok_or(error(Reason::malformed_wire, ""))?;
        let count = counts
            .get_mut(index)
            .ok_or(error(Reason::malformed_wire, ""))?;
        // Step 4: default-valued occurrences count, including oneof members.
        match rule.occurrence {
            Occurrence::Oneof => {
                if oneof {
                    return Err(error(Reason::oneof_conflict, ""));
                }
                oneof = true;
            }
            Occurrence::Scalar | Occurrence::Message if *count != 0 => {
                return Err(error(
                    if rule.occurrence == Occurrence::Message {
                        Reason::split_message
                    } else {
                        Reason::duplicate_field
                    },
                    rule.diagnostic,
                ));
            }
            _ => {}
        }
        let packed = rule.occurrence == Occurrence::Repeated
            && field.wire == 2
            && matches!(rule.kind, Kind::Uint32 | Kind::Enum(_));
        if !packed {
            if matches!(rule.occurrence, Occurrence::Repeated | Occurrence::Map) {
                work.element(rule, count)?;
            } else {
                *count += 1;
            }
        }
        let value_limit = if rule.diagnostic == "Envelope.payload" {
            limit.payload_bytes
        } else {
            rule.value_limit
        };
        if value_limit != 0 && field.body.len() > value_limit {
            return Err(error(Reason::byte_limit_exceeded, rule.diagnostic));
        }
        budgets[index] += field.encoded;
        if rule.encoded_limit != 0 && budgets[index] > rule.encoded_limit {
            return Err(error(Reason::byte_limit_exceeded, rule.diagnostic));
        }
        if message == MessageKind::Envelope {
            // Payload framing belongs to metadata; only its actual value bytes are excluded.
            metadata += field.encoded
                - if field.number == 2 {
                    field.body.len()
                } else {
                    0
                };
            if metadata > limits::METADATA_BYTES {
                return Err(error(Reason::byte_limit_exceeded, ""));
            }
        }
        if packed {
            let mut packed = Cursor::new(field.body);
            while packed.position < field.body.len() {
                work.element(rule, count)?;
                let integer = packed.varint(rule.diagnostic)?;
                if collect {
                    output
                        .fields
                        .push((field.number, integer_value(rule.kind, integer)));
                }
            }
            continue;
        }
        let value = match rule.kind {
            Kind::String => {
                let text = core::str::from_utf8(field.body)
                    .map_err(|_| error(Reason::invalid_utf8, rule.diagnostic))?;
                if collect {
                    Some(Value::String(text.to_owned()))
                } else {
                    None
                }
            }
            Kind::Bytes => {
                if collect {
                    Some(Value::Bytes(field.body.to_vec()))
                } else {
                    None
                }
            }
            Kind::Message(nested) => {
                if depth == limits::MAX_DEPTH {
                    return Err(error(Reason::nesting_too_deep, rule.diagnostic));
                }
                let value = message_scan(
                    nested,
                    field.body,
                    limit,
                    collect,
                    work,
                    depth + 1,
                    rule.diagnostic,
                )?;
                if collect {
                    Some(Value::Message(value))
                } else {
                    None
                }
            }
            Kind::Map { key, value } => {
                if depth == limits::MAX_DEPTH {
                    return Err(error(Reason::nesting_too_deep, rule.diagnostic));
                }
                let (k, v) = map_entry(field.body, rule.diagnostic, key, value)?;
                if !maps[index].insert(k) {
                    return Err(error(Reason::duplicate_map_key, rule.diagnostic));
                }
                if collect {
                    Some(Value::MapEntry(
                        String::from_utf8(k.to_vec())
                            .map_err(|_| error(Reason::invalid_utf8, rule.diagnostic))?,
                        String::from_utf8(v.to_vec())
                            .map_err(|_| error(Reason::invalid_utf8, rule.diagnostic))?,
                    ))
                } else {
                    None
                }
            }
            kind => {
                if message == MessageKind::Timestamp {
                    if field.number == 1 {
                        seconds = field.integer as i64;
                    } else {
                        nanos = field.integer as i32;
                    }
                }
                if collect {
                    Some(integer_value(kind, field.integer))
                } else {
                    None
                }
            }
        };
        if let Some(value) = value {
            output.fields.push((field.number, value));
        }
    }
    if message == MessageKind::Timestamp {
        timestamp(seconds, nanos, parent)?;
    }
    Ok(output)
}

fn integer_value(kind: Kind, integer: u64) -> Value {
    match kind {
        Kind::Uint32 => Value::Uint32(integer as u32),
        Kind::Int32 => Value::Int32(integer as i32),
        Kind::Int64 => Value::Int64(integer as i64),
        Kind::Bool => Value::Bool(integer != 0),
        Kind::Enum(_) => Value::Enum(integer as i32),
        _ => Value::Uint64(integer),
    }
}

fn map_entry<'a>(
    bytes: &'a [u8],
    name: &'static str,
    key_limit: usize,
    value_limit: usize,
) -> Result<(&'a [u8], &'a [u8])> {
    // Entry diagnostics always identify the map, including a known forbidden wire type.
    let rules: &'static [FieldRule] = match name {
        "Command.env" => &ENV_ENTRY,
        "LaunchOption.hints" => &HINT_ENTRY,
        _ => &METADATA_ENTRY,
    };
    let mut values = [&b""[..]; 2];
    let mut seen = [false; 2];
    let mut cursor = Cursor::new(bytes);
    while let Some(field) = cursor.next(rules)? {
        if field.rule.is_none() {
            continue;
        }
        let index = field.number as usize - 1;
        if seen[index] {
            return Err(error(Reason::duplicate_field, name));
        }
        seen[index] = true;
        if field.body.len() > if index == 0 { key_limit } else { value_limit } {
            return Err(error(Reason::byte_limit_exceeded, name));
        }
        core::str::from_utf8(field.body).map_err(|_| error(Reason::invalid_utf8, name))?;
        values[index] = field.body;
    }
    Ok((values[0], values[1]))
}

const fn entry(name: &'static str) -> [FieldRule; 2] {
    let key = FieldRule {
        number: 1,
        name: "key",
        json_name: "key",
        diagnostic: name,
        kind: Kind::String,
        occurrence: Occurrence::Scalar,
        value_limit: 0,
        element_limit: 0,
        encoded_limit: 0,
    };
    [
        key,
        FieldRule {
            number: 2,
            name: "value",
            json_name: "value",
            ..key
        },
    ]
}
const ENV_ENTRY: [FieldRule; 2] = entry("Command.env");
const HINT_ENTRY: [FieldRule; 2] = entry("LaunchOption.hints");
const METADATA_ENTRY: [FieldRule; 2] = entry("ErrorInfo.metadata");
