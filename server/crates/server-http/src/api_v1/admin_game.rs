// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Game administration requires the named Nitro system right. Finished
//! admin browser sessions hold these rights; only system tokens with the scoped right pass.
//! Personal tokens, devices, store sessions and unfinished sign-ins are refused.
//!
//! Later handlers validate AdminGameParams, scope every version operation to that game,
//! refuse missing records and never select a fallback record. Detail requires a library id,
//! omits dropletManifest and unloaded relations, orders versions by versionIndex ascending,
//! preserves nullable size failures, and omits unavailable unimportedVersions. Admin routes
//! do not apply age restrictions. Convert dates to Timestamp and all byte counts to DecU64.
//! Scalar patches whitelist the DTO fields, ignore a body id, reject null for nonnullable
//! columns and Prisma operation objects; do not forward arbitrary input to Prisma.
//! The scalar PATCH accepts up to 1 MiB of JSON for the complete Markdown mDescription;
//! other JSON requests keep the 64 KiB default.
//! Replacing age ratings validates the organization-specific vocabulary and returns the full
//! new array. Replacing tags returns 204. Version order must preserve base-before-delta for
//! each platform; return the ids actually reordered. Game/version deletion returns {}.
//!
//! Multipart bodies are buffered after CSRF and policy, bounded to 32 MiB including framing.
//! The 32 MiB cap covers the whole request, not each file. The current image UI sends all
//! selected files in one form; the later handler/UI must upload per file or show this total
//! limit. The registry checks only the content type/boundary and total length; a later parser
//! must inspect file parts in order.
//! Image upload accepts arbitrary file field names and required text id, rejects no files or
//! an unknown game, pushes all ids into mImageLibraryObjectIds and commits the staged objects.
//! Metadata accepts optional name/description text and the first optional file (UI: icon).
//! Missing text leaves fields unchanged; empty text is a value. Stage/pull/dump consistently.
//! Image deletion requires membership in the game's image library, deletes as system, and
//! updates cover/banner to the first remaining image where needed; never invent an object id.
//! The inherited image writes can return undefined after an update race: json-or-empty means
//! a selected JSON projection on 200 or no body on 204. Empty remaining image lists need a
//! storage decision for the nonnullable cover/banner columns; do not emit undefined fields.
//! All game administration answers are no-store because they carry library paths or commands.

use server_api_types::contracts::EmptyObject;
use server_api_types::contracts::admin_game::{
    AdminAgeRatings, AdminAgeRatingsPatch, AdminGameImageDelete, AdminGameImageForm,
    AdminGameImages, AdminGameMetadataForm, AdminGamePatch, AdminGameResponse, AdminGameTagsPatch,
    AdminVersionDelete, AdminVersionIds, AdminVersionList, AdminVersionOrder,
};
use server_api_types::contracts::catalog::CatalogGame;

use super::{
    GAME_PATCH_BODY_LIMIT, JSON_BODY_LIMIT, UPLOAD_BODY_LIMIT, admin_system, limits,
    not_implemented,
};
use crate::registry::{BodyKind, Contract, Csrf, Method, ParamType, ResponseKind, Route};

pub(super) fn routes() -> Vec<Route> {
    vec![
        Route::new(
            Method::Get,
            "/api/v1/admin/game/{id}",
            admin_system("game:read", false),
            Contract::new(ResponseKind::Json, &[200], limits(0))
                .path_param("id", ParamType::String)
                .response_shape::<AdminGameResponse>()
                .no_store(),
            not_implemented,
        )
        .expect("the route is valid"),
        Route::new(
            Method::Patch,
            "/api/v1/admin/game/{id}",
            admin_system("game:update", false),
            Contract::new(ResponseKind::Json, &[200], limits(GAME_PATCH_BODY_LIMIT))
                .path_param("id", ParamType::String)
                .request_body(BodyKind::Json)
                .request_shape::<AdminGamePatch>()
                .response_shape::<CatalogGame>()
                .csrf(Csrf::Required)
                .no_store(),
            not_implemented,
        )
        .expect("the route is valid"),
        Route::new(
            Method::Delete,
            "/api/v1/admin/game/{id}",
            admin_system("game:delete", false),
            Contract::new(ResponseKind::Json, &[200], limits(0))
                .path_param("id", ParamType::String)
                .response_shape::<EmptyObject>()
                .csrf(Csrf::Required)
                .no_store(),
            not_implemented,
        )
        .expect("the route is valid"),
        Route::new(
            Method::Patch,
            "/api/v1/admin/game/{id}/age-ratings",
            admin_system("game:update", false),
            Contract::new(ResponseKind::Json, &[200], limits(JSON_BODY_LIMIT))
                .path_param("id", ParamType::String)
                .request_body(BodyKind::Json)
                .request_shape::<AdminAgeRatingsPatch>()
                .response_shape::<AdminAgeRatings>()
                .csrf(Csrf::Required)
                .no_store(),
            not_implemented,
        )
        .expect("the route is valid"),
        Route::new(
            Method::Post,
            "/api/v1/admin/game/{id}/metadata",
            admin_system("game:update", false),
            Contract::new(ResponseKind::Json, &[200], limits(UPLOAD_BODY_LIMIT))
                .path_param("id", ParamType::String)
                .request_body(BodyKind::Multipart)
                .request_shape::<AdminGameMetadataForm>()
                .response_shape::<CatalogGame>()
                .csrf(Csrf::Required)
                .no_store(),
            not_implemented,
        )
        .expect("the route is valid"),
        Route::new(
            Method::Patch,
            "/api/v1/admin/game/{id}/tags",
            admin_system("game:update", false),
            Contract::new(ResponseKind::Empty, &[204], limits(JSON_BODY_LIMIT))
                .path_param("id", ParamType::String)
                .request_body(BodyKind::Json)
                .request_shape::<AdminGameTagsPatch>()
                .csrf(Csrf::Required)
                .no_store(),
            not_implemented,
        )
        .expect("the route is valid"),
        Route::new(
            Method::Get,
            "/api/v1/admin/game/{id}/versions",
            admin_system("game:read", false),
            Contract::new(ResponseKind::Json, &[200], limits(0))
                .path_param("id", ParamType::String)
                .response_shape::<AdminVersionList>()
                .no_store(),
            not_implemented,
        )
        .expect("the route is valid"),
        Route::new(
            Method::Patch,
            "/api/v1/admin/game/{id}/versions",
            admin_system("game:version:update", false),
            Contract::new(ResponseKind::Json, &[200], limits(JSON_BODY_LIMIT))
                .path_param("id", ParamType::String)
                .request_body(BodyKind::Json)
                .request_shape::<AdminVersionOrder>()
                .response_shape::<AdminVersionIds>()
                .csrf(Csrf::Required)
                .no_store(),
            not_implemented,
        )
        .expect("the route is valid"),
        Route::new(
            Method::Delete,
            "/api/v1/admin/game/{id}/versions",
            admin_system("game:version:delete", false),
            Contract::new(ResponseKind::Json, &[200], limits(JSON_BODY_LIMIT))
                .path_param("id", ParamType::String)
                .request_body(BodyKind::Json)
                .request_shape::<AdminVersionDelete>()
                .response_shape::<EmptyObject>()
                .csrf(Csrf::Required)
                .no_store(),
            not_implemented,
        )
        .expect("the route is valid"),
        Route::new(
            Method::Post,
            "/api/v1/admin/game/image",
            admin_system("game:image:new", false),
            Contract::new(
                ResponseKind::JsonOrEmpty,
                &[200, 204],
                limits(UPLOAD_BODY_LIMIT),
            )
            .request_body(BodyKind::Multipart)
            .request_shape::<AdminGameImageForm>()
            .response_shape::<CatalogGame>()
            .csrf(Csrf::Required)
            .no_store(),
            not_implemented,
        )
        .expect("the route is valid"),
        Route::new(
            Method::Delete,
            "/api/v1/admin/game/image",
            admin_system("game:image:delete", false),
            Contract::new(
                ResponseKind::JsonOrEmpty,
                &[200, 204],
                limits(JSON_BODY_LIMIT),
            )
            .request_body(BodyKind::Json)
            .request_shape::<AdminGameImageDelete>()
            .response_shape::<AdminGameImages>()
            .csrf(Csrf::Required)
            .no_store(),
            not_implemented,
        )
        .expect("the route is valid"),
    ]
}
