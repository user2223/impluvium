// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Liveness and readiness probes.

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use axum::http::StatusCode;
use common::{get, router_with};
use server_domain::credential::BoxFuture;
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
        registry::application_with(switch.clone()),
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
        [("GET", "/api/health/live"), ("GET", "/api/health/ready")]
    );
    assert!(
        inventory
            .iter()
            .all(|info| info.policy.public_reason().is_some())
    );
}
