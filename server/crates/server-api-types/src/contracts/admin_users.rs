// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Administration of accounts.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::auth::AuthProvider;

/// A sign-in method linked to an account, as the account list shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct LinkedAuthMethod {
    /// The method.
    pub mec: AuthProvider,
}

/// An account in the list of an administrator.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AdminUser {
    /// Whether the account is an administrator.
    pub admin: bool,
    /// The sign-in methods linked to the account.
    pub auth_mecs: Vec<LinkedAuthMethod>,
    /// The name to show.
    pub display_name: String,
    /// The email address.
    pub email: String,
    /// Whether the account may sign in.
    pub enabled: bool,
    /// The id of the account.
    pub id: String,
    /// The object id of the profile picture.
    pub profile_picture_object_id: String,
    /// The user name.
    pub username: String,
}

/// `GET /api/v1/admin/users`: all accounts except the system account; the body is the bare array.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct AdminUserList(pub Vec<AdminUser>);

/// `DELETE /api/v1/admin/users/:id`: the answer to a removed account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct UserDeleted {
    /// Always `true`.
    pub success: bool,
}
