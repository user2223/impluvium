// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! The server information, the settings and the first-run setup.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// `GET /api/v1`: what an anonymous client may know about the server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ServerInfo {
    /// The product name.
    pub app_name: String,
    /// The version of the server.
    pub version: String,
    /// The source revision, written with a leading `#`.
    pub git_ref: String,
    /// The external address of the server, as configured.
    pub external: String,
    /// The name the administrator gave the server.
    pub server_name: String,
    /// The object id of the server logo; `null` when no logo is set.
    pub m_logo_object_id: Option<String>,
}

/// `GET /api/v1/settings`: the settings every signed-in caller may read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ServerSettings {
    /// Settings that concern the whole server.
    pub general_settings: GeneralSettings,
    /// Settings of the store pages.
    pub store: StoreSettings,
}

/// The general settings of the server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct GeneralSettings {
    /// The name of the server.
    pub server_name: String,
    /// The object id of the server logo; `null` when no logo is set.
    pub m_logo_object_id: Option<String>,
}

/// The settings of the store pages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct StoreSettings {
    /// Whether game panels show the text decoration.
    pub show_game_panel_text_decoration: bool,
}
