// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Identity of a server instance.
//!
//! These are plain value types. They never carry private key material.

mod key_metadata;

pub use key_metadata::{KeyId, KeyPurpose, PublicKey, SigningKeyMetadata, TicketGeneration};
