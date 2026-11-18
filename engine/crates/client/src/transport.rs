// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: 0BSD

//! Decisions over public configuration only. No network, environment, clock or
//! credential-store access occurs here. URL parsing uses the protocol's WHATWG
//! implementation, including IDNA and non-canonical IPv4 input normalization.

use crate::{
    credentials::CredentialPurpose,
    error::{Diagnostic, Reason, local},
    response::Binding,
};
use impluvium_protocol::api::url::{Host, Url};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Origin {
    pub scheme: String,
    pub host: String,
    pub port: u16,
}
impl Origin {
    pub fn of(url: &Url) -> Self {
        Self {
            scheme: url.scheme().into(),
            host: url.host_str().expect("validated HTTP URL").into(),
            port: url.port_or_known_default().expect("validated HTTP scheme"),
        }
    }
    pub fn parse(text: &str) -> Result<Self, Diagnostic> {
        Ok(Self::of(&request_url(text)?))
    }
}

pub fn request_url(text: &str) -> Result<Url, Diagnostic> {
    let url = Url::parse(text).map_err(|_| local(Reason::invalid_url))?;
    validate_url(&url)?;
    Ok(url)
}
fn validate_url(url: &Url) -> Result<(), Diagnostic> {
    if !matches!(url.scheme(), "http" | "https")
        || url.host().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err(local(Reason::invalid_url));
    }
    Ok(())
}

/// Remove one trailing slash from user input. A remaining trailing slash is
/// invalid; an empty WHATWG root path serializes as `/` and has prefix `""`.
#[derive(Clone, Debug)]
pub struct BaseUrl(Url);
impl BaseUrl {
    pub fn parse(text: &str) -> Result<Self, Diagnostic> {
        let text = text.strip_suffix('/').unwrap_or(text);
        let url = request_url(text)?;
        if url.query().is_some() || (url.path() != "/" && url.path().ends_with('/')) {
            return Err(local(Reason::invalid_url));
        }
        Ok(Self(url))
    }
    pub fn origin(&self) -> Origin {
        Origin::of(&self.0)
    }
    pub fn prefix(&self) -> &str {
        self.0.path().strip_suffix('/').unwrap_or(self.0.path())
    }
    pub fn as_url(&self) -> &Url {
        &self.0
    }
    pub fn serialized(&self) -> String {
        self.0
            .as_str()
            .strip_suffix('/')
            .unwrap_or(self.0.as_str())
            .into()
    }
}

pub fn is_loopback(url: &Url) -> bool {
    match url.host() {
        Some(Host::Domain(host)) => host == "localhost" || host.ends_with(".localhost"),
        Some(Host::Ipv4(ip)) => ip.is_loopback(),
        Some(Host::Ipv6(ip)) => ip.is_loopback(),
        None => false,
    }
}

/// All adapters must explicitly select this mode and disable automatic proxy
/// discovery. There is intentionally no environment or OS proxy alternative.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConnectionMode {
    Direct,
}
pub const CONNECTION_MODE: ConnectionMode = ConnectionMode::Direct;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Method {
    StartPairing,
    PollPairing,
    BeginSession,
    CompleteSession,
    RevokeSelf,
    ListDevices,
    RevokeDevice,
    ListLibrary,
    GetGame,
    ListVersions,
    GetVersion,
    OpenDownload,
    RefreshTicket,
}
impl Method {
    /// Unknown methods have no fallback response schema or size limit.
    pub fn from_route(route: &str) -> Option<Self> {
        Some(match route {
            "impluvium.v2.AuthService/StartPairing" => Self::StartPairing,
            "impluvium.v2.AuthService/PollPairing" => Self::PollPairing,
            "impluvium.v2.AuthService/BeginSession" => Self::BeginSession,
            "impluvium.v2.AuthService/CompleteSession" => Self::CompleteSession,
            "impluvium.v2.AuthService/RevokeSelf" => Self::RevokeSelf,
            "impluvium.v2.DeviceService/ListDevices" => Self::ListDevices,
            "impluvium.v2.DeviceService/RevokeDevice" => Self::RevokeDevice,
            "impluvium.v2.LibraryService/ListLibrary" => Self::ListLibrary,
            "impluvium.v2.LibraryService/GetGame" => Self::GetGame,
            "impluvium.v2.LibraryService/ListVersions" => Self::ListVersions,
            "impluvium.v2.LibraryService/GetVersion" => Self::GetVersion,
            "impluvium.v2.DownloadService/OpenDownload" => Self::OpenDownload,
            "impluvium.v2.DownloadService/RefreshTicket" => Self::RefreshTicket,
            _ => return None,
        })
    }
    pub const fn route(self) -> &'static str {
        match self {
            Self::StartPairing => "impluvium.v2.AuthService/StartPairing",
            Self::PollPairing => "impluvium.v2.AuthService/PollPairing",
            Self::BeginSession => "impluvium.v2.AuthService/BeginSession",
            Self::CompleteSession => "impluvium.v2.AuthService/CompleteSession",
            Self::RevokeSelf => "impluvium.v2.AuthService/RevokeSelf",
            Self::ListDevices => "impluvium.v2.DeviceService/ListDevices",
            Self::RevokeDevice => "impluvium.v2.DeviceService/RevokeDevice",
            Self::ListLibrary => "impluvium.v2.LibraryService/ListLibrary",
            Self::GetGame => "impluvium.v2.LibraryService/GetGame",
            Self::ListVersions => "impluvium.v2.LibraryService/ListVersions",
            Self::GetVersion => "impluvium.v2.LibraryService/GetVersion",
            Self::OpenDownload => "impluvium.v2.DownloadService/OpenDownload",
            Self::RefreshTicket => "impluvium.v2.DownloadService/RefreshTicket",
        }
    }
    pub const fn credential(self) -> CredentialPurpose {
        match self {
            Self::StartPairing | Self::BeginSession => CredentialPurpose::None,
            Self::PollPairing | Self::CompleteSession => CredentialPurpose::Proof,
            _ => CredentialPurpose::AccessToken,
        }
    }
    pub const fn response_limit(self) -> usize {
        match self {
            Self::StartPairing
            | Self::PollPairing
            | Self::BeginSession
            | Self::CompleteSession
            | Self::RevokeSelf => 65_536,
            Self::ListDevices | Self::RevokeDevice => 1_048_576,
            Self::ListLibrary | Self::GetGame | Self::ListVersions | Self::GetVersion => 4_194_304,
            Self::OpenDownload | Self::RefreshTicket => 2_097_152,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Purpose {
    Discovery,
    Rpc(Method),
    Manifest,
    Descriptor,
    Object,
    Events,
    Chunk,
}
impl Purpose {
    pub const fn credential(self) -> CredentialPurpose {
        match self {
            Self::Discovery => CredentialPurpose::None,
            Self::Rpc(method) => method.credential(),
            Self::Chunk => CredentialPurpose::Ticket,
            _ => CredentialPurpose::AccessToken,
        }
    }
}

/// Exact permitted chunk URLs for one authenticated download. Constructed from
/// authenticated endpoints intersected with current public origin configuration.
#[derive(Clone, Debug, Default)]
pub struct DepotTargets(Vec<Url>);
impl DepotTargets {
    pub fn for_chunk(
        download: &impluvium_protocol::api::Download,
        origins: &[Origin],
        manifest: &[u8; 32],
        chunk: &[u8; 32],
    ) -> Self {
        if !download
            .response()
            .manifest_ids
            .iter()
            .any(|id| id.as_slice() == manifest)
        {
            return Self::default();
        }
        let mut endpoints: Vec<_> = download.response().depot_endpoints.iter().collect();
        endpoints.sort_by_key(|endpoint| endpoint.priority);
        let mut targets = Vec::new();
        for endpoint in endpoints {
            if let Ok(base) = BaseUrl::parse(&endpoint.base_url)
                && origins.contains(&base.origin())
            {
                let text = format!(
                    "{}/depot/v2/{}/{}",
                    base.serialized(),
                    hex(manifest),
                    hex(chunk)
                );
                if let Ok(url) = request_url(&text) {
                    targets.push(url);
                }
            }
        }
        Self(targets)
    }
    /// Public policy substage: each URL must already have been derived from an
    /// authenticated endpoint, its manifest set and the current configuration.
    pub fn from_allowed_urls(urls: &[&str]) -> Result<Self, Diagnostic> {
        Ok(Self(
            urls.iter()
                .map(|s| request_url(s))
                .collect::<Result<_, _>>()?,
        ))
    }
    pub fn urls(&self) -> &[Url] {
        &self.0
    }
    fn contains(&self, url: &Url) -> bool {
        let mut parts = url.path().rsplit('/');
        let id = |s: &str| {
            s.len() == 64
                && s.bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        };
        url.query().is_none()
            && parts.next().is_some_and(id)
            && parts.next().is_some_and(id)
            && parts.next() == Some("v2")
            && parts.next() == Some("depot")
            && self.0.contains(url)
    }
}
fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        out.push(char::from(DIGITS[(byte >> 4) as usize]));
        out.push(char::from(DIGITS[(byte & 15) as usize]));
    }
    out
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TrustState {
    Trusted,
    InstanceMismatch,
    ContentKeyChanged,
}
impl TrustState {
    pub fn diagnostic(self) -> Option<Diagnostic> {
        match self {
            Self::Trusted => None,
            Self::InstanceMismatch => Some(local(Reason::instance_mismatch)),
            Self::ContentKeyChanged => Some(local(Reason::content_key_changed)),
        }
    }
}
pub struct Policy<'a> {
    pub base: &'a str,
    pub platform_trust_available: bool,
    pub insecure_mode: bool,
    pub depots: &'a DepotTargets,
    pub trust: TrustState,
}

/// Authorization to establish a credential-free connection. Response rules are
/// already fixed. This is not evidence that credentials may yet be transferred.
#[derive(Clone, Debug)]
pub struct PreparedRequest {
    url: Url,
    base: BaseUrl,
    purpose: Purpose,
    binding: Binding,
}
impl PreparedRequest {
    pub fn url(&self) -> &Url {
        &self.url
    }
    pub fn base(&self) -> &BaseUrl {
        &self.base
    }
    pub fn purpose(&self) -> Purpose {
        self.purpose
    }
    pub fn binding(&self) -> &Binding {
        &self.binding
    }
}

pub fn preflight(
    policy: &Policy<'_>,
    url: &str,
    binding: Binding,
) -> Result<PreparedRequest, Diagnostic> {
    let base = BaseUrl::parse(policy.base)?;
    let url = request_url(url)?;
    let purpose = binding.purpose();
    if !policy.platform_trust_available {
        return Err(local(Reason::platform_trust_unavailable));
    }
    if url.scheme() == "http" && !policy.insecure_mode && !is_loopback(&url) {
        return Err(local(Reason::insecure_scheme_not_allowed));
    }
    let api = format!("{}/api/v2/", base.prefix());
    let path = url.path();
    let allowed = match purpose {
        Purpose::Chunk => {
            policy.depots.contains(&url)
                && binding
                    .chunk_id()
                    .is_some_and(|id| url.path().rsplit('/').next() == Some(hex(id).as_str()))
        }
        Purpose::Discovery => {
            Origin::of(&url) == base.origin()
                && path == format!("{}/api/info", base.prefix())
                && url.query().is_none()
        }
        Purpose::Rpc(method) => {
            Origin::of(&url) == base.origin()
                && path == format!("{api}{}", method.route())
                && url.query().is_none()
        }
        Purpose::Manifest | Purpose::Descriptor | Purpose::Object => {
            let directory = match purpose {
                Purpose::Manifest => "manifests",
                Purpose::Descriptor => "descriptors",
                _ => "objects",
            };
            Origin::of(&url) == base.origin()
                && path
                    .strip_prefix(&format!("{api}{directory}/"))
                    .is_some_and(|id| !id.is_empty() && !id.contains('/'))
                && url.query().is_none()
        }
        Purpose::Events => Origin::of(&url) == base.origin() && path == format!("{api}events"),
    };
    if !allowed {
        return Err(local(Reason::origin_not_allowed));
    }
    if purpose.credential() != CredentialPurpose::None
        && let Some(d) = policy.trust.diagnostic()
    {
        return Err(d);
    }
    Ok(PreparedRequest {
        url,
        base,
        purpose,
        binding,
    })
}

pub fn redirect(
    policy: &Policy<'_>,
    original: &PreparedRequest,
    current: &Url,
    followed: usize,
    location: &str,
) -> Result<PreparedRequest, Diagnostic> {
    if original.purpose != Purpose::Chunk {
        return Err(local(Reason::redirect_not_allowed));
    }
    if followed >= 5 {
        return Err(local(Reason::redirect_limit));
    }
    let target = current
        .join(location)
        .map_err(|_| local(Reason::invalid_url))?;
    validate_url(&target)?;
    if Origin::of(&target) != Origin::of(&original.url) {
        return Err(local(Reason::redirect_cross_origin));
    }
    // Allowed endpoint paths may differ, but both resource identifiers must stay fixed.
    let tail = |url: &Url| url.path().rsplit('/').take(2).collect::<Vec<_>>().join("/");
    if !policy.depots.contains(&target) || tail(&target) != tail(&original.url) {
        return Err(local(Reason::redirect_target_not_allowed));
    }
    preflight(policy, target.as_str(), original.binding.clone())
}
