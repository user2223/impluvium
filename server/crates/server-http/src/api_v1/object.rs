// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Objects use per-object permissions, so the route policy is deliberately public. The later
//! handlers must resolve the credential and use an account only when authentication is
//! complete and the matching object:read/update/delete right is held. Anonymous, pending,
//! service tokens and credentials without that right use only anonymous object permissions.
//! Public policy never grants access to the object by itself. Exact permission subjects are
//! required: do not copy the old startsWith account-prefix matching. Internal grants apply
//! only to a resolved account, anonymous grants to everyone. Any stored permission permits
//! reading; write/delete need their respective permission or a higher one. Admin status adds
//! no bypass. Denied reads are 404; denied/missing writes and deletes return { success: false }.
//!
//! Validate ObjectParams before any storage access; reject unsafe names instead of sanitizing
//! into a different id. Uploads are raw nonempty bytes, limited to 32 MiB and protected by CSRF
//! for cookie callers. The registry bounds the complete body even when an eventual handler
//! reads a stream directly. These are existing objects: POST must not create a missing object.
//! POST can replace their bytes while GET advertises immutable caching. This inherited
//! replacement/cache mismatch remains open for the storage implementation.
//!
//! GET is binary-or-empty: 200 carries the stored bytes and metadata Content-Type, 304 has no
//! body. Authorize before reading a hash, evaluating If-None-Match or opening storage. Emit
//! ETag (empty when no hash), and for GET the existing Cache-Control value
//! private, max-age=31536000, s-maxage=31536000, immutable, including on 304. The route is not
//! forced no-store: the API default fills only an absent Cache-Control. Later handlers must
//! explicitly set this value to retain object caching. Errors remain no-store.
//!
//! The registry GET also answers HEAD. The later handler must inspect the request method:
//! HEAD checks permission, sets Content-Type/ETag, evaluates If-None-Match and returns 200/304
//! without fetching a download body, even when the backing source is missing. Hash computation
//! may still read storage, as it does today. Match Nitro's HEAD
//! comparison (a present request value equal to the actual hash); GET requires nonempty values.
//! HEAD sets no cache header, as in Nitro, so the API default supplies no-store. Axum strips
//! response bytes for HEAD, but avoiding the separate download fetch belongs to the handler.

use server_api_types::contracts::object::{BinaryUpload, ObjectDownload, ObjectMutation};

use super::{UPLOAD_BODY_LIMIT, limits, not_implemented, public};
use crate::registry::{BodyKind, Contract, Csrf, Method, ParamType, ResponseKind, Route};

pub(super) fn routes() -> Vec<Route> {
    let access = || {
        public(
            "object access is checked against stored permissions, including anonymous grants, by the handler",
        )
    };
    vec![
        Route::new(
            Method::Get,
            "/api/v1/object/{id}",
            access(),
            Contract::new(ResponseKind::BinaryOrEmpty, &[200, 304], limits(0))
                .path_param("id", ParamType::String)
                .response_shape::<ObjectDownload>(),
            not_implemented,
        )
        .expect("the route is valid"),
        Route::new(
            Method::Post,
            "/api/v1/object/{id}",
            access(),
            Contract::new(ResponseKind::Json, &[200], limits(UPLOAD_BODY_LIMIT))
                .path_param("id", ParamType::String)
                .request_body(BodyKind::Binary)
                .request_shape::<BinaryUpload>()
                .response_shape::<ObjectMutation>()
                .csrf(Csrf::Required)
                .no_store(),
            not_implemented,
        )
        .expect("the route is valid"),
        Route::new(
            Method::Delete,
            "/api/v1/object/{id}",
            access(),
            Contract::new(ResponseKind::Json, &[200], limits(0))
                .response_shape::<ObjectMutation>()
                .path_param("id", ParamType::String)
                .csrf(Csrf::Required)
                .no_store(),
            not_implemented,
        )
        .expect("the route is valid"),
    ]
}
