// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Administration game projections. No manifest or unloaded relation is invented.
//! Multipart types describe parts, never a JSON body; file field names are arbitrary.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::catalog::{CatalogGame, CatalogTag, GameAgeRating, GameVersionSize};
use super::object::{BinaryUpload, ObjectReference};
use crate::dec64::DecU64;
use crate::enums::{
    AgeRatingOrganization, GameType, MetadataSource, Platform, UnimportedVersionKind,
};
use crate::time::Timestamp;

/// An opaque catalog path id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AdminGameParams {
    /// The nonempty record id.
    pub id: String,
}

impl AdminGameParams {
    /// Refuses an empty path id before accessing storage.
    pub fn is_valid(&self) -> bool {
        !self.id.is_empty()
    }
}

/// Partial scalar update; id is taken only from the path. Null is invalid for these nonnullable columns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AdminGamePatch {
    /// The metadata provider.
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
    /// The import time.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "super::non_null_option"
    )]
    #[ts(optional)]
    pub created: Option<Timestamp>,
    /// The content type.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "super::non_null_option"
    )]
    #[ts(optional)]
    #[serde(rename = "type")]
    pub kind: Option<GameType>,
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
    /// The release date.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "super::non_null_option"
    )]
    #[ts(optional)]
    pub m_released: Option<Timestamp>,
    /// Whether featured.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "super::non_null_option"
    )]
    #[ts(optional)]
    pub featured: Option<bool>,
    /// The icon reference.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "super::non_null_option"
    )]
    #[ts(optional)]
    pub m_icon_object_id: Option<ObjectReference>,
    /// The banner reference.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "super::non_null_option"
    )]
    #[ts(optional)]
    pub m_banner_object_id: Option<ObjectReference>,
    /// The cover reference.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "super::non_null_option"
    )]
    #[ts(optional)]
    pub m_cover_object_id: Option<ObjectReference>,
    /// The carousel references.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "super::non_null_option"
    )]
    #[ts(optional)]
    pub m_image_carousel_object_ids: Option<Vec<ObjectReference>>,
    /// The library image references.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "super::non_null_option"
    )]
    #[ts(optional)]
    pub m_image_library_object_ids: Option<Vec<ObjectReference>>,
    /// The library id.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "super::non_null_option"
    )]
    #[ts(optional)]
    pub library_id: Option<String>,
    /// The library path.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "super::non_null_option"
    )]
    #[ts(optional)]
    pub library_path: Option<String>,
}

/// A validated organization/rating pair; the handler also checks the rating vocabulary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AdminAgeRatingInput {
    /// The organization.
    pub organization: AgeRatingOrganization,
    /// An organization-specific rating.
    pub rating: String,
}

/// Replaces all age ratings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AdminAgeRatingsPatch {
    /// The complete set, possibly empty.
    pub age_ratings: Vec<AdminAgeRatingInput>,
}

/// The bare array after replacing the ratings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct AdminAgeRatings(pub Vec<GameAgeRating>);

/// Replaces all game tags.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AdminGameTagsPatch {
    /// The complete set of tag ids.
    pub tags: Vec<String>,
}

/// The requested version order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AdminVersionOrder {
    /// Version ids in the desired order.
    pub versions: Vec<String>,
}

/// The version ids actually reordered.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct AdminVersionIds(pub Vec<String>);

/// A version deletion request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AdminVersionDelete {
    /// The version id, scoped to the path game by the handler.
    pub version: String,
}

/// Removes an image from the game library.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AdminGameImageDelete {
    /// The game id.
    pub game_id: String,
    /// The image reference.
    pub image_id: ObjectReference,
}

/// The selected scalar fields of the image deletion answer on 200.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AdminGameImages {
    /// The banner reference.
    pub m_banner_object_id: ObjectReference,
    /// The remaining library image references.
    pub m_image_library_object_ids: Vec<ObjectReference>,
    /// The cover reference.
    pub m_cover_object_id: ObjectReference,
}

/// One multipart file part, in wire order; names may repeat. This is a descriptor,
/// never a JSON encoding. The upload helper identifies files by a nonempty filename.
#[derive(Debug, Clone, PartialEq, Eq, TS)]
pub struct AdminUploadFile {
    /// The actual form field name, when present; arbitrary names are accepted.
    #[ts(optional)]
    pub name: Option<String>,
    /// The nonempty filename supplied by the form.
    pub filename: String,
    /// The raw file content, redacted by BinaryUpload.
    pub content: BinaryUpload,
}

/// Multipart descriptor: `id` is text; each entry in `files` describes a file part whose
/// actual field name is arbitrary (the UI uses the filename). There is no literal files part.
/// At least one file is required. Total multipart bytes, including framing, are capped at 32 MiB.
#[derive(Debug, Clone, PartialEq, Eq, TS)]
pub struct AdminGameImageForm {
    /// Required text part naming the game.
    pub id: String,
    /// File parts in wire order, preserving duplicates.
    pub files: Vec<AdminUploadFile>,
}

/// Multipart descriptor: optional `name` and `description` text and at most the first file.
/// The UI names the file `icon`; the helper accepts any file field name. No literal files part.
#[derive(Debug, Clone, PartialEq, Eq, TS)]
#[ts(rename_all = "camelCase")]
pub struct AdminGameMetadataForm {
    /// Optional text part; omission leaves the name unchanged, empty text clears it.
    #[ts(optional)]
    pub name: Option<String>,
    /// Optional text part; omission leaves the short description unchanged.
    #[ts(optional)]
    pub description: Option<String>,
    /// Arbitrarily named file parts; the handler uses only the first, when present.
    pub files: Vec<AdminUploadFile>,
}

/// Only the fields selected for the version picker.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AdminLaunchSummary {
    /// The launch id.
    pub launch_id: String,
    /// The launch command.
    pub command: String,
    /// The launch name.
    pub name: String,
    /// The target platform.
    pub platform: Platform,
}

/// The version picker projection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AdminVersionSummary {
    /// The version id.
    pub version_id: String,
    /// The display name, always present and nullable.
    pub display_name: Option<String>,
    /// The source path, always present and nullable.
    pub version_path: Option<String>,
    /// The launch choices.
    pub launches: Vec<AdminLaunchSummary>,
}

/// The bare version picker array, without a prescribed order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct AdminVersionList(pub Vec<AdminVersionSummary>);

/// A scalar SetupConfiguration record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AdminSetup {
    /// The setup id.
    pub setup_id: String,
    /// The command.
    pub command: String,
    /// The platform.
    pub platform: Platform,
    /// The owning version.
    pub version_id: String,
}

/// The scalar LaunchConfiguration record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AdminLaunch {
    /// The launch id.
    pub launch_id: String,
    /// The name.
    pub name: String,
    /// The command.
    pub command: String,
    /// The platform.
    pub platform: Platform,
    /// The emulator id, always present and nullable.
    pub emulator_id: Option<String>,
    /// Suggested emulators.
    pub emulator_suggestions: Vec<String>,
    /// The UMU id override, always present and nullable.
    pub umu_id_override: Option<String>,
    /// The UMU store override, always present and nullable.
    pub umu_store_override: Option<String>,
    /// The owning version.
    pub version_id: String,
}

/// The selected game of an emulator.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AdminEmulatedGame {
    /// The game id.
    pub id: String,
    /// The game name.
    pub m_name: String,
    /// The icon reference.
    pub m_icon_object_id: ObjectReference,
}

/// The selected emulator version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AdminEmulatorVersion {
    /// The version id.
    pub version_id: String,
    /// The nullable display name.
    pub display_name: Option<String>,
    /// The nullable path.
    pub version_path: Option<String>,
    /// The selected game.
    pub game: AdminEmulatedGame,
}

/// An emulator launch with its selected game version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AdminEmulator {
    /// The scalar launch, flattened.
    #[serde(flatten)]
    pub launch: AdminLaunch,
    /// The selected version.
    pub game_version: AdminEmulatorVersion,
}

/// An emulation relation selects only its launch id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AdminLaunchId {
    /// The launch id.
    pub launch_id: String,
}

/// A launch with the relations loaded by the admin game handler.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AdminLaunchDetail {
    /// The scalar launch, flattened.
    #[serde(flatten)]
    pub launch: AdminLaunch,
    /// The emulator, always present and nullable.
    pub emulator: Option<AdminEmulator>,
    /// Dependent launches.
    pub emulations: Vec<AdminLaunchId>,
}

/// A version without dropletManifest; includes sizes and the loaded launches/setups.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AdminGameVersion {
    /// The game id.
    pub game_id: String,
    /// The version id.
    pub version_id: String,
    /// The nullable display name.
    pub display_name: Option<String>,
    /// The nullable source path.
    pub version_path: Option<String>,
    /// The import time.
    pub created: Timestamp,
    /// Whether setup only.
    pub only_setup: bool,
    /// The version file list.
    pub file_list: Vec<String>,
    /// Files removed by a delta.
    pub negative_file_list: Vec<String>,
    /// The ascending version order index.
    pub version_index: i32,
    /// Whether a delta.
    pub delta: bool,
    /// The loaded setups.
    pub setups: Vec<AdminSetup>,
    /// The loaded launches.
    pub launches: Vec<AdminLaunchDetail>,
    /// Disk bytes as a decimal string, or null on failure.
    pub disk_size: Option<DecU64>,
    /// Calculated client sizes, or null on failure.
    pub client_size: Option<GameVersionSize>,
}

/// A local path or depot version awaiting import.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct UnimportedVersion {
    /// Where the version comes from.
    #[serde(rename = "type")]
    pub kind: UnimportedVersionKind,
    /// The display name.
    pub name: String,
    /// The local path or depot id.
    pub identifier: String,
}

/// The scalar game plus exactly the loaded admin relations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AdminGameDetail {
    /// The scalar record, flattened.
    #[serde(flatten)]
    pub game: CatalogGame,
    /// Versions ordered by versionIndex ascending.
    pub versions: Vec<AdminGameVersion>,
    /// The loaded tags.
    pub tags: Vec<CatalogTag>,
    /// The loaded ratings.
    pub age_ratings: Vec<GameAgeRating>,
}

/// The admin detail envelope; unavailable unimported versions are omitted, never null.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AdminGameResponse {
    /// The game.
    pub game: AdminGameDetail,
    /// Unimported versions when the library is available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub unimported_versions: Option<Vec<UnimportedVersion>>,
}
