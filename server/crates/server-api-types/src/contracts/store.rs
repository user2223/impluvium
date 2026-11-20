// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Store filters and the distinct list and featured projections.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::catalog::CatalogGame;
use super::object::ObjectReference;
use crate::enums::{SortOrder, StoreSort};

/// The query string. Lists stay comma-separated, as used by the store UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct StoreQuery {
    /// Items to skip; defaults to the string 0.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub skip: Option<String>,
    /// Items to take; defaults to the string 10 and is capped at 50.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub take: Option<String>,
    /// Comma-separated tag ids.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub tags: Option<String>,
    /// Comma-separated platforms; ignore unrecognized platform names.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub platform: Option<String>,
    /// The company id to filter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub company: Option<String>,
    /// Comma-separated published/developed; defaults to published,developed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub company_actions: Option<String>,
    /// Defaults to default (release date); newest is also release date.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub sort: Option<StoreSort>,
    /// Defaults to descending.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub order: Option<SortOrder>,
}

impl StoreQuery {
    /// Parses ASCII digit paging values and caps the page at 50. A zero take is allowed.
    /// Unlike the old parseInt coercion, trailing junk and negative values are refused.
    ///
    /// # Errors
    ///
    /// A value is not an unsigned 32-bit decimal integer.
    pub fn window(&self) -> Result<(u32, u32), std::num::ParseIntError> {
        let skip = super::paging_integer(self.skip.as_deref().unwrap_or("0"))?;
        let take = super::paging_integer(self.take.as_deref().unwrap_or("10"))?;
        Ok((skip, take.min(50)))
    }
}

/// The store uses results/count rather than the common page envelope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct StoreResponse {
    /// The matching scalar game records.
    pub results: Vec<CatalogGame>,
    /// Total matching games.
    pub count: u32,
}

/// The smaller developer/publisher projection used in featured cards.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct FeaturedCompany {
    /// The company id.
    pub id: String,
    /// The name.
    pub m_name: String,
}

/// The selected fields of a featured game.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct FeaturedGame {
    /// The game id.
    pub id: String,
    /// The name.
    pub m_name: String,
    /// The short description.
    pub m_short_description: String,
    /// The cover object reference.
    pub m_cover_object_id: ObjectReference,
    /// The banner object reference.
    pub m_banner_object_id: ObjectReference,
    /// The developers.
    pub developers: Vec<FeaturedCompany>,
    /// The publishers.
    pub publishers: Vec<FeaturedCompany>,
}

/// The featured handler returns a bare array, ordered by import date descending.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct FeaturedGames(pub Vec<FeaturedGame>);
