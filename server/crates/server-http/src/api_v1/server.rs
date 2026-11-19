// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! `GET /api/v1`, `GET /api/v1/settings` and `POST /api/v1/setup`.

use server_api_types::contracts::server::{ServerInfo, ServerSettings};
use server_domain::policy::{Policy, Requirement, Right, SessionKind};

use super::{limits, not_implemented, public};
use crate::registry::{Contract, Csrf, Method, ResponseKind, Route};

const SETTINGS_RIGHT: &str = "settings:read";

/// The right of the one-time setup token. No role carries it.
pub(super) const SETUP_RIGHT: &str = "setup:run";

pub(super) fn routes() -> Vec<Route> {
    let info = Route::new(
        Method::Get,
        "/api/v1",
        public("the name and version of the server, for sign-in pages and clients that look for a server"),
        Contract::new(ResponseKind::Json, &[200], limits(0)).response_shape::<ServerInfo>(),
        not_implemented,
    )
    .expect("the server information route is valid");
    // Needs the right `settings:read`: users, administrators and the store window of the desktop
    // client hold it; a token needs it explicitly.
    let read_settings = Right::new(SETTINGS_RIGHT).expect("the right name is valid");
    let settings = Route::new(
        Method::Get,
        "/api/v1/settings",
        Policy::protected(
            Requirement::authenticated()
                .require_rights(&[read_settings])
                .expect("the list of rights is not empty"),
        ),
        Contract::new(ResponseKind::Json, &[200], limits(0)).response_shape::<ServerSettings>(),
        not_implemented,
    )
    .expect("the settings route is valid");
    // Only the one-time setup token may finish the setup; it is deleted afterwards. No body, and
    // an empty 204 answer. How the first-run setup reaches this route is still open and is
    // decided together with the setup credential; this is today's rule, not the final one.
    let setup_right = Right::new(SETUP_RIGHT).expect("the right name is valid");
    let setup = Route::new(
        Method::Post,
        "/api/v1/setup",
        Policy::protected(
            Requirement::authenticated()
                .require_rights(&[setup_right])
                .expect("the list of rights is not empty")
                .require_session(SessionKind::Token),
        ),
        Contract::new(ResponseKind::Empty, &[204], limits(0)).csrf(Csrf::Required),
        not_implemented,
    )
    .expect("the setup route is valid");
    vec![info, settings, setup]
}
