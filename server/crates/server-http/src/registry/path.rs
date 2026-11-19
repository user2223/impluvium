// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Path templates such as `/api/v1/items/{id}`.

use std::error::Error;
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Segment {
    Literal(String),
    Param(String),
}

/// A validated path template. Parameters are written `{name}` and fill a
/// whole segment; wildcards and mixed segments are not supported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathTemplate {
    raw: String,
    segments: Vec<Segment>,
}

/// Why a path template was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathError {
    /// The template does not start with `/`.
    NotAbsolute,
    /// Two consecutive slashes or a trailing slash.
    EmptySegment,
    /// A literal segment with characters outside `A-Za-z0-9-_.~`.
    InvalidLiteral(String),
    /// A parameter that is not `{name}` with a lowercase name, or braces mixed into a literal.
    InvalidParam(String),
    /// The same parameter name twice.
    DuplicateParam(String),
}

impl PathTemplate {
    /// Parses and validates a template.
    pub fn parse(raw: &str) -> Result<Self, PathError> {
        let rest = raw.strip_prefix('/').ok_or(PathError::NotAbsolute)?;
        let mut segments = Vec::new();
        if !rest.is_empty() {
            for part in rest.split('/') {
                segments.push(parse_segment(part)?);
            }
        }
        let mut seen: Vec<&str> = Vec::new();
        for segment in &segments {
            if let Segment::Param(name) = segment {
                if seen.contains(&name.as_str()) {
                    return Err(PathError::DuplicateParam(name.clone()));
                }
                seen.push(name);
            }
        }
        Ok(Self {
            raw: raw.to_owned(),
            segments,
        })
    }

    /// The template as written.
    pub fn as_str(&self) -> &str {
        &self.raw
    }

    /// Parameter names in path order.
    pub fn params(&self) -> Vec<&str> {
        self.segments
            .iter()
            .filter_map(|segment| match segment {
                Segment::Param(name) => Some(name.as_str()),
                Segment::Literal(_) => None,
            })
            .collect()
    }

    /// Whether the two templates would share a parameter position under
    /// different names. The framework router keeps one name per position and
    /// refuses such a pair, so the registry has to reject it first. Templates
    /// that diverge on a literal before any differing name do not conflict.
    pub fn conflicts_with(&self, other: &Self) -> bool {
        for (left, right) in self.segments.iter().zip(&other.segments) {
            match (left, right) {
                (Segment::Literal(a), Segment::Literal(b)) if a == b => {}
                (Segment::Param(a), Segment::Param(b)) if a == b => {}
                (Segment::Param(_), Segment::Param(_)) => return true,
                _ => return false,
            }
        }
        false
    }

    /// The template with every parameter name erased. Two templates with the
    /// same shape match the same requests.
    pub fn shape(&self) -> String {
        let mut shape = String::new();
        for segment in &self.segments {
            shape.push('/');
            match segment {
                Segment::Literal(text) => shape.push_str(text),
                Segment::Param(_) => shape.push_str("{}"),
            }
        }
        if shape.is_empty() {
            shape.push('/');
        }
        shape
    }
}

fn parse_segment(part: &str) -> Result<Segment, PathError> {
    if part.is_empty() {
        return Err(PathError::EmptySegment);
    }
    if part.contains(['{', '}']) {
        let name = part
            .strip_prefix('{')
            .and_then(|inner| inner.strip_suffix('}'))
            .ok_or_else(|| PathError::InvalidParam(part.to_owned()))?;
        let mut characters = name.chars();
        let valid = matches!(characters.next(), Some('a'..='z'))
            && characters.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
        return if valid {
            Ok(Segment::Param(name.to_owned()))
        } else {
            Err(PathError::InvalidParam(part.to_owned()))
        };
    }
    if part
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '~'))
    {
        Ok(Segment::Literal(part.to_owned()))
    } else {
        Err(PathError::InvalidLiteral(part.to_owned()))
    }
}

impl fmt::Display for PathError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAbsolute => formatter.write_str("path must start with '/'"),
            Self::EmptySegment => formatter.write_str("path has an empty segment"),
            Self::InvalidLiteral(part) => write!(formatter, "invalid path segment: {part}"),
            Self::InvalidParam(part) => write!(formatter, "invalid path parameter: {part}"),
            Self::DuplicateParam(name) => write!(formatter, "duplicate path parameter: {name}"),
        }
    }
}

impl Error for PathError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_literals_and_parameters() {
        let template = PathTemplate::parse("/api/v1/items/{id}/parts/{part_no}").unwrap();
        assert_eq!(template.params(), ["id", "part_no"]);
        assert_eq!(template.shape(), "/api/v1/items/{}/parts/{}");
        assert_eq!(PathTemplate::parse("/").unwrap().shape(), "/");
    }

    #[test]
    fn renamed_parameter_positions_conflict() {
        let parse = |raw| PathTemplate::parse(raw).unwrap();
        assert!(parse("/a/{x}/b").conflicts_with(&parse("/a/{y}/c")));
        assert!(parse("/a/{x}").conflicts_with(&parse("/a/{y}/c")));
        assert!(!parse("/a/{x}/b").conflicts_with(&parse("/a/{x}/c")));
        assert!(!parse("/a/{x}").conflicts_with(&parse("/b/{y}")));
        assert!(!parse("/a/b").conflicts_with(&parse("/a/{y}")));
    }

    #[test]
    fn rejects_malformed_templates_with_the_specific_cause() {
        let cases = [
            ("api", PathError::NotAbsolute),
            ("", PathError::NotAbsolute),
            ("/a//b", PathError::EmptySegment),
            ("/a/", PathError::EmptySegment),
            ("/a b", PathError::InvalidLiteral("a b".into())),
            ("/a/{Id}", PathError::InvalidParam("{Id}".into())),
            ("/a/{*rest}", PathError::InvalidParam("{*rest}".into())),
            ("/a/x{id}", PathError::InvalidParam("x{id}".into())),
            ("/a/{id", PathError::InvalidParam("{id".into())),
            ("/a/{id}/{id}", PathError::DuplicateParam("id".into())),
        ];
        for (raw, expected) in cases {
            assert_eq!(PathTemplate::parse(raw), Err(expected), "{raw}");
        }
    }
}
