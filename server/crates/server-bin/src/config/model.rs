// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! The typed configuration and the parsers behind the field kinds.
//!
//! Parsers return messages that describe the rule and never repeat the text
//! they rejected.

use std::fmt;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize, Serializer};
use serde_json::Value;

use super::fields::Kind;

/// A value that must not appear in output.
#[derive(Clone, PartialEq, Eq, Deserialize)]
#[serde(transparent)]
pub struct Secret(String);

impl Secret {
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Secret(<redacted>)")
    }
}

impl Serialize for Secret {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str("<redacted>")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Scheme {
    Http,
    Https,
}

/// What `ExternalUrl::is_loopback` accepts, in words for the reference.
pub const LOOPBACK_DESCRIPTION: &str =
    "`localhost`, any name ending in `.localhost`, `127.0.0.0/8` and `::1`";

/// The public origin of the server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExternalUrl {
    pub scheme: Scheme,
    /// Lowercase host name, IPv4 address or bracketed IPv6 address.
    pub host: String,
    /// Absent when it is the default port of the scheme.
    pub port: Option<u16>,
}

impl ExternalUrl {
    pub fn origin(&self) -> String {
        let scheme = match self.scheme {
            Scheme::Http => "http",
            Scheme::Https => "https",
        };
        match self.port {
            Some(port) => format!("{scheme}://{}:{port}", self.host),
            None => format!("{scheme}://{}", self.host),
        }
    }

    /// Whether the host is this machine, which is the one place plain http is
    /// always acceptable: see `LOOPBACK_DESCRIPTION`.
    pub fn is_loopback(&self) -> bool {
        if self.host == "localhost" || self.host.ends_with(".localhost") {
            return true;
        }
        let inner = self.host.trim_start_matches('[').trim_end_matches(']');
        inner
            .parse::<IpAddr>()
            .is_ok_and(|address| address.is_loopback())
    }
}

/// A network whose address has no bits set below the prefix length.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cidr {
    pub addr: IpAddr,
    pub prefix: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogFormat {
    Text,
    Json,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BackupMode {
    Auto,
    Command,
    Off,
}

/// The validated server configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub external_url: ExternalUrl,
    pub data_dir: PathBuf,
    pub library_dir: Option<PathBuf>,
    #[serde(default)]
    pub trusted_proxies: Vec<Cidr>,
    pub insecure_lan: bool,
    pub database_url: Secret,
    pub db_pool_max: u32,
    pub db_pool_min: u32,
    pub db_timeout_secs: u32,
    pub spa_dir: Option<PathBuf>,
    pub log_filter: String,
    pub log_format: LogFormat,
    pub backup_mode: BackupMode,
    #[serde(default, serialize_with = "redact_arguments")]
    pub backup_command: Vec<String>,
    pub backup_timeout_secs: u32,
    pub listen_addr: SocketAddr,
    /// A validated instance identifier.
    pub accept_new_database: Option<String>,
}

/// Shows the program and one placeholder per argument: arguments may carry
/// secrets, and the count is all that `config check` needs to show.
fn redact_arguments<S: Serializer>(arguments: &[String], serializer: S) -> Result<S::Ok, S::Error> {
    let mut shown: Vec<&str> = Vec::new();
    for (index, argument) in arguments.iter().enumerate() {
        shown.push(if index == 0 {
            argument.as_str()
        } else {
            "<redacted>"
        });
    }
    serializer.collect_seq(shown)
}

impl Config {
    /// The library directory, by default `library` inside the data directory.
    pub fn library_dir(&self) -> PathBuf {
        match &self.library_dir {
            Some(directory) => directory.clone(),
            None => self.data_dir.join("library"),
        }
    }
}

fn fail<T>(message: &str) -> Result<T, String> {
    Err(message.to_owned())
}

fn encode<T: Serialize>(item: &T) -> Result<Value, String> {
    match serde_json::to_value(item) {
        Ok(value) => Ok(value),
        Err(_) => fail("cannot be represented"),
    }
}

/// Parses the text of one variable into the JSON shape of its typed field.
pub fn parse(kind: Kind, raw: &str) -> Result<Value, String> {
    match kind {
        Kind::Origin => encode(&parse_origin(raw)?),
        Kind::Directory => parse_directory(raw).map(Value::String),
        Kind::Cidrs => encode(&parse_cidrs(raw)?),
        Kind::Flag => parse_flag(raw).map(Value::Bool),
        Kind::Number { min, max } => parse_number(raw, min, max).map(Value::from),
        Kind::DatabaseUrl => parse_database_url(raw).map(Value::String),
        Kind::LogFilter => parse_log_filter(raw).map(Value::String),
        Kind::Choice(words) => parse_choice(raw, words).map(Value::String),
        Kind::Arguments => parse_arguments(raw).map(Value::from),
        Kind::SocketAddress => parse_socket_address(raw).map(Value::String),
        Kind::InstanceId => parse_instance_id(raw).map(Value::String),
    }
}

fn parse_socket_address(raw: &str) -> Result<String, String> {
    match raw.parse::<SocketAddr>() {
        Ok(address) if address.port() != 0 => Ok(address.to_string()),
        Ok(_) => fail("must use a port from 1 to 65535"),
        Err(_) => fail("must be an IP address and a port, such as 127.0.0.1:8080 or [::1]:8080"),
    }
}

fn parse_instance_id(raw: &str) -> Result<String, String> {
    match server_db::instance::InstanceId::parse(raw) {
        Some(_) => Ok(raw.to_owned()),
        None => fail(
            "must be an instance identifier: a lowercase UUID, as written in the data directory marker file",
        ),
    }
}

fn parse_origin(raw: &str) -> Result<ExternalUrl, String> {
    let (scheme, rest) = if let Some(rest) = raw.strip_prefix("https://") {
        (Scheme::Https, rest)
    } else if let Some(rest) = raw.strip_prefix("http://") {
        (Scheme::Http, rest)
    } else {
        return fail("must start with http:// or https://");
    };
    let authority = rest.strip_suffix('/').unwrap_or(rest);
    if authority.is_empty() {
        return fail("must name a host");
    }
    let forbidden =
        |c: char| matches!(c, '/' | '?' | '#' | '@' | '\\') || c.is_whitespace() || c.is_control();
    if authority.contains(forbidden) {
        return fail("must be an origin only: no path, query, credentials or whitespace");
    }
    let (host, port) = split_authority(authority)?;
    let port = match (scheme, port) {
        (Scheme::Http, Some(80)) | (Scheme::Https, Some(443)) => None,
        (_, port) => port,
    };
    Ok(ExternalUrl { scheme, host, port })
}

/// Splits host and optional port; the host is returned in normal form.
fn split_authority(authority: &str) -> Result<(String, Option<u16>), String> {
    if let Some(inner_and_rest) = authority.strip_prefix('[') {
        let Some((inner, after)) = inner_and_rest.split_once(']') else {
            return fail("has an unterminated IPv6 address");
        };
        let Ok(address) = inner.parse::<Ipv6Addr>() else {
            return fail("has an invalid IPv6 address");
        };
        let host = format!("[{address}]");
        if after.is_empty() {
            return Ok((host, None));
        }
        return match after.strip_prefix(':') {
            Some(port) => Ok((host, Some(parse_port(port)?))),
            None => fail("has text after the IPv6 address that is not a port"),
        };
    }
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) => (host, Some(parse_port(port)?)),
        None => (authority, None),
    };
    if host.contains(':') {
        return fail("must put an IPv6 address in square brackets");
    }
    let host = host.to_ascii_lowercase();
    if host.parse::<Ipv4Addr>().is_ok() || valid_domain(&host) {
        Ok((host, port))
    } else {
        fail("has an invalid host name")
    }
}

fn parse_port(text: &str) -> Result<u16, String> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return fail("has an invalid port");
    }
    match text.parse::<u16>() {
        Ok(port) if port != 0 => Ok(port),
        _ => fail("has a port outside 1 to 65535"),
    }
}

fn valid_domain(host: &str) -> bool {
    if host.is_empty() || host.len() > 253 {
        return false;
    }
    let labels: Vec<&str> = host.split('.').collect();
    let valid_label = |label: &&str| {
        !label.is_empty()
            && label.len() <= 63
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    };
    // A last label of digits only is a malformed IPv4 address, not a name.
    let numeric_last = labels
        .last()
        .is_some_and(|label| label.bytes().all(|b| b.is_ascii_digit()));
    labels.iter().all(valid_label) && !numeric_last
}

fn parse_directory(raw: &str) -> Result<String, String> {
    if raw.is_empty() {
        return fail("must not be empty");
    }
    if raw.chars().any(char::is_control) {
        return fail("must not contain control characters");
    }
    if !Path::new(raw).is_absolute() {
        return fail("must be an absolute path");
    }
    Ok(raw.to_owned())
}

fn parse_cidrs(raw: &str) -> Result<Vec<Cidr>, String> {
    let mut networks = Vec::new();
    if raw.trim().is_empty() {
        return Ok(networks);
    }
    for entry in raw.split(',') {
        let entry = entry.trim();
        if entry.is_empty() {
            return fail("contains an empty entry");
        }
        networks.push(parse_cidr(entry)?);
    }
    Ok(networks)
}

fn parse_cidr(entry: &str) -> Result<Cidr, String> {
    let Some((address, prefix)) = entry.split_once('/') else {
        return fail("entries must be networks in CIDR form, such as 10.0.0.0/8");
    };
    let Ok(addr) = address.parse::<IpAddr>() else {
        return fail("contains an invalid network address");
    };
    if prefix.is_empty() || !prefix.bytes().all(|b| b.is_ascii_digit()) {
        return fail("contains an invalid prefix length");
    }
    let longest: u8 = if addr.is_ipv4() { 32 } else { 128 };
    let prefix = match prefix.parse::<u8>() {
        Ok(prefix) if prefix <= longest => prefix,
        _ => return fail("contains a prefix length that is too large"),
    };
    if prefix == 0 {
        return fail("contains a prefix length of 0, which would trust every address");
    }
    if let IpAddr::V6(v6) = addr
        && overlaps_mapped_range(v6, prefix)
    {
        return fail("contains an IPv4-mapped IPv6 network; write IPv4 networks as IPv4");
    }
    if host_bits_set(addr, prefix) {
        return fail("contains a network address with bits set below the prefix length");
    }
    Ok(Cidr { addr, prefix })
}

/// Whether the network shares addresses with `::ffff:0:0/96`, where IPv6
/// stands for IPv4: such a network would sidestep the checks on IPv4 networks.
fn overlaps_mapped_range(addr: Ipv6Addr, prefix: u8) -> bool {
    let compared = u32::from(prefix.min(96));
    let mask = u128::MAX.checked_shl(128 - compared).unwrap_or(0);
    let mapped: u128 = 0xffff << 32;
    ((u128::from(addr) ^ mapped) & mask) == 0
}

fn host_bits_set(addr: IpAddr, prefix: u8) -> bool {
    let free = u32::from(prefix);
    match addr {
        IpAddr::V4(v4) => {
            let mask = u32::MAX.checked_shl(32 - free).unwrap_or(0);
            (u32::from(v4) & !mask) != 0
        }
        IpAddr::V6(v6) => {
            let mask = u128::MAX.checked_shl(128 - free).unwrap_or(0);
            (u128::from(v6) & !mask) != 0
        }
    }
}

fn parse_flag(raw: &str) -> Result<bool, String> {
    match raw {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => fail("must be true or false"),
    }
}

fn parse_number(raw: &str, min: u32, max: u32) -> Result<u32, String> {
    let message = format!("must be a whole number from {min} to {max}");
    if raw.is_empty() || raw.len() > 10 || !raw.bytes().all(|b| b.is_ascii_digit()) {
        return Err(message);
    }
    match raw.parse::<u32>() {
        Ok(number) if (min..=max).contains(&number) => Ok(number),
        _ => Err(message),
    }
}

fn parse_database_url(raw: &str) -> Result<String, String> {
    let rest = match raw.strip_prefix("postgres://") {
        Some(rest) => Some(rest),
        None => raw.strip_prefix("postgresql://"),
    };
    match rest {
        Some(rest)
            if !rest.is_empty() && !rest.chars().any(|c| c.is_whitespace() || c.is_control()) =>
        {
            Ok(raw.to_owned())
        }
        _ => fail("must be a postgres:// or postgresql:// URL without whitespace"),
    }
}

const LOG_LEVELS: [&str; 6] = ["off", "error", "warn", "info", "debug", "trace"];

fn parse_log_filter(raw: &str) -> Result<String, String> {
    let message = "must be a level, or target=level directives separated by commas";
    for directive in raw.split(',') {
        let (target, level) = match directive.split_once('=') {
            Some((target, level)) => (Some(target), level),
            None => (None, directive),
        };
        if !LOG_LEVELS.contains(&level) {
            return fail(message);
        }
        let name_ok = |c: char| c.is_ascii_alphanumeric() || c == '_' || c == ':';
        if let Some(target) = target
            && (target.is_empty() || !target.chars().all(name_ok))
        {
            return fail(message);
        }
    }
    Ok(raw.to_owned())
}

fn parse_choice(raw: &str, words: &[&str]) -> Result<String, String> {
    if words.contains(&raw) {
        Ok(raw.to_owned())
    } else {
        Err(format!("must be one of: {}", words.join(", ")))
    }
}

const MAX_ARGUMENTS: usize = 64;
const MAX_ARGUMENT_BYTES: usize = 4096;

fn parse_arguments(raw: &str) -> Result<Vec<String>, String> {
    let Ok(arguments) = serde_json::from_str::<Vec<String>>(raw) else {
        return fail("must be a JSON array of strings");
    };
    if arguments.is_empty() || arguments.len() > MAX_ARGUMENTS {
        return fail("must hold 1 to 64 arguments");
    }
    let bad = |argument: &String| {
        argument.is_empty() || argument.len() > MAX_ARGUMENT_BYTES || argument.contains('\0')
    };
    if arguments.iter().any(bad) {
        return fail("arguments must be 1 to 4096 bytes without NUL characters");
    }
    Ok(arguments)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origins_are_normalised() {
        let url = parse_origin("https://Impluvium.Example:443/").unwrap();
        assert_eq!(url.origin(), "https://impluvium.example");
        let url = parse_origin("http://[::1]:8080").unwrap();
        assert_eq!(url.origin(), "http://[::1]:8080");
        assert!(url.is_loopback());
        assert!(parse_origin("http://localhost").unwrap().is_loopback());
        assert!(!parse_origin("http://10.0.0.5").unwrap().is_loopback());
    }

    #[test]
    fn malformed_origins_are_rejected() {
        let bad = [
            "impluvium.example",
            "ftp://impluvium.example",
            "https://",
            "https://impluvium.example/path",
            "https://impluvium.example?x=1",
            "https://user@impluvium.example",
            "https://impluvium.example:0",
            "https://impluvium.example:70000",
            "https://impluvium.example:80a",
            "https://-impluvium.example",
            "https://impluvium..example",
            "https://999.1.1.300",
            "https://::1",
            "https://[::1",
            "https://[::1]x",
            "https://impluvium example",
        ];
        for text in bad {
            assert!(parse_origin(text).is_err(), "accepted a bad origin: {text}");
        }
    }

    #[test]
    fn networks_are_checked() {
        let list = parse_cidrs("10.0.0.0/8, fd00::/8").unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].prefix, 8);
        assert!(parse_cidrs("").unwrap().is_empty());
        assert!(parse_cidrs("8000::/1, 2001:db8::/32, ::1/128").is_ok());
        let bad = [
            "10.0.0.1",
            "10.0.0.1/8",
            "10.0.0.0/33",
            "fd00::/129",
            "10.0.0.0/0",
            "10.0.0.0/",
            "10.0.0.0/+8",
            "nonsense/8",
            "10.0.0.0/8,",
            "::ffff:0:0/96",
            "::ffff:10.0.0.0/104",
            "::ffff:10.0.0.0/120",
            "::/1",
            "::ffff:0:0/80",
        ];
        for text in bad {
            assert!(parse_cidrs(text).is_err(), "accepted a bad network: {text}");
        }
    }

    #[test]
    fn numbers_and_flags_are_strict() {
        assert_eq!(parse_number("10", 1, 500), Ok(10));
        for text in ["", "0", "501", "+5", "1e2", " 5", "99999999999"] {
            assert!(parse_number(text, 1, 500).is_err(), "accepted: {text}");
        }
        assert_eq!(parse_flag("true"), Ok(true));
        assert!(parse_flag("True").is_err());
        assert!(parse_flag("1").is_err());
    }

    #[test]
    fn log_filters_are_checked() {
        for text in ["info", "warn,server_http=debug", "off"] {
            assert!(parse_log_filter(text).is_ok(), "rejected: {text}");
        }
        for text in ["", "loud", "info,", "=info", "a b=info", "x=loud"] {
            assert!(parse_log_filter(text).is_err(), "accepted: {text}");
        }
    }

    #[test]
    fn arguments_are_checked() {
        assert!(parse_arguments(r#"["/usr/bin/backup", "--all"]"#).is_ok());
        for text in ["", "[]", r#"[""]"#, "[1]", r#"{"a":1}"#, "backup --all"] {
            assert!(parse_arguments(text).is_err(), "accepted: {text}");
        }
    }

    #[test]
    fn database_urls_are_checked_without_connecting() {
        assert!(parse_database_url("postgres://u:p@db/impluvium").is_ok());
        assert!(parse_database_url("postgresql://db/impluvium").is_ok());
        for text in ["", "mysql://db/x", "postgres://", "postgres://db /x"] {
            assert!(parse_database_url(text).is_err(), "accepted: {text}");
        }
    }

    #[test]
    fn secrets_stay_out_of_debug_and_serialised_output() {
        let secret: Secret = serde_json::from_str("\"hunter2-marker\"").unwrap();
        assert_eq!(secret.expose(), "hunter2-marker");
        assert!(!format!("{secret:?}").contains("hunter2"));
        assert!(!serde_json::to_string(&secret).unwrap().contains("hunter2"));
    }
}
