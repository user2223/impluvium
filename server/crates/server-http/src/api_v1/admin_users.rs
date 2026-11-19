// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! `/api/v1/admin/users`: the list of accounts, one account and its removal.
//!
//! The handlers today ask for the system rights `user:read` and `user:delete`. Here every route
//! needs `admin:manage`, as the other administration routes do: how a token gets a part of the
//! administration is decided together with the token rights, and until then a token can only
//! hold all of it or none.
//!
//! `DELETE /api/v1/admin/users/{id}` refuses an unknown id and never removes a fallback account.

use server_api_types::contracts::admin_users::{AdminUserList, UserDeleted};
use server_api_types::contracts::user::UserAccount;

use super::admin_auth::admin;
use super::{limits, not_implemented};
use crate::registry::{Contract, Csrf, Method, ParamType, ResponseKind, Route};

pub(super) fn routes() -> Vec<Route> {
    let list = Route::new(
        Method::Get,
        "/api/v1/admin/users",
        admin(),
        Contract::new(ResponseKind::Json, &[200], limits(0)).response_shape::<AdminUserList>(),
        not_implemented,
    )
    .expect("the account list route is valid");
    let one = Route::new(
        Method::Get,
        "/api/v1/admin/users/{id}",
        admin(),
        Contract::new(ResponseKind::Json, &[200], limits(0))
            .path_param("id", ParamType::String)
            .response_shape::<UserAccount>(),
        not_implemented,
    )
    .expect("the account route is valid");
    let remove = Route::new(
        Method::Delete,
        "/api/v1/admin/users/{id}",
        admin(),
        Contract::new(ResponseKind::Json, &[200], limits(0))
            .path_param("id", ParamType::String)
            .response_shape::<UserDeleted>()
            .csrf(Csrf::Required),
        not_implemented,
    )
    .expect("the account removal route is valid");
    vec![list, one, remove]
}
