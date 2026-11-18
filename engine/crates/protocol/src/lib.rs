// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: 0BSD

//! Protocol wire types, bounded parsers and signature primitives.

#![forbid(unsafe_code)]

pub mod api;
pub mod decode;
pub mod descriptor;
pub mod envelope;
pub mod events;
pub mod fastcdc;
pub mod hash;
pub mod ids;
pub mod keys;
pub mod limits;
pub mod manifest;
pub mod path;
pub mod proof;
pub mod ticket;

// Preserve the pinned generator's bytes; drift checking owns these trees.
#[rustfmt::skip]
#[path = "generated/buffa/mod.rs"]
pub mod proto;

#[cfg(any(feature = "server", feature = "client"))]
#[rustfmt::skip]
#[path = "generated/connect/mod.rs"]
mod connect_generated;

#[cfg(any(feature = "server", feature = "client"))]
pub mod connect;
