// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

use serde_json::{Value, json};

use super::admin_company::*;
use super::admin_game::*;
use super::admin_library::*;
use super::admin_tags::*;
use super::catalog_tests::{company, game};
use super::tests::round_trip;
use crate::TS;

const DATE: &str = "2026-01-01T00:00:00.000Z";

fn launch() -> Value {
    json!({ "launchId": "launch", "name": "Play", "command": "game", "platform": "Linux",
        "emulatorId": null, "emulatorSuggestions": [], "umuIdOverride": null,
        "umuStoreOverride": null, "versionId": "version" })
}

fn version() -> Value {
    let mut loaded = launch();
    loaded["emulator"] = Value::Null;
    loaded["emulations"] = json!([]);
    json!({ "gameId": "game", "versionId": "version", "displayName": null, "versionPath": null,
        "created": DATE, "onlySetup": false, "fileList": ["game"], "negativeFileList": [],
        "versionIndex": 0, "delta": false, "setups": [{ "setupId": "setup", "command": "setup",
            "platform": "Windows", "versionId": "version" }], "launches": [loaded],
        "diskSize": null, "clientSize": null })
}

fn source() -> Value {
    json!({ "id": "library", "name": "Library", "backend": "Filesystem",
        "options": { "password": "secret-marker", "basePath": "private-path-marker" } })
}

#[test]
fn admin_scalar_patches_keep_optional_fields_and_wire_names() {
    let _: AdminGamePatch = round_trip(json!({}));
    let mut patch = game();
    patch.as_object_mut().unwrap().remove("id");
    let _: AdminGamePatch = round_trip(patch);
    let _: AdminCompanyPatch = round_trip(json!({}));
    let mut patch = company();
    patch.as_object_mut().unwrap().remove("id");
    let _: AdminCompanyPatch = round_trip(patch);
    assert!(serde_json::from_value::<AdminGamePatch>(json!({ "featured": "true" })).is_err());
    assert!(serde_json::from_value::<AdminGamePatch>(json!({ "mReleased": "invalid" })).is_err());
    assert!(
        serde_json::from_value::<AdminCompanyPatch>(json!({ "metadataSource": "other" })).is_err()
    );
    for value in [json!({ "mName": null }), json!({ "metadataSource": null })] {
        assert!(serde_json::from_value::<AdminGamePatch>(value.clone()).is_err());
        assert!(serde_json::from_value::<AdminCompanyPatch>(value).is_err());
    }
    let config = ts_rs::Config::new();
    let declaration = AdminGamePatch::decl(&config);
    assert!(declaration.contains("mName?: string"));
    assert!(!declaration.contains("| null"));
}

#[test]
fn admin_game_detail_preserves_loaded_relations_and_optional_versions() {
    let mut detail = game();
    detail["versions"] = json!([version()]);
    detail["tags"] = json!([{ "id": "tag", "name": "Tag" }]);
    detail["ageRatings"] = json!([]);
    let absent: AdminGameResponse = round_trip(json!({ "game": detail }));
    assert!(absent.unimported_versions.is_none());
    let mut answer = serde_json::to_value(absent).unwrap();
    answer["unimportedVersions"] = json!([{ "type": "local", "name": "release", "identifier": "release" },
        { "type": "depot", "name": "release", "identifier": "depot" }]);
    let answer: AdminGameResponse = round_trip(answer);
    let encoded = serde_json::to_value(answer).unwrap();
    for absent in ["playtime", "developers", "publishers", "ratings"] {
        assert!(encoded["game"].get(absent).is_none());
    }
    assert!(
        encoded["game"]["versions"][0]
            .get("dropletManifest")
            .is_none()
    );
    assert!(encoded["game"]["versions"][0]["displayName"].is_null());
    assert!(encoded["game"]["versions"][0]["diskSize"].is_null());
}

#[test]
fn admin_version_sizes_are_decimal_strings_and_emulator_projection_is_exact() {
    let mut value = version();
    value["diskSize"] = json!("18446744073709551615");
    value["clientSize"] = json!({ "versionId": "version", "installSize": "9007199254740993",
        "downloadSize": "42" });
    let mut emulator = launch();
    emulator["gameVersion"] = json!({ "versionId": "emulator-version", "displayName": null,
        "versionPath": null, "game": { "id": "emulator", "mName": "Emulator", "mIconObjectId": "icon" } });
    value["launches"][0]["emulator"] = emulator;
    value["launches"][0]["emulations"] = json!([{ "launchId": "dependent" }]);
    let answer: AdminGameVersion = round_trip(value.clone());
    assert_eq!(answer.disk_size.unwrap().get(), u64::MAX);
    value["diskSize"] = json!(42);
    assert!(serde_json::from_value::<AdminGameVersion>(value).is_err());
    let _: AdminVersionList = round_trip(json!([{ "versionId": "version", "displayName": null,
        "versionPath": null, "launches": [{ "launchId": "launch", "command": "game",
            "name": "Play", "platform": "macOS" }] }]));
}

#[test]
fn admin_game_write_shapes_round_trip_without_extra_relations() {
    let _: AdminAgeRatingsPatch =
        round_trip(json!({ "ageRatings": [{ "organization": "PEGI", "rating": "12" }] }));
    let _: AdminAgeRatings = round_trip(json!([{ "id": "rating", "gameId": "game",
        "organization": "PEGI", "rating": "12", "ratingCoverUrl": null }]));
    let _: AdminGameTagsPatch = round_trip(json!({ "tags": [] }));
    let _: AdminVersionOrder = round_trip(json!({ "versions": ["version"] }));
    let _: AdminVersionIds = round_trip(json!(["version"]));
    let _: AdminVersionDelete = round_trip(json!({ "version": "version" }));
    let _: AdminGameImageDelete = round_trip(json!({ "gameId": "game", "imageId": "image" }));
    let _: AdminGameImages = round_trip(json!({ "mBannerObjectId": "banner",
        "mImageLibraryObjectIds": [], "mCoverObjectId": "cover" }));
    assert!(
        serde_json::from_value::<AdminAgeRatingsPatch>(json!({ "ageRatings": [{
        "organization": "other", "rating": "12" }] }))
        .is_err()
    );
}

#[test]
fn admin_company_projections_and_relationship_requests_round_trip() {
    let _: AdminCompanyList = round_trip(json!([company()]));
    let _: AdminCompanyCreate = round_trip(json!({ "name": "", "description": "", "website": "" }));
    let mut detail = company();
    detail["developed"] = json!(["game"]);
    detail["published"] = json!([]);
    let _: AdminCompanyResponse = round_trip(json!({ "company": detail, "games": [game()] }));
    let request: AdminCompanyGameCreate =
        round_trip(json!({ "id": "game", "developed": true, "published": false }));
    assert!(request.is_valid());
    assert!(
        !AdminCompanyGameCreate {
            id: "game".into(),
            developed: false,
            published: false
        }
        .is_valid()
    );
    let _: AdminCompanyGamePatch =
        round_trip(json!({ "id": "game", "action": "published", "enabled": false }));
    let _: AdminCompanyGameDelete = round_trip(json!({ "id": "game" }));
    let mut connected = game();
    connected["developers"] = json!([{ "id": "company" }]);
    connected["publishers"] = json!([]);
    let _: AdminCompanyGame = round_trip(connected);
    let _: AdminCompanyImage = round_trip(json!({ "id": "object" }));
}

#[test]
fn admin_library_query_defaults_bounds_and_filters_match_the_list() {
    let empty: AdminLibraryQuery = round_trip(json!({}));
    assert_eq!(empty.window(), Ok((0, 24)));
    let query: AdminLibraryQuery = round_trip(json!({ "query": "Game", "skip": "2", "limit": "80",
        "sort": "name", "order": "asc", "filters": "version.none,metadata.featured" }));
    assert_eq!(query.window(), Ok((2, 50)));
    let query: AdminLibraryQuery = round_trip(json!({ "limit": "0" }));
    assert_eq!(query.window(), Ok((0, 0)));
    let query: AdminLibraryQuery = round_trip(json!({ "skip": "0002", "limit": "0050" }));
    assert_eq!(query.window(), Ok((2, 50)));
    for invalid in [
        "",
        "+1",
        "-1",
        "1.5",
        "12junk",
        "4294967296",
        " 1",
        "1 ",
        "\u{ff11}",
        "1e2",
    ] {
        for field in ["skip", "limit"] {
            let query: AdminLibraryQuery =
                serde_json::from_value(json!({ (field): invalid })).unwrap();
            assert!(query.window().is_err(), "{field} {invalid}");
        }
    }
}

#[test]
fn library_sources_redact_options_and_keep_optional_stats_and_decimal_bytes() {
    let scalar: LibrarySource = round_trip(source());
    let debug = format!("{scalar:?}");
    assert!(debug.contains("[redacted]"));
    assert!(!debug.contains("secret-marker") && !debug.contains("private-path-marker"));
    let mut working = source();
    working["working"] = json!(true);
    let absent: WorkingLibrarySource = round_trip(working.clone());
    assert!(absent.fs_stats.is_none());
    working["fsStats"] =
        json!({ "freeSpace": "9007199254740993", "totalSpace": "18446744073709551615" });
    let value: LibrarySourceList = round_trip(json!([working.clone()]));
    assert!(!format!("{value:?}").contains("secret-marker"));
    working["fsStats"]["freeSpace"] = json!(1);
    assert!(serde_json::from_value::<WorkingLibrarySource>(working).is_err());
    let create: LibrarySourceCreate =
        round_trip(json!({ "name": "Library", "backend": "Filesystem",
        "options": { "password": "secret-marker" } }));
    let patch: LibrarySourcePatch = round_trip(json!({ "id": "library", "name": "Library",
        "options": { "basePath": "private-path-marker" } }));
    assert!(!format!("{create:?}").contains("secret-marker"));
    assert!(!format!("{patch:?}").contains("private-path-marker"));
    assert!(
        serde_json::from_value::<LibrarySourceCreate>(
            json!({ "name": "x", "backend": "other", "options": {} })
        )
        .is_err()
    );
    assert!(
        serde_json::from_value::<LibrarySourceCreate>(
            json!({ "name": "x", "backend": "Filesystem", "options": [] })
        )
        .is_err()
    );
    let _: LibrarySourceDelete = round_trip(json!({ "id": "library" }));
    let mut scalar = source();
    scalar["options"] = Value::Null;
    let _: LibrarySource = round_trip(scalar);
}

#[test]
fn admin_library_list_keeps_online_offline_and_loaded_relation_shapes() {
    let mut loaded = game();
    loaded["library"] = json!({ "id": "library", "name": "Library", "backend": "Filesystem" });
    loaded["versions"] = json!([{ "versionPath": null }]);
    loaded["unimportedGameVersions"] = json!([{ "id": "depot", "versionName": "release" }]);
    for status in [
        json!("offline"),
        json!({ "noVersions": false,
        "unimportedVersions": [{ "type": "depot", "name": "release", "identifier": "depot" }] }),
    ] {
        let value: AdminLibraryResponse =
            round_trip(json!({ "results": [{ "game": loaded, "status": status }], "count": 1 }));
        assert!(!format!("{value:?}").contains("secret-marker"));
    }
    assert!(serde_json::from_value::<AdminLibraryStatus>(json!("other")).is_err());
    let value: AdminLibrariesResponse = round_trip(
        json!({ "unimportedGames": { "library": ["private-path-marker"] }, "hasLibraries": true }),
    );
    assert!(!format!("{value:?}").contains("private-path-marker"));
}

#[test]
fn admin_path_ids_and_tag_create_validate_the_wire_types() {
    for id in ["", "opaque-id"] {
        let game: AdminGameParams = round_trip(json!({ "id": id }));
        let company: AdminCompanyParams = round_trip(json!({ "id": id }));
        let tag: AdminTagParams = round_trip(json!({ "id": id }));
        assert_eq!(game.is_valid(), !id.is_empty());
        assert_eq!(company.is_valid(), !id.is_empty());
        assert_eq!(tag.is_valid(), !id.is_empty());
    }
    let _: AdminTagCreate = round_trip(json!({ "name": "" }));
}

#[test]
fn multipart_descriptors_export_fields_and_redact_file_bytes() {
    use super::object::BinaryUpload;
    let file = BinaryUpload {
        bytes: b"secret-file-marker".to_vec(),
    };
    let files = vec![AdminUploadFile {
        name: Some("arbitrary.png".into()),
        filename: "arbitrary.png".into(),
        content: file,
    }];
    let image = AdminGameImageForm {
        id: "game".into(),
        files: files.clone(),
    };
    let metadata = AdminGameMetadataForm {
        name: None,
        description: Some(String::new()),
        files: files.clone(),
    };
    let company = AdminCompanyImageForm { files };
    for debug in [
        format!("{image:?}"),
        format!("{metadata:?}"),
        format!("{company:?}"),
    ] {
        assert!(debug.contains("[redacted]"));
        assert!(!debug.contains("secret-file-marker"));
    }
    let config = ts_rs::Config::new();
    assert!(AdminGameImageForm::decl(&config).contains("files:"));
    assert!(AdminGameMetadataForm::decl(&config).contains("name?: string"));
    for name in [
        "AdminGameResponse",
        "AdminCompanyResponse",
        "LibrarySource",
        "AdminLibraryResponse",
        "AdminGameImageForm",
    ] {
        assert!(crate::export::root_names().iter().any(|root| root == name));
    }
}

#[test]
fn library_list_projection_excludes_source_options() {
    let expected = json!({ "id": "library", "name": "Library", "backend": "Filesystem" });
    let _: LibrarySummary = round_trip(expected.clone());
    let mut loaded = game();
    loaded["library"] = source();
    loaded["versions"] = json!([]);
    loaded["unimportedGameVersions"] = json!([]);
    let answer: AdminLibraryResponse = serde_json::from_value(json!({
        "results": [{ "game": loaded, "status": "offline" }], "count": 1,
    }))
    .unwrap();
    let encoded = serde_json::to_value(answer).unwrap();
    assert_eq!(encoded["results"][0]["game"]["library"], expected);
    assert!(!encoded.to_string().contains("secret-marker"));
    assert!(!encoded.to_string().contains("private-path-marker"));
    assert!(!LibrarySummary::decl(&ts_rs::Config::new()).contains("options"));
    assert!(
        crate::export::root_names()
            .iter()
            .any(|name| name == "LibrarySummary")
    );
}
