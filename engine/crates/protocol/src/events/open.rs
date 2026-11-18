// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: 0BSD

//! Opening checks in events.md §1 order. Header occurrences are preserved by
//! the HTTP adapter. Credential validity is an injected lookup result, never
//! inferred from a cookie or token's spelling here.

use super::MAX_CLIENT_IDENTIFIERS;
use crate::{
    api::errors::{Code, WireError},
    decode::{Diagnostic, Reason, error},
    proto::impluvium::v2::ErrorReason,
};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum Form {
    Bearer,
    Cookie,
}
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct Key {
    pub user: String,
    pub form: Form,
    /// Device id for Bearer, session id for Cookie; never an access token.
    pub owner: String,
    pub client: String,
}

/// Non-schema rejection information and the corresponding error-body contract.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Rejection {
    pub status: u16,
    pub wire: WireError,
    pub diagnostic: Option<Diagnostic>,
}
fn reject(
    status: u16,
    code: Code,
    reason: Option<ErrorReason>,
    local: Option<Reason>,
) -> Rejection {
    Rejection {
        status,
        wire: WireError { code, reason },
        diagnostic: local.map(|r| error(r, "")),
    }
}
fn bad(reason: Reason) -> Rejection {
    reject(400, Code::invalid_argument, None, Some(reason))
}
fn unauthenticated(reason: Option<ErrorReason>) -> Rejection {
    reject(401, Code::unauthenticated, reason, None)
}
fn same_origin(value: &str, own: &str) -> bool {
    let (Ok(value), Ok(own)) = (url::Url::parse(value), url::Url::parse(own)) else {
        return false;
    };
    matches!(value.scheme(), "http" | "https")
        && value.username().is_empty()
        && value.password().is_none()
        && value.path() == "/"
        && value.query().is_none()
        && value.fragment().is_none()
        && value.origin() == own.origin()
}

/// Ordered observations from proofs.md §2.4. Adapters perform token lookup and
/// time comparisons; events never renew device activity.
#[derive(Clone, Copy, Debug)]
pub struct BearerState {
    pub known: bool,
    pub expired: bool,
    pub device_revoked: bool,
    pub device_expired: bool,
}
impl BearerState {
    fn check(self) -> Result<(), Rejection> {
        if !self.known {
            return Err(unauthenticated(None));
        }
        let reason = if self.expired {
            Some(ErrorReason::TokenExpired)
        } else if self.device_revoked {
            Some(ErrorReason::DeviceRevoked)
        } else if self.device_expired {
            Some(ErrorReason::DeviceExpired)
        } else {
            None
        };
        reason.map_or(Ok(()), |r| Err(unauthenticated(Some(r))))
    }
}

pub struct Request<'a> {
    pub server_starting: bool,
    pub method: &'a str,
    /// The raw query without '?', parsed as WHATWG form data after authentication.
    pub query: &'a str,
    pub headers: &'a [(&'a str, &'a str)],
    pub session_cookie_name: &'a str,
    pub server_origin: &'a str,
    pub user: &'a str,
    pub bearer_owner: &'a str,
    pub cookie_owner: &'a str,
    pub bearer: BearerState,
    pub cookie_valid: bool,
    pub topic_authorized: bool,
}
#[derive(Clone, Debug)]
pub struct Accepted {
    pub form: Form,
    pub key: Option<Key>,
    pub resume_point: Option<String>,
}
impl Accepted {
    pub const CONTENT_TYPE: &'static str = "text/event-stream";
    pub const CACHE_CONTROL: &'static str = "no-cache";
    pub const ACCEL_BUFFERING: &'static str = "no";
}

/// Checks through step 3 only. Step 4 and registration are performed atomically
/// by Table::open. A transport adapter bounds the entire request head first.
pub fn check(request: &Request<'_>) -> Result<Accepted, Rejection> {
    if request.server_starting {
        return Err(reject(
            503,
            Code::unavailable,
            Some(ErrorReason::ServerStarting),
            None,
        ));
    }
    if request.method != "GET" {
        return Err(reject(405, Code::unimplemented, None, None));
    }
    let headers = |name: &'static str| {
        request
            .headers
            .iter()
            .filter(move |(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| *v)
    };
    let mut authorization = headers("authorization");
    let auth = authorization.next();
    let cookie_count = headers("cookie")
        .flat_map(|v| v.split(';'))
        .filter(|part| {
            part.trim()
                .split_once('=')
                .is_some_and(|(name, _)| name == request.session_cookie_name)
        })
        .count();
    if authorization.next().is_some() || cookie_count > 1 || (auth.is_some() && cookie_count != 0) {
        return Err(unauthenticated(None));
    }
    let form = if let Some(auth) = auth {
        if !auth.split_once(' ').is_some_and(|(scheme, token)| {
            scheme.eq_ignore_ascii_case("Bearer") && !token.trim().is_empty()
        }) {
            return Err(unauthenticated(None));
        }
        request.bearer.check()?;
        Form::Bearer
    } else if cookie_count == 1 {
        let mut origins = headers("origin");
        if !request.cookie_valid
            || !origins
                .next()
                .is_some_and(|origin| same_origin(origin, request.server_origin))
            || origins.next().is_some()
        {
            return Err(unauthenticated(None));
        }
        Form::Cookie
    } else {
        return Err(unauthenticated(None));
    };
    let mut topics = None;
    let mut client = None;
    let mut point = None;
    for (name, value) in url::form_urlencoded::parse(request.query.as_bytes()) {
        let slot = match name.as_ref() {
            "topics" => &mut topics,
            "client" => &mut client,
            "last_event_id" => &mut point,
            _ => continue,
        };
        if slot.is_some() {
            return Err(bad(Reason::invalid_parameter));
        }
        *slot = Some(value);
    }
    let mut ids = headers("last-event-id");
    let header_id = ids.next();
    if ids.next().is_some() {
        return Err(bad(Reason::invalid_parameter));
    }
    // This revision defines only library; reserved topics are invalid, even
    // though the extensible parameter grammar permits up to eight topics.
    if topics.as_deref() != Some("library") {
        return Err(bad(Reason::invalid_topic));
    }
    if client.as_deref().is_some_and(|c| {
        c.is_empty()
            || c.len() > 64
            || !c
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    }) || (form == Form::Cookie && client.is_none())
    {
        return Err(bad(Reason::invalid_client_id));
    }
    if !request.topic_authorized {
        return Err(reject(403, Code::permission_denied, None, None));
    }
    let owner = match form {
        Form::Bearer => request.bearer_owner,
        Form::Cookie => request.cookie_owner,
    };
    Ok(Accepted {
        form,
        key: client.map(|client| Key {
            user: request.user.into(),
            form,
            owner: owner.into(),
            client: client.into_owned(),
        }),
        resume_point: point
            .map(|p| p.into_owned())
            .or_else(|| header_id.map(str::to_owned)),
    })
}

/// Opaque stream generation supplied by the caller. A generation must never be
/// reused while a delayed release for it could still arrive (including an ABA
/// remove/reinsert). It is unrelated to a client id or a token generation.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct Generation(pub u128);
#[derive(Clone, Debug, Default)]
pub struct Table {
    entries: BTreeMap<Key, Generation>,
}
#[derive(Clone, Debug)]
pub struct Opened {
    pub accepted: Accepted,
    /// Send Superseded and close this generation, including paused replay.
    pub superseded: Option<Generation>,
    pub created_identifier: bool,
}
impl Table {
    pub fn entries(&self) -> impl Iterator<Item = (&Key, Generation)> {
        self.entries.iter().map(|(k, g)| (k, *g))
    }
    pub fn count(&self, user: &str) -> usize {
        self.entries.keys().filter(|k| k.user == user).count()
    }
    pub fn open(
        &self,
        request: &Request<'_>,
        generation: Generation,
    ) -> (Self, Result<Opened, Rejection>) {
        let accepted = match check(request) {
            Ok(a) => a,
            Err(e) => return (self.clone(), Err(e)),
        };
        let superseded = accepted
            .key
            .as_ref()
            .and_then(|key| self.entries.get(key))
            .copied();
        let created_identifier = accepted.key.is_some() && superseded.is_none();
        if created_identifier && self.count(request.user) >= MAX_CLIENT_IDENTIFIERS {
            return (
                self.clone(),
                Err(reject(
                    429,
                    Code::resource_exhausted,
                    None,
                    Some(Reason::client_table_full),
                )),
            );
        }
        let mut next = self.clone();
        if let Some(key) = &accepted.key {
            next.entries.insert(key.clone(), generation);
        }
        (
            next,
            Ok(Opened {
                accepted,
                superseded,
                created_identifier,
            }),
        )
    }
    pub fn release(&self, key: &Key, generation: Generation) -> (Self, bool) {
        let mut next = self.clone();
        let removed = self.entries.get(key) == Some(&generation);
        if removed {
            next.entries.remove(key);
        }
        (next, removed)
    }
}
