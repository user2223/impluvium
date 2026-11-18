// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: 0BSD

//! A separate process exercises rustls defaults installed outside this module.

use std::sync::Arc;

use impluvium_client::trust::{ProviderError, initialize_aws_lc};
use rustls::crypto::CryptoProvider;

#[test]
fn an_external_default_is_not_silently_adopted() {
    rustls::crypto::aws_lc_rs::default_provider()
        .install_default()
        .unwrap();
    let installed = CryptoProvider::get_default().unwrap().clone();
    assert!(matches!(
        initialize_aws_lc(),
        Err(ProviderError::AlreadyInitialized)
    ));
    assert!(Arc::ptr_eq(
        &installed,
        CryptoProvider::get_default().unwrap()
    ));
}
