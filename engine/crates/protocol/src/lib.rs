// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: 0BSD

//! Wire types only. Protocol validation is added separately.

// Preserve the pinned generator's bytes; drift checking owns these trees.
#[rustfmt::skip]
#[path = "generated/buffa/mod.rs"]
pub mod proto;

#[cfg(any(feature = "server", feature = "client"))]
#[rustfmt::skip]
#[path = "generated/connect/mod.rs"]
pub mod connect;
