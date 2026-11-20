// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! The requesting account's collections and default library. Only the default library
//! loads play time; other collections never pretend that relation was loaded.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::catalog::{CatalogGame, LibraryGame};

/// The id of a non-default collection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CollectionParams {
    /// An opaque, nonempty collection id.
    pub id: String,
}

impl CollectionParams {
    /// A default collection is addressed by its literal route, never as an id.
    pub fn is_valid(&self) -> bool {
        !self.id.is_empty() && self.id != "default"
    }
}

/// The JSON body for creating a collection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CollectionCreate {
    /// The name, including an empty name as accepted by the existing handler.
    pub name: String,
}

/// The JSON body of all four entry writes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CollectionGameId {
    /// The game id.
    pub id: String,
}

impl CollectionGameId {
    /// The handlers require a nonempty game id.
    pub fn is_valid(&self) -> bool {
        !self.id.is_empty()
    }
}

/// The entry returned by a non-default collection and by adding an entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CollectionEntry {
    /// The collection id.
    pub collection_id: String,
    /// The game id.
    pub game_id: String,
    /// The scalar game record.
    pub game: CatalogGame,
}

/// An entry of the default library includes the caller's play time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct LibraryEntry {
    /// The collection id.
    pub collection_id: String,
    /// The game id.
    pub game_id: String,
    /// The game with play time.
    pub game: LibraryGame,
}

/// A non-default collection with its entries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Collection {
    /// The collection id.
    pub id: String,
    /// The name.
    pub name: String,
    /// False for these routes.
    pub is_default: bool,
    /// The owner.
    pub user_id: String,
    /// The visible entries.
    pub entries: Vec<CollectionEntry>,
}

/// The default library with its entries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct DefaultCollection {
    /// The collection id.
    pub id: String,
    /// The name.
    pub name: String,
    /// True for the default library.
    pub is_default: bool,
    /// The owner.
    pub user_id: String,
    /// The visible entries.
    pub entries: Vec<LibraryEntry>,
}

/// The collection list is a bare array and excludes the default library.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct CollectionList(pub Vec<Collection>);

/// The collection deletion answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CollectionDeleted {
    /// True after deleting an owned non-default collection.
    pub success: bool,
}
