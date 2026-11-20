// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Public catalog records. Scalar names keep the existing wire spelling, including the
//! metadata prefix. Relations appear only in the answers whose handlers load them.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::object::ObjectReference;
use crate::dec64::DecU64;
use crate::enums::{AgeRatingOrganization, GameType, MetadataSource, Platform};
use crate::time::Timestamp;

/// The game, company or tag id in a catalog path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CatalogParams {
    /// An opaque, nonempty id.
    pub id: String,
}

impl CatalogParams {
    /// Whether the id supplied in the path is nonempty, as the catalog handlers require.
    pub fn is_valid(&self) -> bool {
        !self.id.is_empty()
    }
}

/// The scalar game record returned by the store and collections.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CatalogGame {
    /// The game id.
    pub id: String,
    /// The metadata provider.
    pub metadata_source: MetadataSource,
    /// The provider id.
    pub metadata_id: String,
    /// When the game was imported.
    pub created: Timestamp,
    /// The kind of content.
    #[serde(rename = "type")]
    pub kind: GameType,
    /// The game name.
    pub m_name: String,
    /// The short description.
    pub m_short_description: String,
    /// The Markdown description.
    pub m_description: String,
    /// The release date.
    pub m_released: Timestamp,
    /// Whether the game is featured.
    pub featured: bool,
    /// The icon object reference.
    pub m_icon_object_id: ObjectReference,
    /// The banner object reference.
    pub m_banner_object_id: ObjectReference,
    /// The cover object reference.
    pub m_cover_object_id: ObjectReference,
    /// The carousel image object references.
    pub m_image_carousel_object_ids: Vec<ObjectReference>,
    /// The library image object references.
    pub m_image_library_object_ids: Vec<ObjectReference>,
    /// The library id.
    pub library_id: String,
    /// The path within the library.
    pub library_path: String,
}

/// The scalar company record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CatalogCompany {
    /// The company id.
    pub id: String,
    /// The metadata provider.
    pub metadata_source: MetadataSource,
    /// The provider id.
    pub metadata_id: String,
    /// The original metadata query.
    pub metadata_original_query: String,
    /// The name.
    pub m_name: String,
    /// The short description.
    pub m_short_description: String,
    /// The Markdown description.
    pub m_description: String,
    /// The logo object reference.
    pub m_logo_object_id: ObjectReference,
    /// The banner object reference.
    pub m_banner_object_id: ObjectReference,
    /// The website.
    pub m_website: String,
}

/// A tag; the handler does not load its games.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CatalogTag {
    /// The tag id.
    pub id: String,
    /// The tag name.
    pub name: String,
}

/// The bare tag array returned by the store.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct CatalogTags(pub Vec<CatalogTag>);

/// A developer or publisher selected by the game detail handler.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct GameCompany {
    /// The company id.
    pub id: String,
    /// The name.
    pub m_name: String,
    /// The short description.
    pub m_short_description: String,
    /// The logo object reference.
    pub m_logo_object_id: ObjectReference,
}

/// An age rating loaded for a game.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct GameAgeRating {
    /// The rating id.
    pub id: String,
    /// The game id.
    pub game_id: String,
    /// The rating organization.
    pub organization: AgeRatingOrganization,
    /// The organization-specific rating.
    pub rating: String,
    /// The badge URL; always present, possibly null.
    pub rating_cover_url: Option<String>,
}

/// The play time of the requesting account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct GamePlaytime {
    /// The game id.
    pub game_id: String,
    /// The account id.
    pub user_id: String,
    /// Seconds played.
    pub seconds: u32,
    /// When tracking started.
    pub created_at: Timestamp,
    /// When tracking last changed.
    pub updated_at: Timestamp,
}

/// The default library adds play time to the scalar game record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct LibraryGame {
    /// The scalar record, flattened onto the wire.
    #[serde(flatten)]
    pub game: CatalogGame,
    /// Only the requesting account's play time.
    pub playtime: Vec<GamePlaytime>,
}

/// The game detail record, without versions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct GameDetail {
    /// The scalar record, flattened onto the wire.
    #[serde(flatten)]
    pub game: CatalogGame,
    /// The developers.
    pub developers: Vec<GameCompany>,
    /// The publishers.
    pub publishers: Vec<GameCompany>,
    /// The tags.
    pub tags: Vec<CatalogTag>,
    /// The age ratings.
    pub age_ratings: Vec<GameAgeRating>,
    /// Only the requesting account's play time.
    pub playtime: Vec<GamePlaytime>,
}

/// The only version summary the public game handler answers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct GameVersionSize {
    /// The version id.
    pub version_id: String,
    /// Installed bytes as a decimal string.
    pub install_size: DecU64,
    /// Download bytes as a decimal string.
    pub download_size: DecU64,
}

/// The average review rating, including an empty aggregate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RatingAverage {
    /// The average, or null when no rating exists.
    pub m_review_rating: Option<f64>,
}

/// The sum of review counts, including an empty aggregate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RatingCount {
    /// The total as a decimal string, or null.
    pub m_review_count: Option<DecU64>,
}

/// The aggregate returned by the game handler.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct GameRating {
    /// The average.
    #[serde(rename = "_avg")]
    pub average: RatingAverage,
    /// The count sum.
    #[serde(rename = "_sum")]
    pub sum: RatingCount,
}

/// The public game answer. A size calculation may fail and yields a null array element.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct GameResponse {
    /// The game with selected relations.
    pub game: GameDetail,
    /// The review aggregates.
    pub rating: GameRating,
    /// Sizes in descending version order; a failed calculation is null.
    pub sizes: Vec<Option<GameVersionSize>>,
    /// Distinct platforms from launches and setups.
    pub platforms: Vec<Platform>,
}

/// The public company answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CompanyResponse {
    /// The company record.
    pub company: CatalogCompany,
}

/// The public tag answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct TagResponse {
    /// The tag record.
    pub tag: CatalogTag,
}
