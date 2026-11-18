// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: 0BSD

//! The single platform trust boundary. Unsupported platforms fail before any
//! provider installation, certificate-store access or client construction.

use std::sync::{Arc, Mutex, OnceLock};

use rustls::{ClientConfig, crypto::CryptoProvider, pki_types::CertificateDer};

use crate::error::{Diagnostic, Reason, local};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProviderError {
    AlreadyInitialized,
    LockPoisoned,
}
impl std::fmt::Display for ProviderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "TLS provider initialization: {self:?}")
    }
}
impl std::error::Error for ProviderError {}

/// A provider's identity is its Arc, not its display name. Reusing an identical
/// handle is idempotent; replacing it, even with a newly allocated provider,
/// is rejected. This avoids guessing whether opaque crypto implementations match.
#[derive(Clone)]
pub struct Provider(Arc<CryptoProvider>);

struct Installed {
    requested: Arc<CryptoProvider>,
    provider: Provider,
}
static INSTALLED: Mutex<Option<Installed>> = Mutex::new(None);
static AWS_LC: OnceLock<Arc<CryptoProvider>> = OnceLock::new();

pub fn initialize_aws_lc() -> Result<Provider, ProviderError> {
    initialize_provider(
        AWS_LC
            .get_or_init(|| Arc::new(rustls::crypto::aws_lc_rs::default_provider()))
            .clone(),
    )
}

/// Call before constructing any TLS client in the process. An externally
/// installed rustls default is a conflict, never an implicit fallback.
pub fn initialize_provider(requested: Arc<CryptoProvider>) -> Result<Provider, ProviderError> {
    let mut installed = INSTALLED.lock().map_err(|_| ProviderError::LockPoisoned)?;
    if let Some(state) = installed.as_ref() {
        return if Arc::ptr_eq(&state.requested, &requested) {
            Ok(state.provider.clone())
        } else {
            Err(ProviderError::AlreadyInitialized)
        };
    }
    requested
        .as_ref()
        .clone()
        .install_default()
        .map_err(|_| ProviderError::AlreadyInitialized)?;
    let provider = Provider(
        CryptoProvider::get_default()
            .expect("provider installed above")
            .clone(),
    );
    *installed = Some(Installed {
        requested,
        provider: provider.clone(),
    });
    Ok(provider)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Platform {
    Linux,
    /// Reserved for the native Windows verifier; deliberately unavailable even
    /// on a Windows build until that integration is implemented and reviewed.
    WindowsNative,
    Unsupported,
}
impl Platform {
    pub const fn current() -> Self {
        if cfg!(target_os = "linux") {
            Self::Linux
        } else if cfg!(target_os = "windows") {
            Self::WindowsNative
        } else {
            Self::Unsupported
        }
    }
    pub fn available(self) -> Result<(), Diagnostic> {
        if self == Self::Linux && cfg!(target_os = "linux") {
            Ok(())
        } else {
            Err(local(Reason::platform_trust_unavailable))
        }
    }
}

#[derive(Debug)]
pub enum Error {
    Provider(ProviderError),
    Platform(Diagnostic),
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "TLS configuration: {self:?}")
    }
}
impl std::error::Error for Error {}

/// Clone this configuration for every HTTP stack. Additional user roots go
/// through the same OS verifier; no webpki root bundle is substituted.
#[derive(Clone)]
pub struct Tls {
    config: Arc<ClientConfig>,
}
impl Tls {
    pub fn new(
        platform: Platform,
        extra_roots: Vec<CertificateDer<'static>>,
    ) -> Result<Self, Error> {
        platform.available().map_err(Error::Platform)?;
        let provider = initialize_aws_lc().map_err(Error::Provider)?;
        Self::with_provider(platform, &provider, extra_roots)
    }

    fn with_provider(
        platform: Platform,
        provider: &Provider,
        extra_roots: Vec<CertificateDer<'static>>,
    ) -> Result<Self, Error> {
        platform.available().map_err(Error::Platform)?;
        #[cfg(target_os = "linux")]
        {
            let unavailable = || Error::Platform(local(Reason::platform_trust_unavailable));
            let verifier = rustls_platform_verifier::Verifier::new_with_extra_roots(
                extra_roots,
                provider.0.clone(),
            )
            .map_err(|_| unavailable())?;
            let mut config = ClientConfig::builder_with_provider(provider.0.clone())
                .with_protocol_versions(&[&rustls::version::TLS13, &rustls::version::TLS12])
                .map_err(|_| unavailable())?
                .dangerous()
                .with_custom_certificate_verifier(Arc::new(verifier))
                .with_no_client_auth();
            config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
            Ok(Self {
                config: Arc::new(config),
            })
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = (provider, extra_roots);
            Err(Error::Platform(local(Reason::platform_trust_unavailable)))
        }
    }

    pub fn config(&self) -> &Arc<ClientConfig> {
        &self.config
    }
}
