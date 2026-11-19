// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! `/api/v1/admin/auth`: the sign-in methods and the invitations.
//!
//! The caller needs the right `admin:manage`: an administrator's browser session or a token that
//! holds it. The first-run setup also calls the method overview and the creation of an invitation
//! with the setup token; a policy cannot say "this right or the setup right" yet, so the setup
//! token is not accepted here.

use server_api_types::contracts::EmptyObject;
use server_api_types::contracts::admin_auth::{
    AdminAuthInfo, Invitation, InvitationCreate, InvitationDelete, InvitationList,
};
use server_domain::policy::{Policy, Requirement, Right};

use super::{JSON_BODY_LIMIT, limits, not_implemented};
use crate::registry::{BodyKind, Contract, Csrf, Method, ResponseKind, Route};

const ADMIN_RIGHT: &str = "admin:manage";
const PATH: &str = "/api/v1/admin/auth";
const INVITATION_PATH: &str = "/api/v1/admin/auth/invitation";

fn admin() -> Policy {
    let right = Right::new(ADMIN_RIGHT).expect("the right name is valid");
    Policy::protected(
        Requirement::authenticated()
            .require_rights(&[right])
            .expect("the list of rights is not empty"),
    )
}

// How the first-run setup reaches the method overview and the invitation creation below is still
// open and is decided together with the setup credential; the policy here is today's rule.
pub(super) fn routes() -> Vec<Route> {
    let info = Route::new(
        Method::Get,
        PATH,
        admin(),
        Contract::new(ResponseKind::Json, &[200], limits(0)).response_shape::<AdminAuthInfo>(),
        not_implemented,
    )
    .expect("the method overview route is valid");
    let list = Route::new(
        Method::Get,
        INVITATION_PATH,
        admin(),
        Contract::new(ResponseKind::Json, &[200], limits(0)).response_shape::<InvitationList>(),
        not_implemented,
    )
    .expect("the invitation list route is valid");
    let create = Route::new(
        Method::Post,
        INVITATION_PATH,
        admin(),
        Contract::new(ResponseKind::Json, &[200], limits(JSON_BODY_LIMIT))
            .request_body(BodyKind::Json)
            .request_shape::<InvitationCreate>()
            .response_shape::<Invitation>()
            .csrf(Csrf::Required),
        not_implemented,
    )
    .expect("the invitation creation route is valid");
    let delete = Route::new(
        Method::Delete,
        INVITATION_PATH,
        admin(),
        Contract::new(ResponseKind::Json, &[200], limits(JSON_BODY_LIMIT))
            .request_body(BodyKind::Json)
            .request_shape::<InvitationDelete>()
            .response_shape::<EmptyObject>()
            .csrf(Csrf::Required),
        not_implemented,
    )
    .expect("the invitation removal route is valid");
    vec![info, list, create, delete]
}
