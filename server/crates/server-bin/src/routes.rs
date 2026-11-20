// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! The `routes --json` command: the inventory of the routes the server
//! actually serves. It needs neither a database nor key files.

use std::process::ExitCode;

use serde_json::{Value, json};
use server_domain::policy::Policy;
use server_http::registry::{self, Contract, Csrf, ParamSpec, Registry, RouteInfo};
use server_http::router::FallbackInfo;

/// Prints the inventory of the application registry.
pub(crate) fn run_json() -> ExitCode {
    println!("{}", render(&registry::application()));
    ExitCode::SUCCESS
}

/// Renders the inventory as pretty-printed JSON.
pub(crate) fn render(registry: &Registry) -> String {
    let routes: Vec<Value> = registry.inventory().iter().map(route_json).collect();
    let fallbacks: Vec<Value> = registry.fallbacks().iter().map(fallback_json).collect();
    let document = json!({ "routes": routes, "fallbacks": fallbacks });
    serde_json::to_string_pretty(&document).unwrap_or_default()
}

fn route_json(info: &RouteInfo) -> Value {
    json!({
        "method": info.method,
        "path": info.path,
        "policy": policy_json(&info.policy),
        "contract": contract_json(&info.contract),
    })
}

fn fallback_json(info: &FallbackInfo) -> Value {
    json!({ "kind": info.kind, "status": info.status, "policy": policy_json(&info.policy) })
}

fn policy_json(policy: &Policy) -> Value {
    match (policy.public_reason(), policy.requirement()) {
        (Some(reason), _) => json!({ "access": "public", "reason": reason }),
        (None, Some(requirement)) => json!({
            "access": "protected",
            "rights": requirement.rights().iter().map(|right| right.as_str()).collect::<Vec<_>>(),
            "session": requirement.session().as_str(),
            "mfa": requirement.mfa().as_str(),
            "fresh_login_within_secs": requirement.fresh_login_within_secs(),
        }),
        (None, None) => json!({ "access": "denied" }),
    }
}

fn params_json(params: &[ParamSpec]) -> Vec<Value> {
    params
        .iter()
        .map(|param| json!({ "name": param.name, "type": param.kind.as_str() }))
        .collect()
}

fn contract_json(contract: &Contract) -> Value {
    let csrf = match contract.csrf_mode() {
        Csrf::Exempt(reason) => json!({ "mode": "exempt", "reason": reason }),
        other => json!({ "mode": other.as_str() }),
    };
    let limits = contract.limits();
    let mut value = json!({
        "path_params": params_json(contract.path_params()),
        "query_params": params_json(contract.query_params()),
        "request_body": contract.request_body_kind().as_str(),
        "response": contract.response().as_str(),
        "success_statuses": contract.success_statuses(),
        "csrf": csrf,
        "limits": {
            "max_body_bytes": limits.max_body_bytes,
            "timeout_ms": u64::try_from(limits.timeout.as_millis()).unwrap_or(u64::MAX),
        },
    });
    // The named shapes, the cookie and the cache rule are listed only where a contract
    // declares them.
    if let Some(object) = value.as_object_mut() {
        let shapes = [
            ("request_shape", contract.request_shape_name()),
            ("query_shape", contract.query_shape_name()),
            ("response_shape", contract.response_shape_name()),
        ];
        for (key, name) in shapes {
            if let Some(name) = name {
                object.insert(key.to_owned(), json!(name));
            }
        }
        if contract.session_cookie_is_set() {
            object.insert("sets_session_cookie".to_owned(), json!(true));
        }
        if contract.is_no_store() {
            object.insert("no_store".to_owned(), json!(true));
        }
    }
    value
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use server_domain::policy::{MfaLevel, Requirement, Right, SessionKind};
    use server_http::registry::{BodyKind, Limits, ParamType, ResponseKind};

    use super::*;

    fn parse(text: &str) -> Value {
        serde_json::from_str(text).unwrap()
    }

    #[test]
    fn application_inventory_lists_only_what_is_served() {
        let document = parse(&render(&registry::application()));
        let served: Vec<(&str, &str)> = document["routes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| {
                (
                    entry["method"].as_str().unwrap(),
                    entry["path"].as_str().unwrap(),
                )
            })
            .collect();
        assert_eq!(
            served,
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
        let protected: Vec<&str> = document["routes"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|entry| entry["policy"]["access"] == "protected")
            .map(|entry| entry["path"].as_str().unwrap())
            .collect();
        assert_eq!(
            protected,
            [
                "/api/v1/admin/auth",
                "/api/v1/admin/auth/invitation",
                "/api/v1/admin/auth/invitation",
                "/api/v1/admin/auth/invitation",
                "/api/v1/admin/token",
                "/api/v1/admin/token",
                "/api/v1/admin/token/acls",
                "/api/v1/admin/token/{id}",
                "/api/v1/admin/users",
                "/api/v1/admin/users/{id}",
                "/api/v1/admin/users/{id}",
                "/api/v1/auth/mfa",
                "/api/v1/auth/mfa/totp",
                "/api/v1/auth/mfa/webauthn/finish",
                "/api/v1/auth/mfa/webauthn/start",
                "/api/v1/collection",
                "/api/v1/collection",
                "/api/v1/collection/default",
                "/api/v1/collection/default/entry",
                "/api/v1/collection/default/entry",
                "/api/v1/collection/{id}",
                "/api/v1/collection/{id}",
                "/api/v1/collection/{id}/entry",
                "/api/v1/collection/{id}/entry",
                "/api/v1/companies/{id}",
                "/api/v1/emoji/{codepoint}",
                "/api/v1/games/{id}",
                "/api/v1/screenshots",
                "/api/v1/screenshots/game/{id}",
                "/api/v1/screenshots/game/{id}",
                "/api/v1/screenshots/{id}",
                "/api/v1/screenshots/{id}",
                "/api/v1/settings",
                "/api/v1/setup",
                "/api/v1/store",
                "/api/v1/store/featured",
                "/api/v1/store/tags",
                "/api/v1/tags/{id}",
                "/api/v1/user/auth",
                "/api/v1/user/client",
                "/api/v1/user/client/{id}",
                "/api/v1/user/mfa",
                "/api/v1/user/mfa/totp/finish",
                "/api/v1/user/mfa/totp/start",
                "/api/v1/user/mfa/webauthn",
                "/api/v1/user/mfa/webauthn/finish",
                "/api/v1/user/mfa/webauthn/start",
                "/api/v1/user/superlevel",
                "/api/v1/user/token",
                "/api/v1/user/token",
                "/api/v1/user/token/acls",
                "/api/v1/user/token/{id}",
                "/api/v1/user/{id}",
            ]
        );
        let fallbacks = document["fallbacks"].as_array().unwrap();
        let kinds: Vec<&str> = fallbacks
            .iter()
            .map(|entry| entry["kind"].as_str().unwrap())
            .collect();
        assert_eq!(kinds, ["unknown_path", "method_not_allowed"]);
        assert!(
            fallbacks
                .iter()
                .all(|entry| entry["policy"]["access"] == "public")
        );
    }

    #[test]
    fn route_entry_carries_policy_contract_and_limits() {
        let requirement = Requirement::authenticated()
            .require_rights(&[Right::new("emoji:read").unwrap()])
            .unwrap()
            .require_session(SessionKind::Interactive)
            .require_mfa(MfaLevel::Completed)
            .require_fresh_login_within(300)
            .unwrap();
        let contract = Contract::new(
            ResponseKind::Json,
            &[200, 201],
            Limits::new(4096, Duration::from_millis(2500)),
        )
        .path_param("id", ParamType::Uuid)
        .query_param("limit", ParamType::Integer)
        .request_body(BodyKind::Json)
        .csrf(Csrf::Required);
        let info = RouteInfo {
            method: "POST",
            path: "/api/v1/things/{id}".into(),
            policy: Policy::protected(requirement),
            contract,
        };
        assert_eq!(
            route_json(&info),
            json!({
                "method": "POST",
                "path": "/api/v1/things/{id}",
                "policy": {
                    "access": "protected",
                    "rights": ["emoji:read"],
                    "session": "interactive",
                    "mfa": "completed",
                    "fresh_login_within_secs": 300,
                },
                "contract": {
                    "path_params": [{ "name": "id", "type": "uuid" }],
                    "query_params": [{ "name": "limit", "type": "integer" }],
                    "request_body": "json",
                    "response": "json",
                    "success_statuses": [200, 201],
                    "csrf": { "mode": "required" },
                    "limits": { "max_body_bytes": 4096, "timeout_ms": 2500 },
                },
            })
        );
    }

    #[test]
    fn named_shapes_the_session_cookie_and_the_cache_rule_are_listed_only_where_declared() {
        let contract = Contract::new(
            ResponseKind::Json,
            &[200],
            Limits::new(4096, Duration::from_secs(1)),
        )
        .request_body(BodyKind::Json)
        .csrf(Csrf::Required)
        .request_shape::<server_http::api_types::contracts::auth::SigninRequest>()
        .query_shape::<server_http::api_types::contracts::auth::SignupQuery>()
        .response_shape::<server_http::api_types::contracts::auth::SigninResponse>()
        .sets_session_cookie()
        .no_store();
        let entry = contract_json(&contract);
        assert_eq!(entry["request_shape"], "SigninRequest");
        assert_eq!(entry["query_shape"], "SignupQuery");
        assert_eq!(entry["response_shape"], "SigninResponse");
        assert_eq!(entry["sets_session_cookie"], true);
        assert_eq!(entry["no_store"], true);
        let plain = contract_json(&Contract::new(
            ResponseKind::Json,
            &[200],
            Limits::new(0, Duration::from_secs(1)),
        ));
        for key in [
            "request_shape",
            "query_shape",
            "response_shape",
            "sets_session_cookie",
            "no_store",
        ] {
            assert!(plain.get(key).is_none(), "{key}");
        }
    }

    #[test]
    fn public_route_entry_shows_its_reason() {
        let info = RouteInfo {
            method: "GET",
            path: "/api/v1/ping".into(),
            policy: Policy::public("probe").unwrap(),
            contract: Contract::new(
                ResponseKind::Empty,
                &[204],
                Limits::new(0, Duration::from_secs(1)),
            ),
        };
        let entry = route_json(&info);
        assert_eq!(
            entry["policy"],
            json!({ "access": "public", "reason": "probe" })
        );
        assert_eq!(
            entry["contract"]["csrf"],
            json!({ "mode": "not_applicable" })
        );
    }
    #[test]
    fn binary_and_upload_inventory_names_keep_their_limits_and_shapes() {
        let document = parse(&render(&registry::application()));
        let routes = document["routes"].as_array().unwrap();
        let get = routes
            .iter()
            .find(|entry| entry["method"] == "GET" && entry["path"] == "/api/v1/object/{id}")
            .unwrap();
        assert_eq!(get["contract"]["response"], "binary-or-empty");
        assert_eq!(get["contract"]["response_shape"], "ObjectDownload");
        assert_eq!(get["contract"]["success_statuses"], json!([200, 304]));
        assert!(get["contract"].get("no_store").is_none());
        for path in ["/api/v1/object/{id}", "/api/v1/screenshots/game/{id}"] {
            let upload = routes
                .iter()
                .find(|entry| entry["method"] == "POST" && entry["path"] == path)
                .unwrap();
            assert_eq!(upload["contract"]["request_body"], "binary");
            assert_eq!(upload["contract"]["request_shape"], "BinaryUpload");
            assert_eq!(upload["contract"]["csrf"]["mode"], "required");
            assert_eq!(
                upload["contract"]["limits"]["max_body_bytes"],
                32 * 1024 * 1024
            );
            assert_eq!(upload["contract"]["no_store"], true);
        }
    }
}
