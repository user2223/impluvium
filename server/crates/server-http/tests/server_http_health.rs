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
            ("GET", "/api/v1/emoji/{codepoint}"),
            ("GET", "/api/v1/settings"),
            ("POST", "/api/v1/setup"),
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
