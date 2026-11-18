// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: 0BSD

//! Connect error wrapper scanning uses the same JSON lexer and profile as
//! schema messages. Unknown subtrees are validated without materializing them.

use super::*;

pub fn error_body(bytes: &[u8]) -> Result<crate::api::errors::ErrorBody> {
    let limit = InputLimit::response(65_536);
    limit.check(bytes)?;
    // Complete profile first: duplicates and depth also in ignored members.
    document(bytes, limit)?;
    let mut r = Reader { bytes, pos: 0 };
    r.need(b'{', "")?;
    let mut code = None;
    let mut details = None;
    if !r.eat(b'}') {
        loop {
            let name = r.text("")?;
            r.need(b':', "")?;
            let start = r.pos;
            let tok = r.token("")?;
            match (
                name.eq_str("code"),
                name.eq_str("message"),
                name.eq_str("details"),
            ) {
                (true, _, _) => {
                    code = Some(match tok {
                        Token::String(s) if (1..=64).contains(&s.len) => s.decoded(),
                        _ => return Err(error(Reason::invalid_json, "")),
                    })
                }
                (_, true, _) => match tok {
                    Token::String(s) if s.len <= 4096 => (),
                    _ => return Err(error(Reason::invalid_json, "")),
                },
                (_, _, true) => details = Some(start),
                _ => (),
            }
            let mut parser = Parser {
                reader: r,
                work: Work::default(),
                limit,
                collect: false,
            };
            parser.unknown(tok, 0)?;
            r = parser.reader;
            if r.eat(b'}') {
                break;
            }
            r.need(b',', "")?;
        }
    }
    let code = code.ok_or(error(Reason::missing_field, ""))?;
    let mut info = None;
    if let Some(pos) = details {
        r.pos = pos;
        r.need(b'[', "")?;
        let mut count = 0;
        let mut info_bytes = None;
        let mut duplicate_info = false;
        if !r.eat(b']') {
            loop {
                if count == 16 {
                    return Err(error(Reason::element_limit_exceeded, ""));
                }
                count += 1;
                r.need(b'{', "")?;
                let mut kind = None;
                let mut value = None;
                if !r.eat(b'}') {
                    loop {
                        let name = r.text("")?;
                        r.need(b':', "")?;
                        let tok = r.token("")?;
                        if name.eq_str("type") {
                            kind = Some(match tok {
                                Token::String(s) if (1..=256).contains(&s.len) => s.decoded(),
                                _ => return Err(error(Reason::invalid_json, "")),
                            });
                        } else if name.eq_str("value") {
                            value = Some(match tok {
                                Token::String(s) => s.decoded(),
                                _ => return Err(error(Reason::invalid_json, "")),
                            });
                        }
                        let mut parser = Parser {
                            reader: r,
                            work: Work::default(),
                            limit,
                            collect: false,
                        };
                        parser.unknown(tok, 0)?;
                        r = parser.reader;
                        if r.eat(b'}') {
                            break;
                        }
                        r.need(b',', "")?;
                    }
                }
                let kind = kind.ok_or(error(Reason::missing_field, ""))?;
                let value = value.ok_or(error(Reason::missing_field, ""))?;
                // Every detail, including ignored ones, has the same bytes bound.
                content::base64(&value, 8192, "", false)?;
                if kind == "impluvium.v2.ErrorInfo" {
                    if info_bytes.is_some() {
                        duplicate_info = true;
                    } else {
                        let Some(Value::Bytes(bytes)) = content::base64(&value, 8192, "", true)?
                        else {
                            unreachable!()
                        };
                        info_bytes = Some(bytes);
                    }
                }
                if r.eat(b']') {
                    break;
                }
                r.need(b',', "")?;
            }
        }
        // Step 6 applies to every detail before step 7 rejects duplicates.
        if duplicate_info {
            return Err(error(Reason::duplicate_field, ""));
        }
        // The entire wrapper is checked before the ErrorInfo decoder runs.
        if let Some(bytes) = info_bytes {
            info = Some(crate::api::decode_error_info(&bytes)?);
        }
    }
    Ok(crate::api::errors::ErrorBody::checked(code, info))
}

pub(crate) fn document(bytes: &[u8], limit: InputLimit) -> Result<()> {
    limit.check(bytes)?;
    let mut parser = Parser {
        reader: Reader { bytes, pos: 0 },
        work: Work::default(),
        limit,
        collect: false,
    };
    let tok = parser.reader.token("")?;
    parser.unknown(tok, 0)?;
    parser.reader.space();
    if parser.reader.pos != bytes.len() {
        return Err(error(Reason::invalid_json, ""));
    }
    Ok(())
}
