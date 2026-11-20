// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Screenshot metadata always belongs to the requesting account. A later handler rejects
//! service tokens (they have no account), limits every list to the caller and returns 404 for
//! a missing or foreign screenshot on reads/deletes, regardless of the private flag or admin
//! role. Delete also removes its backing object as a system operation; direct object-delete
//! permission is not required and is not granted by screenshot ownership.
//!
//! Validate ScreenshotParams for metadata ids and CatalogParams for game ids. The registry
//! buffers raw bytes, not multipart, bounded to 32 MiB after policy and cookie CSRF checks.
//! The later upload handler then verifies the game exists (400 otherwise) before storage.
//! No age restriction or library membership is checked by the inherited upload handler. Create a private screenshot for the caller, recording the
//! upload time, with an object carrying only this account's read permission. Never grant a
//! direct object write/delete permission. Both upload and delete answer 204 with no body.
//! Lists are bare arrays, without pagination. All answers are caller-specific and no-store.

use server_api_types::contracts::object::BinaryUpload;
use server_api_types::contracts::screenshot::{Screenshot, ScreenshotList};

use super::{UPLOAD_BODY_LIMIT, account_right, limits, not_implemented};
use crate::registry::{BodyKind, Contract, Csrf, Method, ParamType, ResponseKind, Route};

pub(super) fn routes() -> Vec<Route> {
    vec![
        Route::new(
            Method::Get,
            "/api/v1/screenshots",
            account_right("screenshots:read"),
            Contract::new(ResponseKind::Json, &[200], limits(0))
                .response_shape::<ScreenshotList>()
                .no_store(),
            not_implemented,
        )
        .expect("the route is valid"),
        Route::new(
            Method::Get,
            "/api/v1/screenshots/{id}",
            account_right("screenshots:read"),
            Contract::new(ResponseKind::Json, &[200], limits(0))
                .response_shape::<Screenshot>()
                .path_param("id", ParamType::String)
                .no_store(),
            not_implemented,
        )
        .expect("the route is valid"),
        Route::new(
            Method::Delete,
            "/api/v1/screenshots/{id}",
            account_right("screenshots:delete"),
            Contract::new(ResponseKind::Empty, &[204], limits(0))
                .path_param("id", ParamType::String)
                .csrf(Csrf::Required)
                .no_store(),
            not_implemented,
        )
        .expect("the route is valid"),
        Route::new(
            Method::Get,
            "/api/v1/screenshots/game/{id}",
            account_right("screenshots:read"),
            Contract::new(ResponseKind::Json, &[200], limits(0))
                .response_shape::<ScreenshotList>()
                .path_param("id", ParamType::String)
                .no_store(),
            not_implemented,
        )
        .expect("the route is valid"),
        Route::new(
            Method::Post,
            "/api/v1/screenshots/game/{id}",
            account_right("screenshots:new"),
            Contract::new(ResponseKind::Empty, &[204], limits(UPLOAD_BODY_LIMIT))
                .path_param("id", ParamType::String)
                .request_body(BodyKind::Binary)
                .request_shape::<BinaryUpload>()
                .csrf(Csrf::Required)
                .no_store(),
            not_implemented,
        )
        .expect("the route is valid"),
    ]
}
