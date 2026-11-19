// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Access policies of server routes.
//!
//! A policy is either an explicitly justified public access or a requirement
//! on the caller: authenticated, rights, kind of session, multi-factor level
//! and, optionally, a recent login. Policies are plain values that carry no
//! HTTP types. Rights granted by roles and by tokens go through the same
//! check; this module only describes what a route demands.

use std::error::Error;
use std::fmt;

use crate::principal::{Principal, SessionClass, SessionStage};

/// A named right such as `emoji:read`: lowercase `scope:action`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Right(&'static str);

impl Right {
    /// Validates the name: two non-empty words of lowercase letters, digits
    /// and underscores (starting with a letter) joined by one colon.
    pub fn new(name: &'static str) -> Result<Self, PolicyError> {
        match name.split_once(':') {
            Some((scope, action)) if is_word(scope) && is_word(action) => Ok(Self(name)),
            _ => Err(PolicyError::InvalidRight(name)),
        }
    }

    /// The right as written in policies and inventories.
    pub fn as_str(self) -> &'static str {
        self.0
    }
}

fn is_word(part: &str) -> bool {
    let mut characters = part.chars();
    matches!(characters.next(), Some('a'..='z'))
        && characters.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// Which kind of credential may satisfy a policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionKind {
    /// An interactive session or an API token.
    Any,
    /// A browser session only. Store sessions and tokens do not qualify.
    Interactive,
    /// An API token only.
    Token,
}

impl SessionKind {
    /// Stable name for inventories.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Any => "any",
            Self::Interactive => "interactive",
            Self::Token => "token",
        }
    }
}

/// Required multi-factor level of the session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MfaLevel {
    /// No second factor needed: the sign-in must be finished, which for an
    /// account with a second factor means it was completed.
    NotRequired,
    /// The second factor must be completed in a browser session. Store
    /// sessions never satisfy this level.
    Completed,
    /// A step of the second-factor sign-in itself: only a session that still
    /// has to complete its second factor passes. Every other level rejects
    /// such a session.
    Pending,
}

impl MfaLevel {
    /// Stable name for inventories.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotRequired => "not_required",
            Self::Completed => "completed",
            Self::Pending => "pending",
        }
    }
}

/// What an authenticated caller must satisfy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Requirement {
    rights: Vec<Right>,
    session: SessionKind,
    mfa: MfaLevel,
    fresh_login_secs: Option<u32>,
}

impl Requirement {
    /// Any authenticated caller. This demands no right at all; a requirement
    /// that demands rights has to list them through [`Requirement::require_rights`].
    pub fn authenticated() -> Self {
        Self {
            rights: Vec::new(),
            session: SessionKind::Any,
            mfa: MfaLevel::NotRequired,
            fresh_login_secs: None,
        }
    }

    /// Additionally requires every listed right. An empty list is rejected:
    /// it would read as a rights check but grant nothing to check against.
    pub fn require_rights(mut self, rights: &[Right]) -> Result<Self, PolicyError> {
        if rights.is_empty() {
            return Err(PolicyError::EmptyRights);
        }
        self.rights.extend_from_slice(rights);
        self.rights.sort();
        self.rights.dedup();
        Ok(self)
    }

    /// Restricts the kind of session.
    pub fn require_session(mut self, session: SessionKind) -> Self {
        self.session = session;
        self
    }

    /// Requires the given multi-factor level.
    pub fn require_mfa(mut self, mfa: MfaLevel) -> Self {
        self.mfa = mfa;
        self
    }

    /// Requires an interactive login no older than `seconds`.
    pub fn require_fresh_login_within(mut self, seconds: u32) -> Result<Self, PolicyError> {
        if seconds == 0 {
            return Err(PolicyError::ZeroFreshness);
        }
        self.fresh_login_secs = Some(seconds);
        Ok(self)
    }

    /// Required rights, sorted and without duplicates.
    pub fn rights(&self) -> &[Right] {
        &self.rights
    }

    /// Required kind of session.
    pub fn session(&self) -> SessionKind {
        self.session
    }

    /// Required multi-factor level.
    pub fn mfa(&self) -> MfaLevel {
        self.mfa
    }

    /// Maximum age of the login in seconds, if one is required.
    pub fn fresh_login_within_secs(&self) -> Option<u32> {
        self.fresh_login_secs
    }

    /// Whether the principal satisfies every part of the requirement.
    ///
    /// Anonymous callers never do. A store session passes only a requirement
    /// that names at least one right, and never an interactive-only one. A token or device without any effective
    /// right does not either, even if the requirement names no right. Tokens
    /// and devices satisfy the second-factor level `NotRequired` only, never
    /// `Completed`, `Pending` or a login-freshness requirement.
    pub fn is_satisfied_by(&self, principal: &Principal) -> bool {
        let rights = principal.rights();
        let (kind_ok, mfa_ok, fresh_ok) = match principal {
            Principal::Anonymous => return false,
            Principal::Session(session) => {
                let finished = session.stage() == SessionStage::Authenticated;
                let browser = session.class() == SessionClass::Browser;
                // A store session only passes where specific rights are asked.
                let store_ok = browser || !self.rights.is_empty();
                (
                    match self.session {
                        SessionKind::Any => store_ok,
                        SessionKind::Interactive => browser,
                        SessionKind::Token => false,
                    },
                    match self.mfa {
                        MfaLevel::Pending => !finished,
                        MfaLevel::NotRequired => finished,
                        MfaLevel::Completed => finished && browser,
                    },
                    self.fresh_login_secs.is_none_or(|limit| {
                        finished && browser && session.login_age_secs() <= limit
                    }),
                )
            }
            Principal::Token(_) | Principal::Device(_) => (
                match principal {
                    Principal::Token(_) => {
                        matches!(self.session, SessionKind::Any | SessionKind::Token)
                    }
                    _ => self.session == SessionKind::Any,
                },
                self.mfa == MfaLevel::NotRequired,
                self.fresh_login_secs.is_none() && !rights.is_empty(),
            ),
        };
        kind_ok && mfa_ok && fresh_ok && rights.contains_all(&self.rights)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Kind {
    Public { reason: &'static str },
    Protected(Requirement),
}

/// The access policy of one route.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Policy(Kind);

/// Outcome of evaluating a policy for a caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// The request may proceed.
    Allow,
    /// The caller has to authenticate first.
    Unauthenticated,
    /// The caller is known but does not satisfy the policy.
    Forbidden,
}

impl Policy {
    /// Public access without authentication. The reason is mandatory and
    /// appears in the route inventory.
    pub fn public(reason: &'static str) -> Result<Self, PolicyError> {
        if reason.trim().is_empty() {
            return Err(PolicyError::PublicWithoutReason);
        }
        Ok(Self(Kind::Public { reason }))
    }

    /// Access for callers that satisfy the requirement.
    pub fn protected(requirement: Requirement) -> Self {
        Self(Kind::Protected(requirement))
    }

    /// The justification of a public policy.
    pub fn public_reason(&self) -> Option<&'static str> {
        match &self.0 {
            Kind::Public { reason } => Some(reason),
            Kind::Protected(_) => None,
        }
    }

    /// The requirement of a protected policy.
    pub fn requirement(&self) -> Option<&Requirement> {
        match &self.0 {
            Kind::Public { .. } => None,
            Kind::Protected(requirement) => Some(requirement),
        }
    }

    /// Decision for a caller without credentials.
    pub fn decide_anonymous(&self) -> Decision {
        self.decide(&Principal::Anonymous)
    }

    /// Decision for the given caller. Public policies allow everyone;
    /// protected ones ask the anonymous caller to authenticate and reject a
    /// known caller that falls short.
    pub fn decide(&self, principal: &Principal) -> Decision {
        match &self.0 {
            Kind::Public { .. } => Decision::Allow,
            Kind::Protected(_) if principal.is_anonymous() => Decision::Unauthenticated,
            Kind::Protected(requirement) => {
                if requirement.is_satisfied_by(principal) {
                    Decision::Allow
                } else {
                    Decision::Forbidden
                }
            }
        }
    }
}

/// A policy or requirement that is not well formed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyError {
    /// A public policy without a justification.
    PublicWithoutReason,
    /// A rights requirement with an empty list.
    EmptyRights,
    /// A right name that is not `scope:action` in lowercase.
    InvalidRight(&'static str),
    /// A login freshness of zero seconds.
    ZeroFreshness,
}

impl fmt::Display for PolicyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PublicWithoutReason => formatter.write_str("a public policy needs a reason"),
            Self::EmptyRights => {
                formatter.write_str("a rights requirement needs at least one right")
            }
            Self::InvalidRight(name) => write!(formatter, "invalid right name: {name}"),
            Self::ZeroFreshness => {
                formatter.write_str("login freshness must be at least one second")
            }
        }
    }
}

impl Error for PolicyError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_policy_needs_a_reason() {
        assert_eq!(Policy::public(""), Err(PolicyError::PublicWithoutReason));
        assert_eq!(
            Policy::public("  \t"),
            Err(PolicyError::PublicWithoutReason)
        );
        let policy = Policy::public("health probe").unwrap();
        assert_eq!(policy.public_reason(), Some("health probe"));
        assert!(policy.requirement().is_none());
    }

    #[test]
    fn right_names_are_validated() {
        assert_eq!(Right::new("emoji:read").unwrap().as_str(), "emoji:read");
        for bad in [
            "",
            "emoji",
            "emoji:",
            ":read",
            "Emoji:read",
            "emoji:read:all",
            "1a:read",
            "emoji:re ad",
        ] {
            assert_eq!(
                Right::new(bad),
                Err(PolicyError::InvalidRight(bad)),
                "{bad}"
            );
        }
    }

    #[test]
    fn empty_rights_are_rejected_and_do_not_grant_anything() {
        assert_eq!(
            Requirement::authenticated().require_rights(&[]),
            Err(PolicyError::EmptyRights)
        );
        assert!(Requirement::authenticated().rights().is_empty());
    }

    #[test]
    fn rights_are_sorted_and_deduplicated() {
        let read = Right::new("emoji:read").unwrap();
        let write = Right::new("emoji:write").unwrap();
        let requirement = Requirement::authenticated()
            .require_rights(&[write, read, write])
            .unwrap();
        assert_eq!(requirement.rights(), &[read, write]);
    }

    #[test]
    fn requirement_records_session_mfa_and_freshness() {
        let requirement = Requirement::authenticated()
            .require_session(SessionKind::Interactive)
            .require_mfa(MfaLevel::Completed)
            .require_fresh_login_within(300)
            .unwrap();
        assert_eq!(requirement.session(), SessionKind::Interactive);
        assert_eq!(requirement.mfa(), MfaLevel::Completed);
        assert_eq!(requirement.fresh_login_within_secs(), Some(300));
        assert_eq!(
            Requirement::authenticated().require_fresh_login_within(0),
            Err(PolicyError::ZeroFreshness)
        );
    }

    #[test]
    fn anonymous_callers_pass_public_policies_only() {
        let public = Policy::public("sign-in entry point").unwrap();
        let protected = Policy::protected(Requirement::authenticated());
        assert_eq!(public.decide_anonymous(), Decision::Allow);
        assert_eq!(protected.decide_anonymous(), Decision::Unauthenticated);
    }
}
