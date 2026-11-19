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
    // The named shapes and the cookie are listed only where a contract declares them.
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
                "/api/v1/auth/mfa",
                "/api/v1/auth/mfa/totp",
                "/api/v1/auth/mfa/webauthn/finish",
                "/api/v1/auth/mfa/webauthn/start",
                "/api/v1/emoji/{codepoint}",
                "/api/v1/settings",
                "/api/v1/setup",
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
    fn named_shapes_and_the_session_cookie_are_listed_only_where_declared() {
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
        .sets_session_cookie();
        let entry = contract_json(&contract);
        assert_eq!(entry["request_shape"], "SigninRequest");
        assert_eq!(entry["query_shape"], "SignupQuery");
        assert_eq!(entry["response_shape"], "SigninResponse");
        assert_eq!(entry["sets_session_cookie"], true);
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
}
