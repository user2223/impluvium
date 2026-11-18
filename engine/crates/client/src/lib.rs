// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: 0BSD

//! Shared client types and pure HTTP rules. Network adapters are added separately.

pub use connectrpc::client::{ClientConfig, ServiceTransport};
pub use impluvium_protocol::{connect, proto};

pub mod credentials;
pub mod error;
pub mod response;
pub mod transport;
