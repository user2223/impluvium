// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Translation of validated configuration into the settings of the HTTP
//! boundary: the external origin and the trusted proxy networks.
//!
//! The insecure-LAN switch is not passed on. It only decides whether a plain
//! http origin may be configured at all; cookie attributes follow the scheme
//! of the origin.

use server_http::auth::{ExternalOrigin, OriginError, Scheme as HttpScheme};
use server_http::proxy::{InvalidNetwork, Network, TrustedProxies};

use super::model::{Cidr, ExternalUrl, Scheme};

/// The external origin as the HTTP boundary uses it.
pub fn external_origin(url: &ExternalUrl) -> Result<ExternalOrigin, OriginError> {
    let scheme = match url.scheme {
        Scheme::Http => HttpScheme::Http,
        Scheme::Https => HttpScheme::Https,
    };
    ExternalOrigin::new(scheme, &url.host, url.port)
}

/// The networks whose forwarding headers are trusted.
pub fn trusted_proxies(networks: &[Cidr]) -> Result<TrustedProxies, InvalidNetwork> {
    let mut parsed = Vec::with_capacity(networks.len());
    for network in networks {
        parsed.push(Network::new(network.addr, network.prefix)?);
    }
    Ok(TrustedProxies::new(parsed))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(scheme: Scheme, host: &str, port: Option<u16>) -> ExternalUrl {
        ExternalUrl {
            scheme,
            host: host.to_owned(),
            port,
        }
    }

    #[test]
    fn origin_follows_the_configured_url() {
        let https = external_origin(&url(Scheme::Https, "impluvium.example", None)).unwrap();
        assert_eq!(https.as_str(), "https://impluvium.example");
        assert!(https.is_https());
        let plain = external_origin(&url(Scheme::Http, "[::1]", Some(8080))).unwrap();
        assert_eq!(plain.as_str(), "http://[::1]:8080");
        assert!(!plain.is_https());
    }

    #[test]
    fn proxy_networks_are_carried_over() {
        let networks = [
            Cidr {
                addr: "192.0.2.0".parse().unwrap(),
                prefix: 24,
            },
            Cidr {
                addr: "2001:db8::".parse().unwrap(),
                prefix: 32,
            },
        ];
        let proxies = trusted_proxies(&networks).unwrap();
        let expected = TrustedProxies::new(vec![
            Network::new("192.0.2.0".parse().unwrap(), 24).unwrap(),
            Network::new("2001:db8::".parse().unwrap(), 32).unwrap(),
        ]);
        assert_eq!(proxies, expected);
        assert_eq!(trusted_proxies(&[]).unwrap(), TrustedProxies::default());
    }
}
