// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Three-state fields of partial updates.
//!
//! A JSON body can say three different things about a field: nothing at all
//! (leave it unchanged), `null` (clear it) or a value (set it). `Option<T>`
//! cannot tell the first two apart, [`Tri`] can.
//!
//! A field of this type must be declared as follows, so that serde and the
//! TypeScript declaration describe the same wire form:
//!
//! ```text
//! #[serde(default, skip_serializing_if = "Tri::is_missing")]
//! #[ts(optional = nullable)] // TypeScript: `nickname?: string | null`
//! pub nickname: Tri<String>,
//! ```
//!
//! [`Tri`] implements `ts_rs::TS` like `Option<T>` (`T | null`, with the
//! imports of `T`), so no type string is needed. A unit test of the export
//! module fails when a field of this type, in any server crate, lacks the two
//! serde attributes or `#[ts(optional = nullable)]`.
//!
//! There is no general rule that strips `null` from bodies: every field that
//! needs the three states says so by using this type.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use ts_rs::{Config, TS, TypeVisitor};

/// A missing field, an explicit `null` or a value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Tri<T> {
    /// The field is not in the body: leave the stored value as it is.
    #[default]
    Missing,
    /// The field is `null`: clear the stored value.
    Clear,
    /// The field carries a value: store it.
    Set(T),
}

impl<T> Tri<T> {
    /// True for a field that was not sent; used by `skip_serializing_if`.
    pub fn is_missing(&self) -> bool {
        matches!(self, Self::Missing)
    }

    /// `None` when missing, `Some(None)` when cleared, `Some(Some(value))` when set.
    pub fn into_option(self) -> Option<Option<T>> {
        match self {
            Self::Missing => None,
            Self::Clear => Some(None),
            Self::Set(value) => Some(Some(value)),
        }
    }

    /// Like [`Tri::into_option`], borrowing the value.
    pub fn as_option(&self) -> Option<Option<&T>> {
        match self {
            Self::Missing => None,
            Self::Clear => Some(None),
            Self::Set(value) => Some(Some(value)),
        }
    }
}

impl<T> From<Option<Option<T>>> for Tri<T> {
    fn from(value: Option<Option<T>>) -> Self {
        match value {
            None => Self::Missing,
            Some(None) => Self::Clear,
            Some(Some(value)) => Self::Set(value),
        }
    }
}

/// `null` is [`Tri::Clear`]. With `#[serde(default)]` a missing field never gets here and
/// stays [`Tri::Missing`]; without it serde reports the missing field to `deserialize`
/// as a `none`, which also becomes [`Tri::Clear`]. Hence the attribute is mandatory.
impl<'de, T: Deserialize<'de>> Deserialize<'de> for Tri<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(match Option::<T>::deserialize(deserializer)? {
            None => Self::Clear,
            Some(value) => Self::Set(value),
        })
    }
}

/// A field that is [`Tri::Missing`] must be skipped with `skip_serializing_if`;
/// if it is serialised anyway it becomes `null`.
impl<T: Serialize> Serialize for Tri<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Set(value) => value.serialize(serializer),
            Self::Missing | Self::Clear => serializer.serialize_none(),
        }
    }
}

/// The TypeScript type is `T | null`, as for `Option<T>`; whether the field is also optional
/// (`name?:`) is up to the field attribute `#[ts(optional = nullable)]`.
impl<T: TS> TS for Tri<T> {
    type WithoutGenerics = Self;
    // Not an `Option` for ts-rs: a struct-level `#[ts(optional)]` must never strip the
    // `| null` and turn the field into `a?: T`.
    type OptionInnerType = Self;

    const IS_OPTION: bool = false;

    fn ident(_: &Config) -> String {
        panic!("Tri<T> has no declaration of its own")
    }

    fn name(config: &Config) -> String {
        format!("{} | null", T::name(config))
    }

    fn inline(config: &Config) -> String {
        format!("{} | null", T::inline(config))
    }

    fn inline_flattened(_: &Config) -> String {
        panic!("Tri<T> cannot be flattened")
    }

    fn visit_dependencies(visitor: &mut impl TypeVisitor)
    where
        Self: 'static,
    {
        T::visit_dependencies(visitor);
    }

    fn visit_generics(visitor: &mut impl TypeVisitor)
    where
        Self: 'static,
    {
        visitor.visit::<T>();
        T::visit_generics(visitor);
    }

    fn decl(_: &Config) -> String {
        panic!("Tri<T> cannot be declared")
    }

    fn decl_concrete(_: &Config) -> String {
        panic!("Tri<T> cannot be declared")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, PartialEq, Serialize, Deserialize, TS)]
    struct Patch {
        #[serde(default, skip_serializing_if = "Tri::is_missing")]
        #[ts(optional = nullable)]
        nickname: Tri<String>,
    }

    #[test]
    fn the_three_bodies_deserialize_to_the_three_states() {
        let missing: Patch = serde_json::from_str("{}").unwrap();
        let clear: Patch = serde_json::from_str(r#"{"nickname":null}"#).unwrap();
        let set: Patch = serde_json::from_str(r#"{"nickname":"x"}"#).unwrap();
        assert_eq!(missing.nickname, Tri::Missing);
        assert_eq!(clear.nickname, Tri::Clear);
        assert_eq!(set.nickname, Tri::Set("x".to_owned()));
        assert!(serde_json::from_str::<Patch>(r#"{"nickname":1}"#).is_err());
    }

    #[test]
    fn the_three_states_serialize_back_to_the_same_bodies() {
        for body in ["{}", r#"{"nickname":null}"#, r#"{"nickname":"x"}"#] {
            let patch: Patch = serde_json::from_str(body).unwrap();
            assert_eq!(serde_json::to_string(&patch).unwrap(), body);
        }
    }

    #[test]
    fn option_conversions_and_default() {
        assert!(Tri::<u8>::default().is_missing());
        assert_eq!(Tri::<u8>::from(None), Tri::Missing);
        assert_eq!(Tri::<u8>::from(Some(None)), Tri::Clear);
        assert_eq!(Tri::from(Some(Some(3u8))), Tri::Set(3));
        assert_eq!(Tri::Set(3u8).into_option(), Some(Some(3)));
        assert_eq!(Tri::<u8>::Clear.as_option(), Some(None));
        assert_eq!(Tri::<u8>::Missing.as_option(), None);
    }
}
