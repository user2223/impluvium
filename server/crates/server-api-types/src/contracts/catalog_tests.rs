// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

use serde_json::{Value, json};

use super::catalog::{
    CatalogGame, CatalogParams, CatalogTags, CompanyResponse, GameAgeRating, GameResponse,
    GameVersionSize, TagResponse,
};
use super::collection::{
    Collection, CollectionCreate, CollectionDeleted, CollectionEntry, CollectionGameId,
    CollectionList, CollectionParams, DefaultCollection,
};
use super::object::{BinaryUpload, ObjectDownload, ObjectMutation, ObjectParams, ObjectReference};
use super::screenshot::{Screenshot, ScreenshotList, ScreenshotParams};
use super::store::{FeaturedGames, StoreQuery, StoreResponse};
use super::tests::round_trip;
use crate::TS;

const DATE: &str = "2026-01-01T00:00:00.000Z";

fn game() -> Value {
    json!({
        "id": "game", "metadataSource": "Manual", "metadataId": "source", "created": DATE,
        "type": "Game", "mName": "Game", "mShortDescription": "Short", "mDescription": "Long",
        "mReleased": DATE, "featured": false, "mIconObjectId": "icon", "mBannerObjectId": "banner",
        "mCoverObjectId": "cover", "mImageCarouselObjectIds": [], "mImageLibraryObjectIds": ["image"],
        "libraryId": "library", "libraryPath": "game",
    })
}

fn company() -> Value {
    json!({
        "id": "company", "metadataSource": "Manual", "metadataId": "source",
        "metadataOriginalQuery": "query", "mName": "Company", "mShortDescription": "Short",
        "mDescription": "Long", "mLogoObjectId": "logo", "mBannerObjectId": "banner",
        "mWebsite": "https://games.example.test",
    })
}

fn playtime() -> Value {
    json!({ "gameId": "game", "userId": "account", "seconds": 7,
        "createdAt": DATE, "updatedAt": DATE })
}

#[test]
fn store_filters_keep_absence_and_paging_rules() {
    let empty: StoreQuery = round_trip(json!({}));
    assert_eq!(empty.window(), Ok((0, 10)));
    let query: StoreQuery = round_trip(json!({
        "skip": "3", "take": "80", "tags": "a,b", "platform": "Windows,Linux",
        "company": "company", "companyActions": "developed,published",
        "sort": "recent", "order": "asc",
    }));
    assert_eq!(query.window(), Ok((3, 50)));
    for (take, expected) in [("0", 0), ("1", 1), ("50", 50)] {
        let query: StoreQuery = serde_json::from_value(json!({ "take": take })).unwrap();
        assert_eq!(query.window(), Ok((0, expected)));
    }
    for bad in ["-1", "12junk", "", "4294967296"] {
        for name in ["skip", "take"] {
            let query: StoreQuery = serde_json::from_value(json!({ (name): bad })).unwrap();
            assert!(query.window().is_err(), "{name} {bad}");
        }
    }
    assert!(serde_json::from_value::<StoreQuery>(json!({ "skip": 3 })).is_err());
    assert!(serde_json::from_value::<StoreQuery>(json!({ "sort": "other" })).is_err());
    assert!(serde_json::from_value::<StoreQuery>(json!({ "order": "other" })).is_err());
}

#[test]
fn store_and_featured_answers_have_distinct_projections() {
    let _: StoreResponse = round_trip(json!({ "results": [game()], "count": 1 }));
    let _: CatalogGame = round_trip(game());
    let _: FeaturedGames = round_trip(json!([{
        "id": "game", "mName": "Game", "mShortDescription": "Short",
        "mCoverObjectId": "cover", "mBannerObjectId": "banner",
        "developers": [{ "id": "company", "mName": "Company" }], "publishers": [],
    }]));
    let _: FeaturedGames = round_trip(json!([]));
    let _: CatalogTags = round_trip(json!([{ "id": "tag", "name": "Tag" }]));
}

#[test]
fn catalog_details_keep_selected_relations_and_null_sizes() {
    let mut detail = game();
    detail["developers"] = json!([{ "id": "company", "mName": "Company",
        "mShortDescription": "Short", "mLogoObjectId": "logo" }]);
    detail["publishers"] = json!([]);
    detail["tags"] = json!([{ "id": "tag", "name": "Tag" }]);
    detail["ageRatings"] = json!([{ "id": "rating", "gameId": "game",
        "organization": "PEGI", "rating": "12", "ratingCoverUrl": null }]);
    detail["playtime"] = json!([playtime()]);
    let answer: GameResponse = round_trip(json!({
        "game": detail, "rating": { "_avg": { "mReviewRating": 0.5 },
            "_sum": { "mReviewCount": "9007199254740993" } },
        "sizes": [null, { "versionId": "version", "installSize": "18446744073709551615",
            "downloadSize": "9007199254740993" }], "platforms": ["macOS", "Linux"],
    }));
    assert!(answer.sizes[0].is_none());
    assert_eq!(
        answer.sizes[1].as_ref().unwrap().install_size.get(),
        u64::MAX
    );
    let encoded = serde_json::to_value(&answer).unwrap();
    for absent in [
        "versions",
        "launches",
        "setups",
        "dropletManifest",
        "fileList",
        "ratings",
    ] {
        assert!(encoded["game"].get(absent).is_none(), "{absent}");
    }
    assert!(
        serde_json::from_value::<GameVersionSize>(json!({
            "versionId": "version", "installSize": 42, "downloadSize": "0",
        }))
        .is_err()
    );
}

#[test]
fn company_tag_and_nullable_rating_round_trip() {
    let _: CompanyResponse = round_trip(json!({ "company": company() }));
    let _: TagResponse = round_trip(json!({ "tag": { "id": "tag", "name": "Tag" } }));
    let rating = json!({ "id": "rating", "gameId": "game", "organization": "ESRB",
        "rating": "E", "ratingCoverUrl": null });
    let _: GameAgeRating = round_trip(rating.clone());
    let mut absent = rating;
    absent.as_object_mut().unwrap().remove("ratingCoverUrl");
    let value: GameAgeRating = serde_json::from_value(absent).unwrap();
    assert!(serde_json::to_value(value).unwrap()["ratingCoverUrl"].is_null());
    let _: super::catalog::GameRating = round_trip(json!({
        "_avg": { "mReviewRating": null }, "_sum": { "mReviewCount": null },
    }));
}

#[test]
fn collection_shapes_distinguish_default_playtime() {
    let entry = json!({ "collectionId": "collection", "gameId": "game", "game": game() });
    let _: CollectionEntry = round_trip(entry.clone());
    let collection = json!({ "id": "collection", "name": "Collection", "isDefault": false,
        "userId": "account", "entries": [entry] });
    let _: Collection = round_trip(collection.clone());
    let _: CollectionList = round_trip(json!([collection]));
    let mut library_game = game();
    library_game["playtime"] = json!([playtime()]);
    let library: DefaultCollection = round_trip(json!({ "id": "library", "name": "Library",
        "isDefault": true, "userId": "account", "entries": [{ "collectionId": "library",
            "gameId": "game", "game": library_game }] }));
    assert_eq!(library.entries[0].game.playtime[0].seconds, 7);
    assert!(serde_json::from_value::<super::catalog::LibraryGame>(game()).is_err());
}

#[test]
fn collection_write_bodies_and_results_round_trip() {
    let _: CollectionCreate = round_trip(json!({ "name": "" }));
    let request: CollectionGameId = round_trip(json!({ "id": "game" }));
    assert!(request.is_valid());
    assert!(!CollectionGameId { id: String::new() }.is_valid());
    assert!(serde_json::from_value::<CollectionGameId>(json!({})).is_err());
    assert!(serde_json::from_value::<CollectionGameId>(json!({ "id": 1 })).is_err());
    let _: CollectionDeleted = round_trip(json!({ "success": true }));
    for success in [false, true] {
        let _: ObjectMutation = round_trip(json!({ "success": success }));
    }
    let _: super::EmptyObject = round_trip(json!({}));
}

#[test]
fn screenshots_round_trip_without_object_contents() {
    let body = json!({ "id": "screenshot", "gameId": "game", "userId": "account",
        "objectId": "object", "private": true, "createdAt": DATE });
    let screenshot: Screenshot = round_trip(body.clone());
    assert_eq!(screenshot.object_id, ObjectReference("object".into()));
    let _: ScreenshotList = round_trip(json!([body]));
    let _: ScreenshotList = round_trip(json!([]));
}

#[test]
fn object_binary_descriptors_and_upload_debug_do_not_expose_bytes() {
    let upload = BinaryUpload {
        bytes: b"private image".to_vec(),
    };
    let debug = format!("{upload:?}");
    assert!(debug.contains("[redacted]"));
    assert!(!debug.contains("private") && !debug.contains("112"));
    let config = ts_rs::Config::new();
    assert_eq!(
        BinaryUpload::inline(&config),
        "Blob | ArrayBuffer | Uint8Array<ArrayBuffer>"
    );
    assert_eq!(ObjectDownload::inline(&config), "Blob | undefined");
    let _: ObjectReference = round_trip(json!("object"));
}

#[test]
fn path_parameters_validate_opaque_ids() {
    let catalog: CatalogParams = round_trip(json!({ "id": "opaque-id" }));
    assert!(catalog.is_valid());
    assert!(!CatalogParams { id: String::new() }.is_valid());
    for id in ["collection", "opaque-id"] {
        let params: CollectionParams = round_trip(json!({ "id": id }));
        assert!(params.is_valid());
    }
    assert!(
        !CollectionParams {
            id: "default".into()
        }
        .is_valid()
    );
    assert!(!CollectionParams { id: String::new() }.is_valid());
    for id in ["object", "profile-picture.png", "non-uuid"] {
        let params: ObjectParams = round_trip(json!({ "id": id }));
        assert!(params.is_valid());
        let params: ScreenshotParams = round_trip(json!({ "id": id }));
        assert!(params.is_valid());
    }
    for id in [
        "",
        ".",
        "..",
        "../other",
        "other/name",
        "name.",
        "name ",
        "name:stream",
        "CON",
        "aux.png",
        "COM1",
        "lpt9.txt",
        "a\\nb",
    ] {
        assert!(!ObjectParams { id: id.into() }.is_valid(), "{id}");
        assert!(!ScreenshotParams { id: id.into() }.is_valid(), "{id}");
    }
}
