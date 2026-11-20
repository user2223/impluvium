// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Collections belong to the caller's account; service tokens have no account and are refused
//! by later handlers. Named rights preserve the desktop store window's collection controls.
//! The list excludes the default library. Reads and additions apply age restrictions, and
//! only the default-library read loads this account's play time. All answers are no-store.
//!
//! Before any read, add, removal or deletion by id, verify ownership of the collection. In
//! particular the old upsert's owner filter on the update branch does not protect its create
//! branch: check the owner before creating an entry. Non-default routes must reject default
//! collection ids, including the real stored id, not just the literal default segment.
//! Literal /default and /default/entry paths take precedence over /{id} and /{id}/entry;
//! registry/path.rs already permits this overlap. Validate CollectionParams and CollectionGameId.
//! Create accepts a string name; entry writes accept JSON { id: gameId }. Keep the existing
//! answers: adding a non-default entry returns the entry, the other entry writes return {},
//! deleting a collection returns { success: true }. A missing removal returns 404 for a
//! non-default entry, while removing a missing default entry still returns {}. Never remove
//! a fallback record. The default library is created lazily for the caller when needed.

use server_api_types::contracts::EmptyObject;
use server_api_types::contracts::collection::{
    Collection, CollectionCreate, CollectionDeleted, CollectionEntry, CollectionGameId,
    CollectionList, DefaultCollection,
};

use super::{JSON_BODY_LIMIT, account_right, limits, not_implemented};
use crate::registry::{BodyKind, Contract, Csrf, Method, ParamType, ResponseKind, Route};

pub(super) fn routes() -> Vec<Route> {
    vec![
        Route::new(
            Method::Get,
            "/api/v1/collection",
            account_right("collections:read"),
            Contract::new(ResponseKind::Json, &[200], limits(0))
                .response_shape::<CollectionList>()
                .no_store(),
            not_implemented,
        )
        .expect("the route is valid"),
        Route::new(
            Method::Post,
            "/api/v1/collection",
            account_right("collections:new"),
            Contract::new(ResponseKind::Json, &[200], limits(JSON_BODY_LIMIT))
                .response_shape::<Collection>()
                .csrf(Csrf::Required)
                .request_body(BodyKind::Json)
                .request_shape::<CollectionCreate>()
                .no_store(),
            not_implemented,
        )
        .expect("the route is valid"),
        Route::new(
            Method::Get,
            "/api/v1/collection/default",
            account_right("collections:read"),
            Contract::new(ResponseKind::Json, &[200], limits(0))
                .response_shape::<DefaultCollection>()
                .no_store(),
            not_implemented,
        )
        .expect("the route is valid"),
        Route::new(
            Method::Post,
            "/api/v1/collection/default/entry",
            account_right("library:add"),
            Contract::new(ResponseKind::Json, &[200], limits(JSON_BODY_LIMIT))
                .response_shape::<EmptyObject>()
                .csrf(Csrf::Required)
                .request_body(BodyKind::Json)
                .request_shape::<CollectionGameId>()
                .no_store(),
            not_implemented,
        )
        .expect("the route is valid"),
        Route::new(
            Method::Delete,
            "/api/v1/collection/default/entry",
            account_right("library:remove"),
            Contract::new(ResponseKind::Json, &[200], limits(JSON_BODY_LIMIT))
                .response_shape::<EmptyObject>()
                .csrf(Csrf::Required)
                .request_body(BodyKind::Json)
                .request_shape::<CollectionGameId>()
                .no_store(),
            not_implemented,
        )
        .expect("the route is valid"),
        Route::new(
            Method::Get,
            "/api/v1/collection/{id}",
            account_right("collections:read"),
            Contract::new(ResponseKind::Json, &[200], limits(0))
                .response_shape::<Collection>()
                .path_param("id", ParamType::String)
                .no_store(),
            not_implemented,
        )
        .expect("the route is valid"),
        Route::new(
            Method::Delete,
            "/api/v1/collection/{id}",
            account_right("collections:delete"),
            Contract::new(ResponseKind::Json, &[200], limits(0))
                .response_shape::<CollectionDeleted>()
                .path_param("id", ParamType::String)
                .csrf(Csrf::Required)
                .no_store(),
            not_implemented,
        )
        .expect("the route is valid"),
        Route::new(
            Method::Post,
            "/api/v1/collection/{id}/entry",
            account_right("collections:add"),
            Contract::new(ResponseKind::Json, &[200], limits(JSON_BODY_LIMIT))
                .response_shape::<CollectionEntry>()
                .path_param("id", ParamType::String)
                .csrf(Csrf::Required)
                .request_body(BodyKind::Json)
                .request_shape::<CollectionGameId>()
                .no_store(),
            not_implemented,
        )
        .expect("the route is valid"),
        Route::new(
            Method::Delete,
            "/api/v1/collection/{id}/entry",
            account_right("collections:remove"),
            Contract::new(ResponseKind::Json, &[200], limits(JSON_BODY_LIMIT))
                .response_shape::<EmptyObject>()
                .path_param("id", ParamType::String)
                .csrf(Csrf::Required)
                .request_body(BodyKind::Json)
                .request_shape::<CollectionGameId>()
                .no_store(),
            not_implemented,
        )
        .expect("the route is valid"),
    ]
}
