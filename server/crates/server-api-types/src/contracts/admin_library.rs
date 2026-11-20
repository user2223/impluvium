// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Library source options are returned verbatim by Nitro, including credentials and paths.
//! Source-route Debug output redacts them; the library list omits them. Answers are no-store.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::admin_game::UnimportedVersion;
use super::catalog::CatalogGame;
use crate::dec64::DecU64;
use crate::enums::{LibraryBackend, LibraryOffline, SortOrder, StoreSort};
use crate::json::JsonValue;

/// Query strings; defaults skip=0, limit=24 (cap 50), sort=default, order=desc.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AdminLibraryQuery {
    /// Case-insensitive name search.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub query: Option<String>,
    /// Nonnegative decimal offset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub skip: Option<String>,
    /// Nonnegative decimal page size; zero is allowed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub limit: Option<String>,
    /// Default/newest use release date; recent uses import date.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub sort: Option<StoreSort>,
    /// Sort direction.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub order: Option<SortOrder>,
    /// Comma-separated version.none, metadata.featured, metadata.noCarousel, metadata.emptyDescription; unknown filters are ignored.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub filters: Option<String>,
}

impl AdminLibraryQuery {
    /// Parses ASCII digit paging strings, refusing signs, whitespace and numeric coercions.
    ///
    /// # Errors
    ///
    /// A paging value is not an unsigned 32-bit decimal integer.
    pub fn window(&self) -> Result<(u32, u32), std::num::ParseIntError> {
        let skip = super::paging_integer(self.skip.as_deref().unwrap_or("0"))?;
        let limit = super::paging_integer(self.limit.as_deref().unwrap_or("24"))?;
        Ok((skip, limit.min(50)))
    }
}

/// The source identity exposed by the library list, without options or credentials.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct LibrarySummary {
    /// The source id.
    pub id: String,
    /// The source name shown in the library UI.
    pub name: String,
    /// The backend.
    pub backend: LibraryBackend,
}

/// A scalar Library record. Stored options can be any bounded JSON value.
#[derive(Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct LibrarySource {
    /// The source id.
    pub id: String,
    /// The name.
    pub name: String,
    /// The backend.
    pub backend: LibraryBackend,
    /// The stored options; secret and path-bearing.
    pub options: JsonValue,
}

redacted_debug!(LibrarySource, shown: [id, name, backend], hidden: [options]);

/// Filesystem byte counts, converted to decimal strings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct LibraryFsStats {
    /// Free bytes.
    pub free_space: DecU64,
    /// Total bytes.
    pub total_space: DecU64,
}

/// A source plus availability; fsStats is absent when unavailable, never null.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct WorkingLibrarySource {
    /// The scalar source, flattened.
    #[serde(flatten)]
    pub source: LibrarySource,
    /// Whether an active provider is registered.
    pub working: bool,
    /// Filesystem statistics, when provided.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub fs_stats: Option<LibraryFsStats>,
}

/// The bare array of configured sources.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct LibrarySourceList(pub Vec<WorkingLibrarySource>);

/// A source write with redacted options.
#[derive(Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct LibrarySourceCreate {
    /// The name.
    pub name: String,
    /// The validated backend.
    pub backend: LibraryBackend,
    /// Options must be an object; may contain credentials and paths.
    pub options: BTreeMap<String, JsonValue>,
}

redacted_debug!(LibrarySourceCreate, shown: [name, backend], hidden: [options]);

/// A source write with redacted options.
#[derive(Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct LibrarySourcePatch {
    /// The existing source id.
    pub id: String,
    /// The new name.
    pub name: String,
    /// Replacement options object; may contain secrets.
    pub options: BTreeMap<String, JsonValue>,
}

redacted_debug!(LibrarySourcePatch, shown: [id, name], hidden: [options]);

/// Deletes one configured source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct LibrarySourceDelete {
    /// The source id.
    pub id: String,
}

/// Only the version path selected by the library list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct LibraryVersionPath {
    /// The path, always present and nullable.
    pub version_path: Option<String>,
}

/// Only the depot fields selected by the library list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct LibraryDepotVersion {
    /// The depot version id.
    pub id: String,
    /// The version name.
    pub version_name: String,
}

/// The scalar game and exactly the loaded library list relations.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AdminLibraryGame {
    /// The scalar game, flattened.
    #[serde(flatten)]
    pub game: CatalogGame,
    /// Only the source identity; options are exclusive to source routes.
    pub library: LibrarySummary,
    /// Selected version paths.
    pub versions: Vec<LibraryVersionPath>,
    /// Selected depot versions.
    pub unimported_game_versions: Vec<LibraryDepotVersion>,
}

/// The status of a reachable library.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct LibraryOnlineStatus {
    /// Whether no version is imported.
    pub no_versions: bool,
    /// Versions awaiting import.
    pub unimported_versions: Vec<UnimportedVersion>,
}

/// A literal offline status or the online status object, without a wrapper tag.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(untagged)]
pub enum AdminLibraryStatus {
    /// The exact string offline.
    Offline(LibraryOffline),
    /// A reachable library.
    Online(LibraryOnlineStatus),
}

/// One game and its computed status.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AdminLibraryEntry {
    /// The loaded game.
    pub game: AdminLibraryGame,
    /// Offline or the status object.
    pub status: AdminLibraryStatus,
}

/// Paged results and the total matching count.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AdminLibraryResponse {
    /// The requested page.
    pub results: Vec<AdminLibraryEntry>,
    /// Total matches.
    pub count: u32,
}

/// Paths awaiting import, keyed by library id.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AdminLibrariesResponse {
    /// Unimported library paths excluding running imports.
    pub unimported_games: BTreeMap<String, Vec<String>>,
    /// Whether any sources are configured.
    pub has_libraries: bool,
}

redacted_debug!(AdminLibrariesResponse, shown: [has_libraries], hidden: [unimported_games]);
