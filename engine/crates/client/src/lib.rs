// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: 0BSD

//! Shared HTTP rules, platform TLS and a response-protected Connect transport.

pub use connectrpc::client::{ClientConfig, ServiceTransport};
pub use impluvium_protocol::{connect, proto};

pub mod credentials;
pub mod error;
pub mod http;
pub mod release;
pub mod response;
pub mod transport;
pub mod trust;

pub mod rpc;
