// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Company administration uses the named Nitro system right. Only finished
//! browser sessions and system tokens qualify; the latter need the named right too.
//!
//! Later handlers validate AdminCompanyParams and return 404 for missing companies/games.
//! List/create/patch return scalar CatalogCompany; detail flattens developed/published to id
//! arrays and returns distinct scalar games. Creation builds a manual metadata record and
//! internal-read logo/banner objects. Scalar patches whitelist the DTO, ignore a body id,
//! reject null and Prisma operation objects, and never forward arbitrary input to Prisma.
//! POST game requires developed or published and returns the scalar game plus selected
//! developer/publisher ids. PATCH toggles one relation; DELETE disconnects both; both are 204.
//! Company deletion is 204 and must not delete a fallback record.
//!
//! Banner/icon multipart bodies are bounded to 32 MiB including framing, after CSRF and
//! policy. Any file field name is accepted; only the first file is used and one is required.
//! Text parts are ignored. Later parsing/storage checks company existence (400 for an invalid
//! upload target), replaces its object, commits or dumps the staged upload, and returns
//! { id: objectReference }. These are metadata object ids, never file bytes or a company id.
//! Answers are no-store: related games carry library paths, and metadata updates are private.

use server_api_types::contracts::admin_company::{
    AdminCompanyCreate, AdminCompanyGame, AdminCompanyGameCreate, AdminCompanyGameDelete,
    AdminCompanyGamePatch, AdminCompanyImage, AdminCompanyImageForm, AdminCompanyList,
    AdminCompanyPatch, AdminCompanyResponse,
};
use server_api_types::contracts::catalog::CatalogCompany;

use super::{JSON_BODY_LIMIT, UPLOAD_BODY_LIMIT, admin_system, limits, not_implemented};
use crate::registry::{BodyKind, Contract, Csrf, Method, ParamType, ResponseKind, Route};

pub(super) fn routes() -> Vec<Route> {
    vec![
        Route::new(
            Method::Get,
            "/api/v1/admin/company",
            admin_system("company:read", false),
            Contract::new(ResponseKind::Json, &[200], limits(0))
                .response_shape::<AdminCompanyList>()
                .no_store(),
            not_implemented,
        )
        .expect("the route is valid"),
        Route::new(
            Method::Post,
            "/api/v1/admin/company",
            admin_system("company:create", false),
            Contract::new(ResponseKind::Json, &[200], limits(JSON_BODY_LIMIT))
                .request_body(BodyKind::Json)
                .request_shape::<AdminCompanyCreate>()
                .response_shape::<CatalogCompany>()
                .csrf(Csrf::Required)
                .no_store(),
            not_implemented,
        )
        .expect("the route is valid"),
        Route::new(
            Method::Get,
            "/api/v1/admin/company/{id}",
            admin_system("company:read", false),
            Contract::new(ResponseKind::Json, &[200], limits(0))
                .path_param("id", ParamType::String)
                .response_shape::<AdminCompanyResponse>()
                .no_store(),
            not_implemented,
        )
        .expect("the route is valid"),
        Route::new(
            Method::Patch,
            "/api/v1/admin/company/{id}",
            admin_system("company:update", false),
            Contract::new(ResponseKind::Json, &[200], limits(JSON_BODY_LIMIT))
                .path_param("id", ParamType::String)
                .request_body(BodyKind::Json)
                .request_shape::<AdminCompanyPatch>()
                .response_shape::<CatalogCompany>()
                .csrf(Csrf::Required)
                .no_store(),
            not_implemented,
        )
        .expect("the route is valid"),
        Route::new(
            Method::Delete,
            "/api/v1/admin/company/{id}",
            admin_system("company:delete", false),
            Contract::new(ResponseKind::Empty, &[204], limits(0))
                .path_param("id", ParamType::String)
                .csrf(Csrf::Required)
                .no_store(),
            not_implemented,
        )
        .expect("the route is valid"),
        Route::new(
            Method::Post,
            "/api/v1/admin/company/{id}/banner",
            admin_system("company:update", false),
            Contract::new(ResponseKind::Json, &[200], limits(UPLOAD_BODY_LIMIT))
                .path_param("id", ParamType::String)
                .request_body(BodyKind::Multipart)
                .request_shape::<AdminCompanyImageForm>()
                .response_shape::<AdminCompanyImage>()
                .csrf(Csrf::Required)
                .no_store(),
            not_implemented,
        )
        .expect("the route is valid"),
        Route::new(
            Method::Post,
            "/api/v1/admin/company/{id}/icon",
            admin_system("company:update", false),
            Contract::new(ResponseKind::Json, &[200], limits(UPLOAD_BODY_LIMIT))
                .path_param("id", ParamType::String)
                .request_body(BodyKind::Multipart)
                .request_shape::<AdminCompanyImageForm>()
                .response_shape::<AdminCompanyImage>()
                .csrf(Csrf::Required)
                .no_store(),
            not_implemented,
        )
        .expect("the route is valid"),
        Route::new(
            Method::Post,
            "/api/v1/admin/company/{id}/game",
            admin_system("company:update", false),
            Contract::new(ResponseKind::Json, &[200], limits(JSON_BODY_LIMIT))
                .path_param("id", ParamType::String)
                .request_body(BodyKind::Json)
                .request_shape::<AdminCompanyGameCreate>()
                .response_shape::<AdminCompanyGame>()
                .csrf(Csrf::Required)
                .no_store(),
            not_implemented,
        )
        .expect("the route is valid"),
        Route::new(
            Method::Patch,
            "/api/v1/admin/company/{id}/game",
            admin_system("company:update", false),
            Contract::new(ResponseKind::Empty, &[204], limits(JSON_BODY_LIMIT))
                .path_param("id", ParamType::String)
                .request_body(BodyKind::Json)
                .request_shape::<AdminCompanyGamePatch>()
                .csrf(Csrf::Required)
                .no_store(),
            not_implemented,
        )
        .expect("the route is valid"),
        Route::new(
            Method::Delete,
            "/api/v1/admin/company/{id}/game",
            admin_system("company:update", false),
            Contract::new(ResponseKind::Empty, &[204], limits(JSON_BODY_LIMIT))
                .path_param("id", ParamType::String)
                .request_body(BodyKind::Json)
                .request_shape::<AdminCompanyGameDelete>()
                .csrf(Csrf::Required)
                .no_store(),
            not_implemented,
        )
        .expect("the route is valid"),
    ]
}
