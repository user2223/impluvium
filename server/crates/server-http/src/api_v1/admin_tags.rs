// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Tag administration uses the exact inherited system right. Creation
//! deliberately checks tags:read, not tags:create, matching Nitro. Only finished admin browser
//! sessions and scoped system tokens pass; personal tokens, devices and store sessions do not.
//! Later handlers validate AdminTagParams, sort the bare tag list by name ascending, create
//! from a string name, and return 404 when deletion removes nothing. Deletion returns 204.
//! Scalar tag answers contain no caller data or secrets; API default caching still applies.

use server_api_types::contracts::admin_tags::AdminTagCreate;
use server_api_types::contracts::catalog::{CatalogTag, CatalogTags};

use super::{JSON_BODY_LIMIT, admin_system, limits, not_implemented};
use crate::registry::{BodyKind, Contract, Csrf, Method, ParamType, ResponseKind, Route};

pub(super) fn routes() -> Vec<Route> {
    vec![
        Route::new(
            Method::Get,
            "/api/v1/admin/tags",
            admin_system("tags:read", false),
            Contract::new(ResponseKind::Json, &[200], limits(0)).response_shape::<CatalogTags>(),
            not_implemented,
        )
        .expect("the route is valid"),
        Route::new(
            Method::Post,
            "/api/v1/admin/tags",
            admin_system("tags:read", false),
            Contract::new(ResponseKind::Json, &[200], limits(JSON_BODY_LIMIT))
                .request_body(BodyKind::Json)
                .request_shape::<AdminTagCreate>()
                .response_shape::<CatalogTag>()
                .csrf(Csrf::Required),
            not_implemented,
        )
        .expect("the route is valid"),
        Route::new(
            Method::Delete,
            "/api/v1/admin/tags/{id}",
            admin_system("tags:delete", false),
            Contract::new(ResponseKind::Empty, &[204], limits(0))
                .path_param("id", ParamType::String)
                .csrf(Csrf::Required),
            not_implemented,
        )
        .expect("the route is valid"),
    ]
}
