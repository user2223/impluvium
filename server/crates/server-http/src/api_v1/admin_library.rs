// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Library administration requires the scoped Nitro system right, with the setup:run
//! alternative on source routes only. System tokens receive only their mapped scoped rights
//! (legacy setup maps to setup:run), never an implicit admin:manage. Personal tokens never
//! receive admin:manage either. Personal tokens and devices are not system credentials;
//! finished admin browser sessions pass, while store sessions do not.
//!
//! Later list handlers search names case-insensitively and merge recognized comma-separated
//! filters: version.none, metadata.featured, metadata.noCarousel, metadata.emptyDescription.
//! Unknown filters are ignored. Default/newest sort by release, recent by import, name by name;
//! defaults skip=0, limit=24 capped at 50, order=desc. Validate integer paging without fractional
//! or negative coercion. Return results/count, exactly the loaded library and version/depot
//! projections, and status either offline or { noVersions, unimportedVersions }. Project the
//! game's library to id/name/backend only; library:read must never expose its source options.
//! The libraries answer maps source ids to unimported paths, excluding active imports, and
//! hasLibraries counts configured sources. No administration route applies age restrictions.
//!
//! Sources return stored options verbatim, including credentials/paths: redact Debug and keep
//! every answer no-store. Stored options may be any bounded JSON; create/update require an
//! object. Validate the backend enum, instantiate and test the provider before saving, then
//! register it. Update preserves the backend and replaces options/name; missing update is 400
//! before save or 404 after a race. Delete removes the provider and returns 204, or 404 when
//! missing. Working records flatten the Library columns; fsStats is optional, never null,
//! omitted on update and when unavailable on list/create. Byte counts use decimal strings.

use server_api_types::contracts::admin_library::{
    AdminLibrariesResponse, AdminLibraryQuery, AdminLibraryResponse, LibrarySourceCreate,
    LibrarySourceDelete, LibrarySourceList, LibrarySourcePatch, WorkingLibrarySource,
};

use super::{JSON_BODY_LIMIT, admin_system, limits, not_implemented};
use crate::registry::{BodyKind, Contract, Csrf, Method, ParamType, ResponseKind, Route};

pub(super) fn routes() -> Vec<Route> {
    vec![
        Route::new(
            Method::Get,
            "/api/v1/admin/library",
            admin_system("library:read", false),
            Contract::new(ResponseKind::Json, &[200], limits(0))
                .query_shape::<AdminLibraryQuery>()
                .query_param("query", ParamType::String)
                .query_param("skip", ParamType::String)
                .query_param("limit", ParamType::String)
                .query_param("sort", ParamType::String)
                .query_param("order", ParamType::String)
                .query_param("filters", ParamType::String)
                .response_shape::<AdminLibraryResponse>()
                .no_store(),
            not_implemented,
        )
        .expect("the route is valid"),
        Route::new(
            Method::Get,
            "/api/v1/admin/library/libraries",
            admin_system("library:read", false),
            Contract::new(ResponseKind::Json, &[200], limits(0))
                .response_shape::<AdminLibrariesResponse>()
                .no_store(),
            not_implemented,
        )
        .expect("the route is valid"),
        Route::new(
            Method::Get,
            "/api/v1/admin/library/sources",
            admin_system("library:sources:read", true),
            Contract::new(ResponseKind::Json, &[200], limits(0))
                .response_shape::<LibrarySourceList>()
                .no_store(),
            not_implemented,
        )
        .expect("the route is valid"),
        Route::new(
            Method::Post,
            "/api/v1/admin/library/sources",
            admin_system("library:sources:new", true),
            Contract::new(ResponseKind::Json, &[200], limits(JSON_BODY_LIMIT))
                .request_body(BodyKind::Json)
                .request_shape::<LibrarySourceCreate>()
                .response_shape::<WorkingLibrarySource>()
                .csrf(Csrf::Required)
                .no_store(),
            not_implemented,
        )
        .expect("the route is valid"),
        Route::new(
            Method::Patch,
            "/api/v1/admin/library/sources",
            admin_system("library:sources:update", true),
            Contract::new(ResponseKind::Json, &[200], limits(JSON_BODY_LIMIT))
                .request_body(BodyKind::Json)
                .request_shape::<LibrarySourcePatch>()
                .response_shape::<WorkingLibrarySource>()
                .csrf(Csrf::Required)
                .no_store(),
            not_implemented,
        )
        .expect("the route is valid"),
        Route::new(
            Method::Delete,
            "/api/v1/admin/library/sources",
            admin_system("library:sources:delete", true),
            Contract::new(ResponseKind::Empty, &[204], limits(JSON_BODY_LIMIT))
                .request_body(BodyKind::Json)
                .request_shape::<LibrarySourceDelete>()
                .csrf(Csrf::Required)
                .no_store(),
            not_implemented,
        )
        .expect("the route is valid"),
    ]
}
