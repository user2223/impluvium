// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Who is calling: the principal of a request.
//!
//! A principal is the result of resolving a credential, or the anonymous
//! caller when there is none. It never derives anything from client-supplied
//! hints such as the user agent: the kind of a session is set by the server
//! when it issues the session.

use crate::permissions::{RightSet, role_rights, store_session_rights};

/// The role of a user.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// An ordinary account.
    User,
    /// An administrator.
    Admin,
}

/// How far the sign-in of a session has progressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionStage {
    /// The password was accepted but the second factor is still missing.
    MfaPending,
    /// The sign-in is complete.
    Authenticated,
}

/// The kind of session, decided by the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionClass {
    /// A normal browser session.
    Browser,
    /// The restricted session of the store window in the desktop client. It
    /// holds store, library and game page rights only, never counts as a
    /// completed second factor and never counts as a recent login.
    Store,
}

/// An interactive session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    role: Role,
    stage: SessionStage,
    class: SessionClass,
    login_age_secs: u32,
}

impl Session {
    /// A session of a user with the given role. `login_age_secs` is the time
    /// since the last interactive sign-in.
    pub fn new(role: Role, stage: SessionStage, class: SessionClass, login_age_secs: u32) -> Self {
        Self {
            role,
            stage,
            class,
            login_age_secs,
        }
    }

    /// Role of the user.
    pub fn role(&self) -> Role {
        self.role
    }

    /// Progress of the sign-in.
    pub fn stage(&self) -> SessionStage {
        self.stage
    }

    /// Kind of session.
    pub fn class(&self) -> SessionClass {
        self.class
    }

    /// Seconds since the last interactive sign-in.
    pub fn login_age_secs(&self) -> u32 {
        self.login_age_secs
    }

    fn rights(&self) -> RightSet {
        if self.stage == SessionStage::MfaPending {
            return RightSet::empty();
        }
        let role = role_rights(self.role);
        match self.class {
            SessionClass::Browser => role,
            SessionClass::Store => role.intersection(&store_session_rights()),
        }
    }
}

/// The kind of an API token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    /// A personal access token.
    Personal,
    /// A service token.
    Service,
}

/// An API token. Its rights are fixed when the principal is built: the
/// intersection of what the token was granted and what its owner holds now.
/// The kind of the token adds nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    kind: TokenKind,
    rights: RightSet,
}

impl Token {
    /// A token with the rights it was granted, limited to the current rights
    /// of its owner.
    pub fn new(kind: TokenKind, granted: &RightSet, owner_rights: &RightSet) -> Self {
        Self {
            kind,
            rights: granted.intersection(owner_rights),
        }
    }

    /// Kind of token.
    pub fn kind(&self) -> TokenKind {
        self.kind
    }
}

/// A paired device acting on behalf of its owner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    rights: RightSet,
}

impl Device {
    /// A device with the rights it was granted, limited to the current rights
    /// of its owner.
    pub fn new(granted: &RightSet, owner_rights: &RightSet) -> Self {
        Self {
            rights: granted.intersection(owner_rights),
        }
    }
}

/// The caller of a request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Principal {
    /// No credential, or none that resolved.
    Anonymous,
    /// An interactive session.
    Session(Session),
    /// An API token.
    Token(Token),
    /// A paired device.
    Device(Device),
}

impl Principal {
    /// The rights the caller holds right now.
    pub fn rights(&self) -> RightSet {
        match self {
            Self::Anonymous => RightSet::empty(),
            Self::Session(session) => session.rights(),
            Self::Token(token) => token.rights.clone(),
            Self::Device(device) => device.rights.clone(),
        }
    }

    /// Whether the caller presented no usable credential.
    pub fn is_anonymous(&self) -> bool {
        matches!(self, Self::Anonymous)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy::{Decision, MfaLevel, Policy, Requirement, Right, SessionKind};

    fn right(name: &'static str) -> Right {
        Right::new(name).unwrap()
    }

    fn session(role: Role, stage: SessionStage, class: SessionClass, age: u32) -> Principal {
        Principal::Session(Session::new(role, stage, class, age))
    }

    fn browser(role: Role) -> Principal {
        session(role, SessionStage::Authenticated, SessionClass::Browser, 10)
    }

    fn token(kind: TokenKind, granted: &[Right], owner: Role) -> Principal {
        Principal::Token(Token::new(
            kind,
            &RightSet::from_rights(granted),
            &role_rights(owner),
        ))
    }

    fn needs(rights: &[Right]) -> Policy {
        Policy::protected(Requirement::authenticated().require_rights(rights).unwrap())
    }

    fn decide(policy: &Policy, principal: &Principal) -> Decision {
        policy.decide(principal)
    }

    #[test]
    fn matrix_over_principals_for_a_rights_policy() {
        let read = needs(&[right("emoji:read")]);
        let admin = needs(&[right("admin:manage")]);
        let cases: Vec<(&str, Principal, Decision, Decision)> = vec![
            (
                "anonymous",
                Principal::Anonymous,
                Decision::Unauthenticated,
                Decision::Unauthenticated,
            ),
            (
                "mfa pending",
                session(
                    Role::Admin,
                    SessionStage::MfaPending,
                    SessionClass::Browser,
                    1,
                ),
                Decision::Forbidden,
                Decision::Forbidden,
            ),
            (
                "user",
                browser(Role::User),
                Decision::Allow,
                Decision::Forbidden,
            ),
            (
                "admin",
                browser(Role::Admin),
                Decision::Allow,
                Decision::Allow,
            ),
            (
                "store session of an admin",
                session(
                    Role::Admin,
                    SessionStage::Authenticated,
                    SessionClass::Store,
                    1,
                ),
                Decision::Allow,
                Decision::Forbidden,
            ),
            (
                "narrow token",
                token(TokenKind::Personal, &[right("emoji:read")], Role::Admin),
                Decision::Allow,
                Decision::Forbidden,
            ),
            (
                "empty token",
                token(TokenKind::Personal, &[], Role::Admin),
                Decision::Forbidden,
                Decision::Forbidden,
            ),
            (
                "device",
                Principal::Device(Device::new(
                    &RightSet::from_rights(&[right("emoji:read")]),
                    &role_rights(Role::User),
                )),
                Decision::Allow,
                Decision::Forbidden,
            ),
        ];
        for (name, principal, on_read, on_admin) in cases {
            assert_eq!(decide(&read, &principal), on_read, "{name}: read");
            assert_eq!(decide(&admin, &principal), on_admin, "{name}: admin");
        }
    }

    #[test]
    fn empty_right_set_grants_no_rights() {
        let token = token(TokenKind::Service, &[], Role::Admin);
        assert!(token.rights().is_empty());
        assert!(Principal::Anonymous.rights().is_empty());
        // A token that holds nothing does not even pass a policy that asks
        // for no right.
        assert_eq!(
            decide(&Policy::protected(Requirement::authenticated()), &token),
            Decision::Forbidden
        );
    }

    #[test]
    fn token_rights_are_limited_to_the_current_rights_of_the_owner() {
        let granted = [right("emoji:read"), right("admin:manage")];
        let of_user = token(TokenKind::Personal, &granted, Role::User);
        assert!(of_user.rights().contains(right("emoji:read")));
        assert!(!of_user.rights().contains(right("admin:manage")));
        let of_admin = token(TokenKind::Personal, &granted, Role::Admin);
        assert!(of_admin.rights().contains(right("admin:manage")));
    }

    #[test]
    fn the_kind_of_a_token_derives_no_administration_right() {
        for kind in [TokenKind::Personal, TokenKind::Service] {
            let principal = token(kind, &[right("emoji:read")], Role::Admin);
            assert_eq!(
                decide(&needs(&[right("admin:manage")]), &principal),
                Decision::Forbidden
            );
        }
    }

    #[test]
    fn roles_and_tokens_are_checked_the_same_way() {
        let policy = needs(&[right("emoji:read")]);
        let by_role = browser(Role::User);
        let all = role_rights(Role::User).iter().collect::<Vec<_>>();
        let by_token = token(TokenKind::Personal, &all, Role::User);
        assert_eq!(by_role.rights(), by_token.rights());
        assert_eq!(decide(&policy, &by_role), decide(&policy, &by_token));
    }

    #[test]
    fn session_kind_restricts_the_credential() {
        let interactive = Policy::protected(
            Requirement::authenticated().require_session(SessionKind::Interactive),
        );
        let tokens =
            Policy::protected(Requirement::authenticated().require_session(SessionKind::Token));
        let narrow = token(TokenKind::Personal, &[right("emoji:read")], Role::User);
        assert_eq!(decide(&interactive, &browser(Role::User)), Decision::Allow);
        assert_eq!(decide(&interactive, &narrow), Decision::Forbidden);
        assert_eq!(decide(&tokens, &narrow), Decision::Allow);
        assert_eq!(decide(&tokens, &browser(Role::User)), Decision::Forbidden);
    }

    #[test]
    fn a_pending_second_factor_reaches_only_the_second_factor_step() {
        let pending = session(
            Role::User,
            SessionStage::MfaPending,
            SessionClass::Browser,
            1,
        );
        let step = Policy::protected(Requirement::authenticated().require_mfa(MfaLevel::Pending));
        let ordinary = Policy::protected(Requirement::authenticated());
        let completed =
            Policy::protected(Requirement::authenticated().require_mfa(MfaLevel::Completed));
        assert_eq!(decide(&step, &pending), Decision::Allow);
        assert_eq!(decide(&ordinary, &pending), Decision::Forbidden);
        assert_eq!(decide(&completed, &pending), Decision::Forbidden);
        // The step is not meant for sessions that are done.
        assert_eq!(decide(&step, &browser(Role::User)), Decision::Forbidden);
    }

    #[test]
    fn completed_second_factor_and_fresh_login_need_a_browser_session() {
        let completed =
            Policy::protected(Requirement::authenticated().require_mfa(MfaLevel::Completed));
        let fresh = Policy::protected(
            Requirement::authenticated()
                .require_fresh_login_within(300)
                .unwrap(),
        );
        let store = session(
            Role::User,
            SessionStage::Authenticated,
            SessionClass::Store,
            1,
        );
        let stale = session(
            Role::User,
            SessionStage::Authenticated,
            SessionClass::Browser,
            301,
        );
        let narrow = token(TokenKind::Personal, &[right("emoji:read")], Role::User);
        assert_eq!(decide(&completed, &browser(Role::User)), Decision::Allow);
        assert_eq!(decide(&completed, &store), Decision::Forbidden);
        assert_eq!(decide(&fresh, &browser(Role::User)), Decision::Allow);
        assert_eq!(decide(&fresh, &stale), Decision::Forbidden);
        assert_eq!(decide(&fresh, &store), Decision::Forbidden);
        assert_eq!(decide(&fresh, &narrow), Decision::Forbidden);
    }

    #[test]
    fn public_policies_allow_every_principal() {
        let policy = Policy::public("test").unwrap();
        assert_eq!(decide(&policy, &Principal::Anonymous), Decision::Allow);
        assert_eq!(decide(&policy, &browser(Role::User)), Decision::Allow);
    }

    #[test]
    fn tokens_and_devices_never_satisfy_a_completed_second_factor() {
        let completed =
            Policy::protected(Requirement::authenticated().require_mfa(MfaLevel::Completed));
        let device = Principal::Device(Device::new(
            &RightSet::from_rights(&[right("emoji:read")]),
            &role_rights(Role::User),
        ));
        let narrow = token(TokenKind::Personal, &[right("emoji:read")], Role::User);
        assert_eq!(decide(&completed, &narrow), Decision::Forbidden);
        assert_eq!(decide(&completed, &device), Decision::Forbidden);
        let not_required = needs(&[right("emoji:read")]);
        assert_eq!(decide(&not_required, &narrow), Decision::Allow);
        assert_eq!(decide(&not_required, &device), Decision::Allow);
    }

    #[test]
    fn store_sessions_pass_only_requirements_that_name_a_right() {
        let store = session(
            Role::User,
            SessionStage::Authenticated,
            SessionClass::Store,
            1,
        );
        let no_right = Policy::protected(Requirement::authenticated());
        let any_session =
            Policy::protected(Requirement::authenticated().require_session(SessionKind::Any));
        let interactive = Policy::protected(
            Requirement::authenticated()
                .require_session(SessionKind::Interactive)
                .require_rights(&[right("emoji:read")])
                .unwrap(),
        );
        assert_eq!(decide(&no_right, &store), Decision::Forbidden);
        assert_eq!(decide(&any_session, &store), Decision::Forbidden);
        assert_eq!(
            decide(&needs(&[right("emoji:read")]), &store),
            Decision::Allow
        );
        // Interactive means a browser session: a store session does not qualify.
        assert_eq!(decide(&interactive, &store), Decision::Forbidden);
        assert_eq!(decide(&interactive, &browser(Role::User)), Decision::Allow);
    }
}
