// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Request identifiers.
//!
//! The server assigns every request an identifier of its own. An identifier
//! sent by the client is never trusted or echoed. An identifier is a random
//! per-process prefix and a counter, both in hex, so identifiers of one
//! process are unique and those of different processes differ.

use std::collections::hash_map::RandomState;
use std::fmt;
use std::hash::{BuildHasher, Hasher};
use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};

/// The identifier of one request. It is stored in the request extensions and
/// returned in the `X-Request-Id` response header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestId(Arc<str>);

impl RequestId {
    /// Assigns a fresh identifier.
    pub fn generate() -> Self {
        static PREFIX: OnceLock<u64> = OnceLock::new();
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let prefix = *PREFIX.get_or_init(|| RandomState::new().build_hasher().finish());
        let count = COUNTER.fetch_add(1, Ordering::Relaxed);
        Self(format!("{prefix:016x}-{count:08x}").into())
    }

    /// The identifier as text: ASCII hex digits and one hyphen.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for RequestId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn identifiers_are_unique_hex_and_share_a_prefix() {
        let ids: Vec<RequestId> = (0..1000).map(|_| RequestId::generate()).collect();
        let unique: HashSet<&str> = ids.iter().map(RequestId::as_str).collect();
        assert_eq!(unique.len(), ids.len());
        let prefix = |id: &RequestId| id.as_str().split('-').next().unwrap().to_owned();
        assert!(ids.iter().all(|id| prefix(id) == prefix(&ids[0])));
        for id in &ids {
            let text = id.as_str();
            assert_eq!(text.len(), 25);
            assert!(
                text.bytes()
                    .all(|byte| byte == b'-' || byte.is_ascii_hexdigit())
            );
        }
    }
}
