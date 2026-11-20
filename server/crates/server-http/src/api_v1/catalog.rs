// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Public catalog metadata requires store:read, including in the desktop store window.
//! Every handler still needs an account behind the principal and rejects service tokens.
//! The game handler applies age restrictions before fetching details and answers 404 when
//! restricted or missing, includes only this account's play time, computes ratings and version
//! sizes in descending version order, preserves null size failures, and deduplicates platforms
//! from setups/launches. Versions themselves are deleted from the answer; do not expose their
//! manifests, file lists or launch commands. Company and tag lookups answer 404 when missing.
//! Convert all byte sizes and the review-count sum to decimal strings and dates to Timestamp.
//! Path ids are opaque nonempty strings validated with CatalogParams, not necessarily UUIDs.
//! Administrators are exempt from age restrictions. For restricted accounts a game must
//! have at least one age rating and no banned organization/rating pair to be visible.

use server_api_types::contracts::catalog::{CompanyResponse, GameResponse, TagResponse};

use super::{account_right, limits, not_implemented};
use crate::registry::{Contract, Method, ParamType, ResponseKind, Route};

pub(super) fn routes() -> Vec<Route> {
    vec![
        Route::new(
            Method::Get,
            "/api/v1/games/{id}",
            account_right("store:read"),
            Contract::new(ResponseKind::Json, &[200], limits(0))
                .response_shape::<GameResponse>()
                .path_param("id", ParamType::String)
                .no_store(),
            not_implemented,
        )
        .expect("the route is valid"),
        Route::new(
            Method::Get,
            "/api/v1/companies/{id}",
            account_right("store:read"),
            Contract::new(ResponseKind::Json, &[200], limits(0))
                .response_shape::<CompanyResponse>()
                .path_param("id", ParamType::String),
            not_implemented,
        )
        .expect("the route is valid"),
        Route::new(
            Method::Get,
            "/api/v1/tags/{id}",
            account_right("store:read"),
            Contract::new(ResponseKind::Json, &[200], limits(0))
                .response_shape::<TagResponse>()
                .path_param("id", ParamType::String),
            not_implemented,
        )
        .expect("the route is valid"),
    ]
}
