// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: 0BSD

//! A separate test executable keeps global provider initialization independent
//! of HTTP tests. No test ordering or reset of rustls's process global is needed.

use std::sync::Arc;

use impluvium_client::{
    error::{Diagnostic, Reason},
    trust::{self, Error, Platform, ProviderError, Tls},
};
use rustls::crypto::CryptoProvider;

#[test]
fn unavailable_windows_has_no_initialization_side_effect_and_provider_conflicts_are_errors() {
    assert!(CryptoProvider::get_default().is_none());
    assert!(matches!(
        Tls::new(Platform::WindowsNative, vec![]),
        Err(Error::Platform(Diagnostic {
            reason: Reason::platform_trust_unavailable,
            field: "",
        }))
    ));
    assert!(CryptoProvider::get_default().is_none());

    let _provider = trust::initialize_aws_lc().unwrap();
    let installed = CryptoProvider::get_default().unwrap().clone();
    let threads: Vec<_> = (0..8)
        .map(|_| {
            std::thread::spawn(|| {
                let _ = trust::initialize_aws_lc().unwrap();
            })
        })
        .collect();
    for thread in threads {
        thread.join().unwrap();
    }
    assert!(Arc::ptr_eq(
        &installed,
        CryptoProvider::get_default().unwrap()
    ));

    let mut different = rustls::crypto::aws_lc_rs::default_provider();
    different.cipher_suites.clear();
    assert!(matches!(
        trust::initialize_provider(Arc::new(different)),
        Err(ProviderError::AlreadyInitialized)
    ));
    assert!(Arc::ptr_eq(
        &installed,
        CryptoProvider::get_default().unwrap()
    ));
    assert!(matches!(
        Tls::new(Platform::WindowsNative, vec![]),
        Err(Error::Platform(_))
    ));
    assert!(Arc::ptr_eq(
        &installed,
        CryptoProvider::get_default().unwrap()
    ));

    #[cfg(target_os = "linux")]
    {
        let tls = Tls::new(Platform::Linux, vec![]).unwrap();
        assert_eq!(
            tls.config().alpn_protocols,
            [b"h2".to_vec(), b"http/1.1".to_vec()]
        );
        // Invalid user roots must fail through the same verifier, without a
        // bundled-root fallback or an accept-invalid-certificates alternative.
        assert!(matches!(
            Tls::new(
                Platform::Linux,
                vec![rustls::pki_types::CertificateDer::from(vec![0])]
            ),
            Err(Error::Platform(Diagnostic {
                reason: Reason::platform_trust_unavailable,
                field: ""
            }))
        ));
    }
}
