// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Turns a PostgreSQL URL into libpq environment variables.
//!
//! The PostgreSQL tools get their connection settings only through the
//! environment, never through their arguments, so that the password does not
//! show up in a process list. Errors never repeat any part of the URL.

use super::error::BackupError;

const SSL_MODES: [&str; 6] = [
    "disable",
    "allow",
    "prefer",
    "require",
    "verify-ca",
    "verify-full",
];

fn bad(reason: &'static str) -> BackupError {
    BackupError::Url(reason)
}

fn hex_digit(byte: u8) -> Option<u8> {
    char::from(byte)
        .to_digit(16)
        .and_then(|d| u8::try_from(d).ok())
}

/// Decodes `%XX` escapes. The result must be UTF-8 without NUL.
fn decode(text: &str) -> Result<String, BackupError> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let high = bytes.get(index + 1).copied().and_then(hex_digit);
            let low = bytes.get(index + 2).copied().and_then(hex_digit);
            match (high, low) {
                (Some(high), Some(low)) => out.push(high * 16 + low),
                _ => return Err(bad("a percent escape is malformed")),
            }
            index += 3;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    let text = String::from_utf8(out).map_err(|_| bad("a part of it is not valid UTF-8"))?;
    if text.contains('\0') {
        return Err(bad("a part of it contains a NUL character"));
    }
    Ok(text)
}

/// The libpq environment variables for a `postgres://` or `postgresql://` URL.
///
/// Supported query parameters: `sslmode`, `sslrootcert`, `sslcert`, `sslkey`, `host`
/// (a socket directory or a host name), `application_name` and `connect_timeout`.
/// Anything else is refused.
pub(crate) fn libpq_environment(url: &str) -> Result<Vec<(&'static str, String)>, BackupError> {
    let rest = url
        .strip_prefix("postgres://")
        .or_else(|| url.strip_prefix("postgresql://"))
        .ok_or_else(|| bad("it does not use the postgres or postgresql scheme"))?;
    if rest.contains('#') {
        return Err(bad("it contains a fragment"));
    }
    let (before_query, query) = match rest.split_once('?') {
        Some((head, query)) => (head, Some(query)),
        None => (rest, None),
    };
    let (authority, path) = before_query.split_once('/').unwrap_or((before_query, ""));
    let (userinfo, host_and_port) = match authority.rfind('@') {
        Some(at) => (Some(&authority[..at]), &authority[at + 1..]),
        None => (None, authority),
    };

    let (host, port) = if let Some(inner) = host_and_port.strip_prefix('[') {
        let end = inner
            .find(']')
            .ok_or_else(|| bad("an IPv6 address is not closed"))?;
        let after = &inner[end + 1..];
        let port = match after.strip_prefix(':') {
            Some(port) => Some(port),
            None if after.is_empty() => None,
            None => return Err(bad("the host part is malformed")),
        };
        (&inner[..end], port)
    } else {
        match host_and_port.rsplit_once(':') {
            Some((host, port)) if !host.contains(':') => (host, Some(port)),
            Some(_) => return Err(bad("the host part is malformed")),
            None => (host_and_port, None),
        }
    };

    let mut environment: Vec<(&'static str, String)> = Vec::new();
    let mut query_host: Option<String> = None;
    let mut seen: Vec<String> = Vec::new();
    for pair in query
        .unwrap_or("")
        .split('&')
        .filter(|pair| !pair.is_empty())
    {
        let (key, value) = pair
            .split_once('=')
            .ok_or_else(|| bad("a query parameter has no value"))?;
        let key = decode(key)?;
        let value = decode(value)?;
        if seen.contains(&key) {
            return Err(bad("a query parameter is given twice"));
        }
        let variable = match key.as_str() {
            "sslmode" => {
                if !SSL_MODES.contains(&value.as_str()) {
                    return Err(bad("sslmode has an unknown value"));
                }
                Some("PGSSLMODE")
            }
            "sslrootcert" => Some("PGSSLROOTCERT"),
            "sslcert" => Some("PGSSLCERT"),
            "sslkey" => Some("PGSSLKEY"),
            "application_name" => Some("PGAPPNAME"),
            "connect_timeout" => {
                if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
                    return Err(bad("connect_timeout is not a number"));
                }
                Some("PGCONNECT_TIMEOUT")
            }
            "host" => {
                query_host = Some(value.clone());
                None
            }
            _ => {
                return Err(bad("it has a query parameter that backups do not support \
                     (supported: sslmode, sslrootcert, sslcert, sslkey, host, \
                     application_name, connect_timeout)"));
            }
        };
        seen.push(key);
        if let Some(variable) = variable {
            environment.push((variable, value));
        }
    }

    let authority_host = decode(host)?;
    let host = match (authority_host.is_empty(), query_host) {
        (false, Some(_)) => return Err(bad("the host is given twice")),
        (true, Some(from_query)) => from_query,
        (_, None) => authority_host,
    };
    if !host.is_empty() {
        environment.push(("PGHOST", host));
    }
    if let Some(port) = port {
        match port.parse::<u16>() {
            Ok(number) if number != 0 => environment.push(("PGPORT", number.to_string())),
            _ => return Err(bad("the port is not a number from 1 to 65535")),
        }
    }
    if let Some(userinfo) = userinfo {
        let (user, password) = match userinfo.split_once(':') {
            Some((user, password)) => (user, Some(password)),
            None => (userinfo, None),
        };
        let user = decode(user)?;
        if !user.is_empty() {
            environment.push(("PGUSER", user));
        }
        if let Some(password) = password {
            let password = decode(password)?;
            if !password.is_empty() {
                environment.push(("PGPASSWORD", password));
            }
        }
    }
    if path.contains('/') {
        return Err(bad("the database name contains a slash"));
    }
    let database = decode(path)?;
    if !database.is_empty() {
        environment.push(("PGDATABASE", database));
    }
    Ok(environment)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lookup<'a>(environment: &'a [(&'static str, String)], name: &str) -> Option<&'a str> {
        environment
            .iter()
            .find(|(key, _)| *key == name)
            .map(|(_, value)| value.as_str())
    }

    #[test]
    fn a_full_url_maps_to_environment_variables_with_decoding() {
        let url = "postgresql://us%40er:p%40ss%2Fw%3Aord%25@db.example:5433/my%20db\
                   ?sslmode=verify-full&sslrootcert=%2Fetc%2Fca.pem&application_name=x&connect_timeout=7";
        let environment = libpq_environment(url).unwrap();
        assert_eq!(lookup(&environment, "PGUSER"), Some("us@er"));
        assert_eq!(lookup(&environment, "PGPASSWORD"), Some("p@ss/w:ord%"));
        assert_eq!(lookup(&environment, "PGHOST"), Some("db.example"));
        assert_eq!(lookup(&environment, "PGPORT"), Some("5433"));
        assert_eq!(lookup(&environment, "PGDATABASE"), Some("my db"));
        assert_eq!(lookup(&environment, "PGSSLMODE"), Some("verify-full"));
        assert_eq!(lookup(&environment, "PGSSLROOTCERT"), Some("/etc/ca.pem"));
        assert_eq!(lookup(&environment, "PGAPPNAME"), Some("x"));
        assert_eq!(lookup(&environment, "PGCONNECT_TIMEOUT"), Some("7"));
    }

    #[test]
    fn ipv6_hosts_and_missing_parts_are_handled() {
        let environment = libpq_environment("postgres://u@[2001:db8::1]:5432/d").unwrap();
        assert_eq!(lookup(&environment, "PGHOST"), Some("2001:db8::1"));
        assert_eq!(lookup(&environment, "PGPORT"), Some("5432"));
        assert_eq!(lookup(&environment, "PGPASSWORD"), None);
        let environment = libpq_environment("postgres://[::1]/d").unwrap();
        assert_eq!(lookup(&environment, "PGHOST"), Some("::1"));
        assert_eq!(lookup(&environment, "PGPORT"), None);
        let environment = libpq_environment("postgres:///d").unwrap();
        assert_eq!(environment, vec![("PGDATABASE", "d".to_owned())]);
    }

    #[test]
    fn a_socket_directory_comes_from_the_host_parameter() {
        let url = "postgres://u:p@/d?host=%2Fvar%2Frun%2Fpostgresql";
        let environment = libpq_environment(url).unwrap();
        assert_eq!(lookup(&environment, "PGHOST"), Some("/var/run/postgresql"));
        assert_eq!(lookup(&environment, "PGUSER"), Some("u"));
    }

    #[test]
    fn unusable_urls_are_refused_without_echoing_any_part() {
        const MARKER: &str = "zz-secret-marker";
        let cases = [
            format!("postgres://u:{MARKER}@h/d?options={MARKER}"),
            format!("postgres://u:{MARKER}@h/d?sslmode={MARKER}"),
            format!("postgres://u:{MARKER}@h:99999/d"),
            format!("postgres://u:{MARKER}@h:0/d"),
            format!("postgres://u:{MARKER}@h/d?host={MARKER}"),
            format!("postgres://u:{MARKER}@h/d?sslmode=require&sslmode=require"),
            format!("postgres://u:{MARKER}%zz@h/d"),
            format!("postgres://u:{MARKER}@h/d/e"),
            format!("postgres://u:{MARKER}@h/d?connect_timeout={MARKER}"),
            format!("postgres://u:{MARKER}@h/d#{MARKER}"),
            format!("mysql://u:{MARKER}@h/d"),
            format!("postgres://u:{MARKER}@[::1/d"),
        ];
        for url in cases {
            let error = libpq_environment(&url).expect_err(&url);
            let text = format!("{error} {error:?}");
            assert!(!text.contains(MARKER), "{text}");
            assert!(!text.contains("postgres://"), "{text}");
        }
    }
}
