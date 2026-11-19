// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Resolution of the client address behind reverse proxies.
//!
//! The socket address is the default. A forwarding chain is read only when
//! the immediate peer lies in a configured trusted network. The chain is
//! walked from the right through trusted hops and stops at the first hop
//! that is not trusted. Headers of any other peer change nothing. A trusted
//! peer must use exactly one of `Forwarded` and `X-Forwarded-For`; sending
//! both, or a malformed chain, is an error and the request is rejected.
//! Addresses are compared in normal form: an IPv4-mapped IPv6 address
//! (`::ffff:192.0.2.1`) stands for the IPv4 address.

use std::error::Error;
use std::fmt;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

use axum::http::HeaderMap;

/// Largest number of hops accepted in a forwarding chain.
const MAX_HOPS: usize = 32;

/// A trusted network: an address and a prefix length of at least one bit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Network {
    addr: IpAddr,
    prefix: u8,
}

/// An invalid network definition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidNetwork;

impl fmt::Display for InvalidNetwork {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a trusted network needs a prefix length from 1 to the address size")
    }
}

impl Error for InvalidNetwork {}

impl Network {
    /// A network from an address and a prefix length. A prefix of zero would
    /// trust every address and is refused. A network written as IPv4-mapped
    /// IPv6 is converted to IPv4; one that is wider than the mapped range is
    /// refused.
    pub fn new(addr: IpAddr, prefix: u8) -> Result<Self, InvalidNetwork> {
        let (addr, prefix) = match addr {
            IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
                Some(v4) if prefix >= 96 => (IpAddr::V4(v4), prefix - 96),
                Some(_) => return Err(InvalidNetwork),
                None => (addr, prefix),
            },
            IpAddr::V4(_) => (addr, prefix),
        };
        let longest = if addr.is_ipv4() { 32 } else { 128 };
        if prefix == 0 || prefix > longest {
            return Err(InvalidNetwork);
        }
        Ok(Self { addr, prefix })
    }

    fn contains(&self, ip: IpAddr) -> bool {
        match (self.addr, ip) {
            (IpAddr::V4(network), IpAddr::V4(candidate)) => {
                let mask = u32::MAX
                    .checked_shl(32 - u32::from(self.prefix))
                    .unwrap_or(0);
                u32::from(network) & mask == u32::from(candidate) & mask
            }
            (IpAddr::V6(network), IpAddr::V6(candidate)) => {
                let mask = u128::MAX
                    .checked_shl(128 - u32::from(self.prefix))
                    .unwrap_or(0);
                u128::from(network) & mask == u128::from(candidate) & mask
            }
            _ => false,
        }
    }
}

/// The networks of the reverse proxies whose forwarding headers are trusted.
/// The default trusts none.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TrustedProxies(Vec<Network>);

impl TrustedProxies {
    /// Trusts the given networks.
    pub fn new(networks: Vec<Network>) -> Self {
        Self(networks)
    }

    fn contains(&self, ip: IpAddr) -> bool {
        let ip = normalize(ip);
        self.0.iter().any(|network| network.contains(ip))
    }
}

/// The normal form of an address: IPv4-mapped IPv6 becomes IPv4.
pub fn normalize(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(v6) => v6.to_ipv4_mapped().map_or(ip, IpAddr::V4),
        IpAddr::V4(_) => ip,
    }
}

/// Why forwarding headers of a trusted proxy were rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForwardingError {
    /// Both `Forwarded` and `X-Forwarded-For` are present.
    Conflicting,
    /// A header is not text, or an entry is empty, unparseable or obfuscated.
    Malformed,
    /// More hops than the limit.
    TooLong,
}

/// The address of the client.
pub fn client_ip(
    trusted: &TrustedProxies,
    peer: SocketAddr,
    headers: &HeaderMap,
) -> Result<IpAddr, ForwardingError> {
    let peer = normalize(peer.ip());
    if !trusted.contains(peer) {
        return Ok(peer);
    }
    let has_forwarded = headers.contains_key("forwarded");
    let has_forwarded_for = headers.contains_key("x-forwarded-for");
    let chain = match (has_forwarded, has_forwarded_for) {
        (false, false) => return Ok(peer),
        (true, true) => return Err(ForwardingError::Conflicting),
        (true, false) => forwarded_chain(headers)?,
        (false, true) => forwarded_for_chain(headers)?,
    };
    if chain.len() > MAX_HOPS {
        return Err(ForwardingError::TooLong);
    }
    let mut client = peer;
    for hop in chain.into_iter().rev() {
        client = hop;
        if !trusted.contains(hop) {
            break;
        }
    }
    Ok(client)
}

fn forwarded_for_chain(headers: &HeaderMap) -> Result<Vec<IpAddr>, ForwardingError> {
    let mut chain = Vec::new();
    for line in headers.get_all("x-forwarded-for") {
        let text = line.to_str().map_err(|_| ForwardingError::Malformed)?;
        for entry in text.split(',') {
            let address = entry
                .trim()
                .parse::<IpAddr>()
                .map_err(|_| ForwardingError::Malformed)?;
            chain.push(normalize(address));
            if chain.len() > MAX_HOPS {
                return Err(ForwardingError::TooLong);
            }
        }
    }
    Ok(chain)
}

fn forwarded_chain(headers: &HeaderMap) -> Result<Vec<IpAddr>, ForwardingError> {
    let mut chain = Vec::new();
    for line in headers.get_all("forwarded") {
        let text = line.to_str().map_err(|_| ForwardingError::Malformed)?;
        if text.contains('\\') {
            return Err(ForwardingError::Malformed);
        }
        for element in split_outside_quotes(text, ',')? {
            chain.push(normalize(forwarded_element(element)?));
            if chain.len() > MAX_HOPS {
                return Err(ForwardingError::TooLong);
            }
        }
    }
    Ok(chain)
}

/// The `for` address of one element of a `Forwarded` header.
fn forwarded_element(element: &str) -> Result<IpAddr, ForwardingError> {
    let mut found: Option<IpAddr> = None;
    for pair in split_outside_quotes(element, ';')? {
        let (name, value) = pair.split_once('=').ok_or(ForwardingError::Malformed)?;
        if !name.trim().eq_ignore_ascii_case("for") {
            continue;
        }
        if found.is_some() {
            return Err(ForwardingError::Malformed);
        }
        found = Some(node_address(unquote(value.trim())?)?);
    }
    found.ok_or(ForwardingError::Malformed)
}

fn unquote(value: &str) -> Result<&str, ForwardingError> {
    match value.strip_prefix('"') {
        Some(rest) => rest.strip_suffix('"').ok_or(ForwardingError::Malformed),
        None if value.contains('"') => Err(ForwardingError::Malformed),
        None => Ok(value),
    }
}

/// An IP address with an optional port. Names such as `unknown` and
/// obfuscated identifiers are not addresses and are rejected.
fn node_address(node: &str) -> Result<IpAddr, ForwardingError> {
    if let Some(rest) = node.strip_prefix('[') {
        let (inner, after) = rest.split_once(']').ok_or(ForwardingError::Malformed)?;
        check_port(after.strip_prefix(':'), after.is_empty())?;
        let address = inner
            .parse::<Ipv6Addr>()
            .map_err(|_| ForwardingError::Malformed)?;
        return Ok(IpAddr::V6(address));
    }
    let mut parts = node.split(':');
    let host = parts.next().unwrap_or_default();
    let port = parts.next();
    if parts.next().is_some() {
        // An unbracketed IPv6 address.
        return Err(ForwardingError::Malformed);
    }
    check_port(port, port.is_none())?;
    host.parse::<Ipv4Addr>()
        .map(IpAddr::V4)
        .map_err(|_| ForwardingError::Malformed)
}

fn check_port(port: Option<&str>, absent_is_fine: bool) -> Result<(), ForwardingError> {
    match port {
        None if absent_is_fine => Ok(()),
        Some(digits)
            if (1..=5).contains(&digits.len()) && digits.bytes().all(|b| b.is_ascii_digit()) =>
        {
            Ok(())
        }
        _ => Err(ForwardingError::Malformed),
    }
}

fn split_outside_quotes(text: &str, separator: char) -> Result<Vec<&str>, ForwardingError> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut quoted = false;
    for (index, character) in text.char_indices() {
        if character == '"' {
            quoted = !quoted;
        } else if character == separator && !quoted {
            parts.push(&text[start..index]);
            start = index + character.len_utf8();
        }
    }
    if quoted {
        return Err(ForwardingError::Malformed);
    }
    parts.push(&text[start..]);
    Ok(parts)
}

#[cfg(test)]
mod tests {
    use axum::http::HeaderValue;

    use super::*;

    fn ip(text: &str) -> IpAddr {
        text.parse().unwrap()
    }

    fn proxies(networks: &[&str]) -> TrustedProxies {
        TrustedProxies::new(
            networks
                .iter()
                .map(|text| {
                    let (address, prefix) = text.split_once('/').unwrap();
                    Network::new(ip(address), prefix.parse().unwrap()).unwrap()
                })
                .collect(),
        )
    }

    fn headers(pairs: &[(&'static str, &str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (name, value) in pairs {
            map.append(*name, HeaderValue::from_str(value).unwrap());
        }
        map
    }

    fn resolve(
        trusted: &TrustedProxies,
        peer: &str,
        pairs: &[(&'static str, &str)],
    ) -> Result<IpAddr, ForwardingError> {
        client_ip(trusted, SocketAddr::new(ip(peer), 4000), &headers(pairs))
    }

    #[test]
    fn forged_headers_of_an_untrusted_peer_change_nothing() {
        let trusted = proxies(&["192.0.2.0/24"]);
        for pairs in [
            &[("x-forwarded-for", "203.0.113.9")][..],
            &[("forwarded", "for=203.0.113.9")][..],
            &[("x-forwarded-for", "garbage"), ("forwarded", "garbage")][..],
        ] {
            assert_eq!(
                resolve(&trusted, "198.51.100.7", pairs),
                Ok(ip("198.51.100.7"))
            );
        }
        // Without any trusted network nobody is trusted.
        assert_eq!(
            resolve(
                &TrustedProxies::default(),
                "192.0.2.5",
                &[("x-forwarded-for", "203.0.113.9")]
            ),
            Ok(ip("192.0.2.5"))
        );
    }

    #[test]
    fn trusted_peer_without_headers_is_the_client() {
        let trusted = proxies(&["192.0.2.0/24"]);
        assert_eq!(resolve(&trusted, "192.0.2.5", &[]), Ok(ip("192.0.2.5")));
    }

    #[test]
    fn trusted_peer_names_the_client_from_the_right() {
        let trusted = proxies(&["192.0.2.0/24", "2001:db8:ffff::/48"]);
        let via = |chain: &str| resolve(&trusted, "192.0.2.5", &[("x-forwarded-for", chain)]);
        assert_eq!(via("203.0.113.9"), Ok(ip("203.0.113.9")));
        // A spoofed left part is ignored: the walk stops at the first
        // untrusted hop from the right.
        assert_eq!(via("198.51.100.1, 203.0.113.9"), Ok(ip("203.0.113.9")));
        // Several trusted hops are skipped.
        assert_eq!(
            via("203.0.113.9, 192.0.2.77, 2001:db8:ffff::1"),
            Ok(ip("203.0.113.9"))
        );
        // When every hop is trusted the leftmost one is the client.
        assert_eq!(via("192.0.2.1, 192.0.2.2"), Ok(ip("192.0.2.1")));
    }

    #[test]
    fn ipv6_clients_and_peers_work() {
        let trusted = proxies(&["2001:db8:ffff::/48"]);
        assert_eq!(
            resolve(
                &trusted,
                "2001:db8:ffff::5",
                &[("x-forwarded-for", "2001:db8:1::9")]
            ),
            Ok(ip("2001:db8:1::9"))
        );
        assert_eq!(
            resolve(
                &trusted,
                "2001:db8:ffff::5",
                &[(
                    "forwarded",
                    "for=\"[2001:db8:1::9]:4711\", for=\"[2001:db8:ffff::2]\""
                )]
            ),
            Ok(ip("2001:db8:1::9"))
        );
        // A peer outside the network is not trusted, even in IPv6.
        assert_eq!(
            resolve(
                &trusted,
                "2001:db8:eeee::5",
                &[("x-forwarded-for", "203.0.113.9")]
            ),
            Ok(ip("2001:db8:eeee::5"))
        );
    }

    #[test]
    fn ipv4_mapped_addresses_are_normalized() {
        let trusted = proxies(&["192.0.2.0/24"]);
        // The peer arrives as a mapped IPv6 address (dual-stack listener).
        assert_eq!(
            resolve(
                &trusted,
                "::ffff:192.0.2.5",
                &[("x-forwarded-for", "::ffff:203.0.113.9")]
            ),
            Ok(ip("203.0.113.9"))
        );
        // A mapped hop that is trusted is skipped like its IPv4 form.
        assert_eq!(
            resolve(
                &trusted,
                "192.0.2.5",
                &[("x-forwarded-for", "203.0.113.9, ::ffff:192.0.2.77")]
            ),
            Ok(ip("203.0.113.9"))
        );
        // An untrusted mapped peer stays untrusted and is reported as IPv4.
        assert_eq!(
            resolve(
                &trusted,
                "::ffff:198.51.100.7",
                &[("x-forwarded-for", "203.0.113.9")]
            ),
            Ok(ip("198.51.100.7"))
        );
    }

    #[test]
    fn forwarded_header_is_read_with_ports_and_parameters() {
        let trusted = proxies(&["192.0.2.0/24"]);
        assert_eq!(
            resolve(
                &trusted,
                "192.0.2.5",
                &[(
                    "forwarded",
                    "For=203.0.113.9:5000;proto=https;by=192.0.2.5, for=192.0.2.9"
                )]
            ),
            Ok(ip("203.0.113.9"))
        );
    }

    #[test]
    fn both_header_families_from_a_trusted_peer_are_rejected() {
        let trusted = proxies(&["192.0.2.0/24"]);
        assert_eq!(
            resolve(
                &trusted,
                "192.0.2.5",
                &[
                    ("x-forwarded-for", "203.0.113.9"),
                    ("forwarded", "for=203.0.113.9")
                ]
            ),
            Err(ForwardingError::Conflicting)
        );
    }

    #[test]
    fn malformed_chains_are_rejected_even_left_of_the_stop() {
        let trusted = proxies(&["192.0.2.0/24"]);
        for chain in [
            "unknown",
            "203.0.113.9,",
            ", 203.0.113.9",
            "garbage, 203.0.113.9",
            "203.0.113.9:80",
            "203.0.113.999",
            "203.0.113.9 203.0.113.8",
        ] {
            assert_eq!(
                resolve(&trusted, "192.0.2.5", &[("x-forwarded-for", chain)]),
                Err(ForwardingError::Malformed),
                "{chain:?}"
            );
        }
        for value in [
            "for=unknown",
            "for=_hidden",
            "for=2001:db8::1",
            "proto=https",
            "for=203.0.113.9;for=203.0.113.8",
            "for=\"203.0.113.9",
            "for=203.0.113.9, ",
            "for",
            "for=203.0.113.9:port",
            "for=\"[2001:db8::1\"",
        ] {
            assert_eq!(
                resolve(&trusted, "192.0.2.5", &[("forwarded", value)]),
                Err(ForwardingError::Malformed),
                "{value:?}"
            );
        }
    }

    #[test]
    fn repeated_header_lines_form_one_chain() {
        let trusted = proxies(&["192.0.2.0/24"]);
        assert_eq!(
            resolve(
                &trusted,
                "192.0.2.5",
                &[
                    ("x-forwarded-for", "198.51.100.1"),
                    ("x-forwarded-for", "203.0.113.9")
                ]
            ),
            Ok(ip("203.0.113.9"))
        );
    }

    #[test]
    fn overlong_chains_are_rejected() {
        let trusted = proxies(&["192.0.2.0/24"]);
        let chain = vec!["203.0.113.9"; MAX_HOPS + 1].join(", ");
        assert_eq!(
            resolve(&trusted, "192.0.2.5", &[("x-forwarded-for", &chain)]),
            Err(ForwardingError::TooLong)
        );
        let allowed = vec!["203.0.113.9"; MAX_HOPS].join(", ");
        assert_eq!(
            resolve(&trusted, "192.0.2.5", &[("x-forwarded-for", &allowed)]),
            Ok(ip("203.0.113.9"))
        );
    }

    #[test]
    fn networks_are_validated_and_masked() {
        assert_eq!(Network::new(ip("192.0.2.0"), 0), Err(InvalidNetwork));
        assert_eq!(Network::new(ip("192.0.2.0"), 33), Err(InvalidNetwork));
        assert_eq!(Network::new(ip("2001:db8::"), 129), Err(InvalidNetwork));
        // A mapped network wider than the mapped range would trust IPv4 by accident.
        assert_eq!(Network::new(ip("::ffff:0:0"), 64), Err(InvalidNetwork));
        let mapped = Network::new(ip("::ffff:192.0.2.0"), 120).unwrap();
        assert_eq!(mapped, Network::new(ip("192.0.2.0"), 24).unwrap());
        let host = proxies(&["192.0.2.5/32"]);
        assert!(host.contains(ip("192.0.2.5")));
        assert!(!host.contains(ip("192.0.2.6")));
        assert!(!host.contains(ip("2001:db8::5")));
    }
}
