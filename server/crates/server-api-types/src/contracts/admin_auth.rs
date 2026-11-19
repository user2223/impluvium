// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Administration of the sign-in methods and of invitations.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::time::Timestamp;

/// `GET /api/v1/admin/auth`: which sign-in methods are on.
///
/// The keys are written exactly as the sign-in methods are named (`Simple`, `OpenID`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct AdminAuthInfo {
    /// Whether signing in with a name and a password is on.
    #[serde(rename = "Simple")]
    pub simple: bool,
    /// The settings of single sign-on; absent when single sign-on is off.
    #[serde(rename = "OpenID", default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub open_id: Option<OidcConfiguration>,
}

/// The single sign-on settings an administrator can see.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct OidcConfiguration {
    /// The authorisation endpoint of the identity provider.
    pub authorization_url: String,
    /// The scopes the provider supports, joined by `, `.
    pub scopes: String,
    /// The provider group that makes an account an administrator; absent when none is set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub admin_group: Option<String>,
    /// The claim that names the account.
    pub username_claim: UsernameClaim,
    /// The external address that identity providers are configured with.
    pub external_url: String,
}

/// The claims of an identity provider that can name an account.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum UsernameClaim {
    /// The email address.
    Email,
    /// The groups.
    Groups,
    /// The full name.
    Name,
    /// The picture address.
    Picture,
    /// The preferred user name.
    PreferredUsername,
    /// The subject identifier.
    Sub,
}

/// An invitation to register, as an administrator sees it.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Invitation {
    /// The id of the invitation; it is the secret of the invite link.
    pub id: String,
    /// Whether the new account becomes an administrator.
    pub is_admin: bool,
    /// The user name the invitation fixes; `null` when the invitee chooses.
    pub username: Option<String>,
    /// The email address the invitation fixes; `null` when the invitee chooses.
    pub email: Option<String>,
    /// When the invitation runs out.
    pub expires: Timestamp,
    /// The address of the registration page for this invitation.
    pub invite_url: String,
}

/// `GET /api/v1/admin/auth/invitation`: all invitations; the body is the bare array.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct InvitationList(pub Vec<Invitation>);

/// `POST /api/v1/admin/auth/invitation`: creates an invitation. The answer is an [`Invitation`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct InvitationCreate {
    /// Fixes the user name; absent or `null` leaves it to the invitee.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub username: Option<String>,
    /// Fixes the email address; absent or `null` leaves it to the invitee.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub email: Option<String>,
    /// When the invitation runs out, as an RFC 3339 date and time. Any UTC offset is accepted
    /// here (the web interface sends its local one); the answer always has the canonical form.
    pub expires: String,
    /// Whether the new account becomes an administrator; absent or `null` means `false`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub is_admin: Option<bool>,
}

/// `DELETE /api/v1/admin/auth/invitation`: the invitation to remove. The answer is `{}`.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct InvitationDelete {
    /// The id of the invitation.
    pub id: String,
}

// The invite link carries the id, so it is hidden too.
redacted_debug!(Invitation, shown: [is_admin, username, email, expires], hidden: [id, invite_url]);

redacted_debug!(InvitationDelete, shown: [], hidden: [id]);
