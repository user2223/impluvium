// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Liveness and readiness probes.

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use axum::http::StatusCode;
use common::{get, router_with};
use server_domain::credential::BoxFuture;
use server_http::emoji::EmojiFiles;
use server_http::health::ReadinessPort;
use server_http::registry;
use server_http::router::AppOptions;

struct Switch(AtomicBool);

impl ReadinessPort for Switch {
    fn ready(&self) -> BoxFuture<'_, bool> {
        let up = self.0.load(Ordering::SeqCst);
        Box::pin(async move { up })
    }
}

#[test]
fn ready_follows_the_dependencies_while_live_stays_up() {
    let switch = Arc::new(Switch(AtomicBool::new(false)));
    let router = router_with(
        registry::application_with(switch.clone(), Arc::new(EmojiFiles::empty())),
        &AppOptions::default(),
    );
    let live = get(&router, "/api/health/live");
    assert_eq!(live.status, StatusCode::OK);
    assert_eq!(live.json()["status"], "live");
    let down = get(&router, "/api/health/ready");
    assert_eq!(down.status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(down.json()["code"], "not_ready");
    assert_eq!(get(&router, "/api/health/live").status, StatusCode::OK);
    switch.0.store(true, Ordering::SeqCst);
    let up = get(&router, "/api/health/ready");
    assert_eq!(up.status, StatusCode::OK);
    assert_eq!(up.json()["status"], "ready");
}

#[test]
fn probes_are_public_and_listed_in_the_inventory() {
    let inventory = registry::application().inventory();
    let listed: Vec<(&str, &str)> = inventory
        .iter()
        .map(|info| (info.method, info.path.as_str()))
        .collect();
    assert_eq!(
        listed,
        [
            ("GET", "/api/health/live"),
            ("GET", "/api/health/ready"),
            ("GET", "/api/v1"),
            ("GET", "/api/v1/admin/auth"),
            ("GET", "/api/v1/admin/auth/invitation"),
            ("POST", "/api/v1/admin/auth/invitation"),
            ("DELETE", "/api/v1/admin/auth/invitation"),
            ("GET", "/api/v1/admin/company"),
            ("POST", "/api/v1/admin/company"),
            ("GET", "/api/v1/admin/company/{id}"),
            ("PATCH", "/api/v1/admin/company/{id}"),
            ("DELETE", "/api/v1/admin/company/{id}"),
            ("POST", "/api/v1/admin/company/{id}/banner"),
            ("POST", "/api/v1/admin/company/{id}/game"),
            ("PATCH", "/api/v1/admin/company/{id}/game"),
            ("DELETE", "/api/v1/admin/company/{id}/game"),
            ("POST", "/api/v1/admin/company/{id}/icon"),
            ("POST", "/api/v1/admin/game/image"),
            ("DELETE", "/api/v1/admin/game/image"),
            ("GET", "/api/v1/admin/game/{id}"),
            ("PATCH", "/api/v1/admin/game/{id}"),
            ("DELETE", "/api/v1/admin/game/{id}"),
            ("PATCH", "/api/v1/admin/game/{id}/age-ratings"),
            ("POST", "/api/v1/admin/game/{id}/metadata"),
            ("PATCH", "/api/v1/admin/game/{id}/tags"),
            ("GET", "/api/v1/admin/game/{id}/versions"),
            ("PATCH", "/api/v1/admin/game/{id}/versions"),
            ("DELETE", "/api/v1/admin/game/{id}/versions"),
            ("GET", "/api/v1/admin/library"),
            ("GET", "/api/v1/admin/library/libraries"),
            ("GET", "/api/v1/admin/library/sources"),
            ("POST", "/api/v1/admin/library/sources"),
            ("PATCH", "/api/v1/admin/library/sources"),
            ("DELETE", "/api/v1/admin/library/sources"),
            ("GET", "/api/v1/admin/tags"),
            ("POST", "/api/v1/admin/tags"),
            ("DELETE", "/api/v1/admin/tags/{id}"),
            ("GET", "/api/v1/admin/token"),
            ("POST", "/api/v1/admin/token"),
            ("GET", "/api/v1/admin/token/acls"),
            ("DELETE", "/api/v1/admin/token/{id}"),
            ("GET", "/api/v1/admin/users"),
            ("GET", "/api/v1/admin/users/{id}"),
            ("DELETE", "/api/v1/admin/users/{id}"),
            ("GET", "/api/v1/auth"),
            ("GET", "/api/v1/auth/mfa"),
            ("POST", "/api/v1/auth/mfa/totp"),
            ("POST", "/api/v1/auth/mfa/webauthn/finish"),
            ("POST", "/api/v1/auth/mfa/webauthn/start"),
            ("GET", "/api/v1/auth/oidc/callback"),
            ("POST", "/api/v1/auth/oidc/logout"),
            ("POST", "/api/v1/auth/passkey/finish"),
            ("POST", "/api/v1/auth/passkey/start"),
            ("POST", "/api/v1/auth/signin/simple"),
            ("GET", "/api/v1/auth/signup/simple"),
            ("POST", "/api/v1/auth/signup/simple"),
            ("GET", "/api/v1/collection"),
            ("POST", "/api/v1/collection"),
            ("GET", "/api/v1/collection/default"),
            ("POST", "/api/v1/collection/default/entry"),
            ("DELETE", "/api/v1/collection/default/entry"),
            ("GET", "/api/v1/collection/{id}"),
            ("DELETE", "/api/v1/collection/{id}"),
            ("POST", "/api/v1/collection/{id}/entry"),
            ("DELETE", "/api/v1/collection/{id}/entry"),
            ("GET", "/api/v1/companies/{id}"),
            ("GET", "/api/v1/emoji/{codepoint}"),
            ("GET", "/api/v1/games/{id}"),
            ("GET", "/api/v1/object/{id}"),
            ("POST", "/api/v1/object/{id}"),
            ("DELETE", "/api/v1/object/{id}"),
            ("GET", "/api/v1/screenshots"),
            ("GET", "/api/v1/screenshots/game/{id}"),
            ("POST", "/api/v1/screenshots/game/{id}"),
            ("GET", "/api/v1/screenshots/{id}"),
            ("DELETE", "/api/v1/screenshots/{id}"),
            ("GET", "/api/v1/settings"),
            ("POST", "/api/v1/setup"),
            ("GET", "/api/v1/store"),
            ("GET", "/api/v1/store/featured"),
            ("GET", "/api/v1/store/tags"),
            ("GET", "/api/v1/tags/{id}"),
            ("GET", "/api/v1/token"),
            ("GET", "/api/v1/user"),
            ("GET", "/api/v1/user/auth"),
            ("GET", "/api/v1/user/client"),
            ("DELETE", "/api/v1/user/client/{id}"),
            ("GET", "/api/v1/user/mfa"),
            ("POST", "/api/v1/user/mfa/totp/finish"),
            ("POST", "/api/v1/user/mfa/totp/start"),
            ("DELETE", "/api/v1/user/mfa/webauthn"),
            ("POST", "/api/v1/user/mfa/webauthn/finish"),
            ("POST", "/api/v1/user/mfa/webauthn/start"),
            ("GET", "/api/v1/user/superlevel"),
            ("GET", "/api/v1/user/token"),
            ("POST", "/api/v1/user/token"),
            ("GET", "/api/v1/user/token/acls"),
            ("DELETE", "/api/v1/user/token/{id}"),
            ("GET", "/api/v1/user/{id}"),
            ("GET", "/auth/callback/oidc"),
            ("GET", "/auth/oidc"),
            ("GET", "/auth/signout"),
        ]
    );
    for info in inventory
        .iter()
        .filter(|info| info.path.starts_with("/api/health/"))
    {
        assert!(info.policy.public_reason().is_some(), "{}", info.path);
    }
    let emoji = inventory
        .iter()
        .find(|info| info.path == "/api/v1/emoji/{codepoint}")
        .expect("the emoji route is listed");
    assert!(emoji.policy.public_reason().is_none());
}
