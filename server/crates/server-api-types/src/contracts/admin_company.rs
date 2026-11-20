// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Company administration, including the flattened relationship ids in detail answers.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::admin_game::AdminUploadFile;
use super::catalog::{CatalogCompany, CatalogGame};
use super::object::ObjectReference;
use crate::enums::{CompanyAction, MetadataSource};

/// An opaque catalog path id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AdminCompanyParams {
    /// The nonempty record id.
    pub id: String,
}

impl AdminCompanyParams {
    /// Refuses an empty path id before accessing storage.
    pub fn is_valid(&self) -> bool {
        !self.id.is_empty()
    }
}

/// The bare company list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct AdminCompanyList(pub Vec<CatalogCompany>);

/// Creates a manual company.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AdminCompanyCreate {
    /// The name.
    pub name: String,
    /// The short description.
    pub description: String,
    /// The website.
    pub website: String,
}

/// Partial scalar update; id is ignored in favor of the path. All stored columns are nonnullable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AdminCompanyPatch {
    /// The provider.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "super::non_null_option"
    )]
    #[ts(optional)]
    pub metadata_source: Option<MetadataSource>,
    /// The provider id.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "super::non_null_option"
    )]
    #[ts(optional)]
    pub metadata_id: Option<String>,
    /// The original query.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "super::non_null_option"
    )]
    #[ts(optional)]
    pub metadata_original_query: Option<String>,
    /// The name.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "super::non_null_option"
    )]
    #[ts(optional)]
    pub m_name: Option<String>,
    /// The short description.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "super::non_null_option"
    )]
    #[ts(optional)]
    pub m_short_description: Option<String>,
    /// The Markdown description.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "super::non_null_option"
    )]
    #[ts(optional)]
    pub m_description: Option<String>,
    /// The logo reference.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "super::non_null_option"
    )]
    #[ts(optional)]
    pub m_logo_object_id: Option<ObjectReference>,
    /// The banner reference.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "super::non_null_option"
    )]
    #[ts(optional)]
    pub m_banner_object_id: Option<ObjectReference>,
    /// The website.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "super::non_null_option"
    )]
    #[ts(optional)]
    pub m_website: Option<String>,
}

/// The scalar company and flattened game ids.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AdminCompanyDetail {
    /// The scalar record, flattened.
    #[serde(flatten)]
    pub company: CatalogCompany,
    /// Published game ids.
    pub published: Vec<String>,
    /// Developed game ids.
    pub developed: Vec<String>,
}

/// Company detail and distinct related scalar games.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AdminCompanyResponse {
    /// The company with relationship ids.
    pub company: AdminCompanyDetail,
    /// Related games, distinct by id.
    pub games: Vec<CatalogGame>,
}

/// Connects at least one relationship.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AdminCompanyGameCreate {
    /// The game id.
    pub id: String,
    /// Whether published by the company.
    pub published: bool,
    /// Whether developed by the company.
    pub developed: bool,
}

impl AdminCompanyGameCreate {
    /// At least one relationship is required.
    pub fn is_valid(&self) -> bool {
        self.published || self.developed
    }
}

/// Connects or disconnects one relationship.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AdminCompanyGamePatch {
    /// The game id.
    pub id: String,
    /// The relationship.
    pub action: CompanyAction,
    /// Connect when true, disconnect otherwise.
    pub enabled: bool,
}

/// Disconnects both relationships.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AdminCompanyGameDelete {
    /// The game id.
    pub id: String,
}

/// A selected related record id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AdminRelatedId {
    /// The record id.
    pub id: String,
}

/// The scalar game and selected developer/publisher ids after connecting.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AdminCompanyGame {
    /// The scalar game, flattened.
    #[serde(flatten)]
    pub game: CatalogGame,
    /// Publisher ids.
    pub publishers: Vec<AdminRelatedId>,
    /// Developer ids.
    pub developers: Vec<AdminRelatedId>,
}

/// The uploaded object reference, not the company id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AdminCompanyImage {
    /// The object reference.
    pub id: ObjectReference,
}

/// Multipart descriptor shared by banner and icon uploads. File field names are arbitrary
/// (the UI uses filenames); only the first file is used and at least one is required.
/// There is no literal files part. Total bytes including framing are capped at 32 MiB.
#[derive(Debug, Clone, PartialEq, Eq, TS)]
pub struct AdminCompanyImageForm {
    /// File parts in wire order; text parts are ignored.
    pub files: Vec<AdminUploadFile>,
}
