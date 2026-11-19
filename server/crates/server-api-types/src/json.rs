// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Fields that carry arbitrary JSON.
//!
//! [`JsonValue`] wraps a [`serde_json::Value`] and bounds it when it is read
//! from a request: arrays and objects may be nested inside each other at most
//! [`MAX_DEPTH`] levels deep (a scalar is not a level) and the value holds at
//! most [`MAX_NODES`] values in total. The size of the whole body is bounded
//! separately by the HTTP layer. In TypeScript the field has the recursive
//! `JsonValue` union that ts-rs writes to `JsonValue.ts`.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;
use ts_rs::TS;

/// Deepest allowed nesting of arrays and objects: `[[1]]` has two levels.
pub const MAX_DEPTH: usize = 32;
/// Largest allowed number of values (every element, member and scalar counts).
pub const MAX_NODES: usize = 10_000;

/// A bounded JSON value.
#[derive(Debug, Clone, PartialEq, TS)]
#[ts(type = "string | number | boolean | null | Array<JsonValue> | { [key: string]: JsonValue }")]
pub struct JsonValue(Value);

/// Why a value exceeds the bounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JsonBound {
    /// Nested deeper than [`MAX_DEPTH`].
    TooDeep,
    /// More than [`MAX_NODES`] values.
    TooLarge,
}

impl std::fmt::Display for JsonBound {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::TooDeep => "JSON value is nested too deeply",
            Self::TooLarge => "JSON value has too many elements",
        })
    }
}

impl std::error::Error for JsonBound {}

impl JsonValue {
    /// Wraps a value after checking its bounds.
    pub fn new(value: Value) -> Result<Self, JsonBound> {
        let mut nodes = 0;
        check(&value, 0, &mut nodes)?;
        Ok(Self(value))
    }

    /// The wrapped value.
    pub fn as_value(&self) -> &Value {
        &self.0
    }

    /// Unwraps the value.
    pub fn into_value(self) -> Value {
        self.0
    }
}

/// `depth` is the number of arrays and objects around `value`.
fn check(value: &Value, depth: usize, nodes: &mut usize) -> Result<(), JsonBound> {
    *nodes += 1;
    if *nodes > MAX_NODES {
        return Err(JsonBound::TooLarge);
    }
    if matches!(value, Value::Array(_) | Value::Object(_)) && depth >= MAX_DEPTH {
        return Err(JsonBound::TooDeep);
    }
    match value {
        Value::Array(items) => items
            .iter()
            .try_for_each(|item| check(item, depth + 1, nodes)),
        Value::Object(members) => members
            .values()
            .try_for_each(|item| check(item, depth + 1, nodes)),
        _ => Ok(()),
    }
}

impl Serialize for JsonValue {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for JsonValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::new(Value::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn nested(levels: usize) -> String {
        format!("{}1{}", "[".repeat(levels), "]".repeat(levels))
    }

    #[test]
    fn values_round_trip() {
        let body = r#"{"a":[1,"x",null,true,{"b":1.5}]}"#;
        let value: JsonValue = serde_json::from_str(body).unwrap();
        assert_eq!(
            value.as_value(),
            &json!({"a": [1, "x", null, true, {"b": 1.5}]})
        );
        assert_eq!(serde_json::to_string(&value).unwrap(), body);
    }

    #[test]
    fn depth_is_bounded() {
        assert!(serde_json::from_str::<JsonValue>(&nested(MAX_DEPTH)).is_ok());
        assert!(serde_json::from_str::<JsonValue>(&nested(MAX_DEPTH + 1)).is_err());
        let deep = serde_json::from_str::<Value>(&nested(MAX_DEPTH + 1)).unwrap();
        assert_eq!(JsonValue::new(deep), Err(JsonBound::TooDeep));
    }

    #[test]
    fn size_is_bounded() {
        let fits = Value::Array(vec![Value::Null; MAX_NODES - 1]);
        assert!(JsonValue::new(fits).is_ok());
        let too_many = Value::Array(vec![Value::Null; MAX_NODES]);
        assert_eq!(JsonValue::new(too_many), Err(JsonBound::TooLarge));
    }
}
