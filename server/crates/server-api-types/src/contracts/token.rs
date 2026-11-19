// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! API tokens of accounts and of the server, and the rights a token can carry.
//!
//! The same shapes serve the routes under `/api/v1/user/token` and `/api/v1/admin/token`; only
//! the rights a token may be given differ.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::time::Timestamp;

/// What a token acts for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
pub enum TokenMode {
    /// A desktop client.
    Client,
    /// The server itself, created by an administrator.
    System,
    /// An account.
    User,
}

/// A token as it is listed. The token value is shown only once, when the token is created.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ApiToken {
    /// The rights the token carries.
    pub acls: Vec<String>,
    /// The client the token belongs to; `null` for tokens that are not a client's.
    pub client_id: Option<String>,
    /// When the token runs out; `null` when it never does.
    pub expires_at: Option<Timestamp>,
    /// The id of the token. It names the token for removal and is not the token value.
    pub id: String,
    /// What the token acts for.
    pub mode: TokenMode,
    /// The name the owner gave the token.
    pub name: String,
    /// The account the token belongs to; `null` for system tokens.
    pub user_id: Option<String>,
}

/// `GET /api/v1/user/token` and `GET /api/v1/admin/token`: the tokens; the body is the bare array.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ApiTokenList(pub Vec<ApiToken>);

/// A token that was just created, with its value. The value cannot be read again.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct NewApiToken {
    /// The rights the token carries.
    pub acls: Vec<String>,
    /// The client the token belongs to; `null` for tokens that are not a client's.
    pub client_id: Option<String>,
    /// When the token runs out; `null` when it never does.
    pub expires_at: Option<Timestamp>,
    /// The id of the token.
    pub id: String,
    /// What the token acts for.
    pub mode: TokenMode,
    /// The name the owner gave the token.
    pub name: String,
    /// The token value, sent as `Authorization: Bearer`.
    pub token: String,
    /// The account the token belongs to; `null` for system tokens.
    pub user_id: Option<String>,
}

/// `POST /api/v1/user/token` and `POST /api/v1/admin/token`: creates a token. The answer is a
/// [`NewApiToken`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct TokenCreate {
    /// The rights to give the token. At least one, each a name that the matching list of rights
    /// knows.
    pub acls: Vec<String>,
    /// When the token runs out, as an ISO 8601 date and time. Any UTC offset is accepted (the
    /// web interface sends its local one); absent means it never runs out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub expiry: Option<String>,
    /// The name of the token.
    pub name: String,
}

/// `GET /api/v1/user/token/acls` and `GET /api/v1/admin/token/acls`: every right a token can be
/// given, each with a description. The body is a map from the right to its description.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(type = "Record<string, string>")]
pub struct AclDescriptions(pub BTreeMap<String, String>);

/// `GET /api/v1/token`: the rights of the credential of the caller, or `null` when the caller
/// has no valid credential.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct GrantedAcls(pub Option<Vec<String>>);

// The value of a token is a password.
redacted_debug!(NewApiToken, shown: [acls, client_id, expires_at, id, mode, name, user_id], hidden: [token]);
