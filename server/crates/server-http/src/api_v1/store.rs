// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Store listing and filters, guarded by store:read for browsers, store sessions and personal
//! tokens. A later handler must require an account (service tokens have none), apply that
//! account's age restrictions to the list and featured cards, and return only Game content in
//! the paged list. Featured cards need no type filter. Tags are sorted by name.
//! Query defaults are skip=0, take=10 (at most 50), sort=default, order=desc and
//! companyActions=published,developed. Default/newest sort by release date, recent by import
//! date, name by name. Tags, platforms and company actions are comma-separated. Unknown
//! platforms are ignored. The paging validator refuses negative and malformed integers rather
//! than using parseInt's partial coercion. Only the list is paged. Age-filtered answers must
//! remain no-store.

use server_api_types::contracts::catalog::CatalogTags;
use server_api_types::contracts::store::{FeaturedGames, StoreQuery, StoreResponse};

use super::{account_right, limits, not_implemented};
use crate::registry::{Contract, Method, ParamType, ResponseKind, Route};

pub(super) fn routes() -> Vec<Route> {
    vec![
        Route::new(
            Method::Get,
            "/api/v1/store",
            account_right("store:read"),
            Contract::new(ResponseKind::Json, &[200], limits(0))
                .query_shape::<StoreQuery>()
                .query_param("skip", ParamType::String)
                .query_param("take", ParamType::String)
                .query_param("tags", ParamType::String)
                .query_param("platform", ParamType::String)
                .query_param("company", ParamType::String)
                .query_param("companyActions", ParamType::String)
                .query_param("sort", ParamType::String)
                .query_param("order", ParamType::String)
                .response_shape::<StoreResponse>()
                .no_store(),
            not_implemented,
        )
        .expect("the route is valid"),
        Route::new(
            Method::Get,
            "/api/v1/store/featured",
            account_right("store:read"),
            Contract::new(ResponseKind::Json, &[200], limits(0))
                .response_shape::<FeaturedGames>()
                .no_store(),
            not_implemented,
        )
        .expect("the route is valid"),
        Route::new(
            Method::Get,
            "/api/v1/store/tags",
            account_right("store:read"),
            Contract::new(ResponseKind::Json, &[200], limits(0)).response_shape::<CatalogTags>(),
            not_implemented,
        )
        .expect("the route is valid"),
    ]
}
