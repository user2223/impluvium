// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Sets of rights and the rights that roles carry.
//!
//! Sessions, tokens and devices all end up with a [`RightSet`], and every
//! policy is checked against that one type, so a right means the same thing
//! whichever credential presented it. An empty set grants nothing.

use std::collections::BTreeSet;

use crate::policy::Right;
use crate::principal::Role;

/// A set of rights. The default and the empty set contain no right.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RightSet(BTreeSet<Right>);

impl RightSet {
    /// The set without any right.
    pub fn empty() -> Self {
        Self::default()
    }

    /// A set holding exactly the given rights.
    pub fn from_rights(rights: &[Right]) -> Self {
        Self(rights.iter().copied().collect())
    }

    /// Builds a set from right names. A name that is not a valid right is
    /// dropped, so a typo can only ever grant less.
    fn from_names(names: &[&'static str]) -> Self {
        Self(
            names
                .iter()
                .filter_map(|name| Right::new(name).ok())
                .collect(),
        )
    }

    /// Whether the set holds the right.
    pub fn contains(&self, right: Right) -> bool {
        self.0.contains(&right)
    }

    /// Whether the set holds every listed right.
    pub fn contains_all(&self, rights: &[Right]) -> bool {
        rights.iter().all(|right| self.0.contains(right))
    }

    /// The rights held by both sets.
    pub fn intersection(&self, other: &Self) -> Self {
        Self(self.0.intersection(&other.0).copied().collect())
    }

    /// Whether the set holds no right.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The rights in sorted order.
    pub fn iter(&self) -> impl Iterator<Item = Right> + '_ {
        self.0.iter().copied()
    }
}

/// The rights of a role. Roles carry only what is listed here; the name of a
/// role never stands for "everything".
pub fn role_rights(role: Role) -> RightSet {
    match role {
        Role::User => RightSet::from_names(&[
            "emoji:read",
            "settings:read",
            "profile:read",
            "clients:read",
            "clients:revoke",
        ]),
        Role::Admin => RightSet::from_names(&[
            "emoji:read",
            "settings:read",
            "profile:read",
            "clients:read",
            "clients:revoke",
            "admin:manage",
        ]),
    }
}

/// The only rights a store session can ever hold: the store, the library
/// and game pages and the public profile of an account, but nothing about
/// administration, devices, tokens or second factors.
pub fn store_session_rights() -> RightSet {
    RightSet::from_names(&["emoji:read", "settings:read", "profile:read"])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn right(name: &'static str) -> Right {
        Right::new(name).unwrap()
    }

    #[test]
    fn empty_set_holds_nothing() {
        let empty = RightSet::empty();
        assert!(empty.is_empty());
        assert!(!empty.contains(right("emoji:read")));
        assert!(!empty.contains_all(&[right("emoji:read")]));
        assert_eq!(empty, RightSet::default());
    }

    #[test]
    fn intersection_keeps_only_shared_rights() {
        let left = RightSet::from_rights(&[right("emoji:read"), right("admin:manage")]);
        let other = RightSet::from_rights(&[right("emoji:read"), right("token:create")]);
        let shared = left.intersection(&other);
        assert_eq!(shared, RightSet::from_rights(&[right("emoji:read")]));
        assert!(left.intersection(&RightSet::empty()).is_empty());
    }

    #[test]
    fn roles_differ_and_admin_is_a_listed_right() {
        let user = role_rights(Role::User);
        let admin = role_rights(Role::Admin);
        assert!(user.contains(right("emoji:read")));
        assert!(user.contains(right("settings:read")));
        assert!(user.contains(right("profile:read")));
        assert!(user.contains(right("clients:read")));
        assert!(user.contains(right("clients:revoke")));
        assert!(!user.contains(right("admin:manage")));
        assert!(admin.contains(right("admin:manage")));
        assert!(role_rights(Role::User).intersection(&admin) == user);
    }

    #[test]
    fn store_sessions_never_reach_administration() {
        let store = store_session_rights();
        assert!(store.contains(right("emoji:read")));
        assert!(store.contains(right("settings:read")));
        assert!(store.contains(right("profile:read")));
        assert!(!store.contains(right("admin:manage")));
        // Devices belong to the account pages, which the store window does not show.
        assert!(!store.contains(right("clients:read")));
        assert!(!store.contains(right("clients:revoke")));
    }
}
