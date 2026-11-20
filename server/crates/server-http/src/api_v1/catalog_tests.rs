// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

use super::*;
use crate::registry::{Csrf, ParamType, PathTemplate};
use server_domain::permissions::store_session_rights;
use server_domain::policy::Decision;

/// Method, path, right (empty for per-object authorization), response/body kinds, named shapes,
/// forced no-store. The object GET also serves HEAD without adding a separate registry key.
type CatalogExpected = (
    &'static str,
    &'static str,
    &'static str,
    &'static str,
    &'static str,
    Option<&'static str>,
    Option<&'static str>,
    Option<&'static str>,
    bool,
);

const CATALOG_ROUTES: [CatalogExpected; 23] = [
    (
        "GET",
        "/api/v1/store",
        "store:read",
        "json",
        "none",
        None,
        Some("StoreQuery"),
        Some("StoreResponse"),
        true,
    ),
    (
        "GET",
        "/api/v1/store/featured",
        "store:read",
        "json",
        "none",
        None,
        None,
        Some("FeaturedGames"),
        true,
    ),
    (
        "GET",
        "/api/v1/store/tags",
        "store:read",
        "json",
        "none",
        None,
        None,
        Some("CatalogTags"),
        false,
    ),
    (
        "GET",
        "/api/v1/games/{id}",
        "store:read",
        "json",
        "none",
        None,
        None,
        Some("GameResponse"),
        true,
    ),
    (
        "GET",
        "/api/v1/companies/{id}",
        "store:read",
        "json",
        "none",
        None,
        None,
        Some("CompanyResponse"),
        false,
    ),
    (
        "GET",
        "/api/v1/tags/{id}",
        "store:read",
        "json",
        "none",
        None,
        None,
        Some("TagResponse"),
        false,
    ),
    (
        "GET",
        "/api/v1/collection",
        "collections:read",
        "json",
        "none",
        None,
        None,
        Some("CollectionList"),
        true,
    ),
    (
        "POST",
        "/api/v1/collection",
        "collections:new",
        "json",
        "json",
        Some("CollectionCreate"),
        None,
        Some("Collection"),
        true,
    ),
    (
        "GET",
        "/api/v1/collection/default",
        "collections:read",
        "json",
        "none",
        None,
        None,
        Some("DefaultCollection"),
        true,
    ),
    (
        "POST",
        "/api/v1/collection/default/entry",
        "library:add",
        "json",
        "json",
        Some("CollectionGameId"),
        None,
        Some("EmptyObject"),
        true,
    ),
    (
        "DELETE",
        "/api/v1/collection/default/entry",
        "library:remove",
        "json",
        "json",
        Some("CollectionGameId"),
        None,
        Some("EmptyObject"),
        true,
    ),
    (
        "GET",
        "/api/v1/collection/{id}",
        "collections:read",
        "json",
        "none",
        None,
        None,
        Some("Collection"),
        true,
    ),
    (
        "DELETE",
        "/api/v1/collection/{id}",
        "collections:delete",
        "json",
        "none",
        None,
        None,
        Some("CollectionDeleted"),
        true,
    ),
    (
        "POST",
        "/api/v1/collection/{id}/entry",
        "collections:add",
        "json",
        "json",
        Some("CollectionGameId"),
        None,
        Some("CollectionEntry"),
        true,
    ),
    (
        "DELETE",
        "/api/v1/collection/{id}/entry",
        "collections:remove",
        "json",
        "json",
        Some("CollectionGameId"),
        None,
        Some("EmptyObject"),
        true,
    ),
    (
        "GET",
        "/api/v1/object/{id}",
        "",
        "binary-or-empty",
        "none",
        None,
        None,
        Some("ObjectDownload"),
        false,
    ),
    (
        "POST",
        "/api/v1/object/{id}",
        "object:update",
        "json",
        "binary",
        Some("BinaryUpload"),
        None,
        Some("ObjectMutation"),
        true,
    ),
    (
        "DELETE",
        "/api/v1/object/{id}",
        "object:delete",
        "json",
        "none",
        None,
        None,
        Some("ObjectMutation"),
        true,
    ),
    (
        "GET",
        "/api/v1/screenshots",
        "screenshots:read",
        "json",
        "none",
        None,
        None,
        Some("ScreenshotList"),
        true,
    ),
    (
        "GET",
        "/api/v1/screenshots/{id}",
        "screenshots:read",
        "json",
        "none",
        None,
        None,
        Some("Screenshot"),
        true,
    ),
    (
        "DELETE",
        "/api/v1/screenshots/{id}",
        "screenshots:delete",
        "empty",
        "none",
        None,
        None,
        None,
        true,
    ),
    (
        "GET",
        "/api/v1/screenshots/game/{id}",
        "screenshots:read",
        "json",
        "none",
        None,
        None,
        Some("ScreenshotList"),
        true,
    ),
    (
        "POST",
        "/api/v1/screenshots/game/{id}",
        "screenshots:new",
        "empty",
        "binary",
        Some("BinaryUpload"),
        None,
        None,
        true,
    ),
];

#[test]
fn catalog_library_object_and_screenshot_contracts_are_registered() {
    let inventory = built().inventory();
    for (method, path, right, response, body, request, query, answer, no_store) in CATALOG_ROUTES {
        let info = inventory
            .iter()
            .find(|info| info.method == method && info.path == path)
            .unwrap();
        let contract = &info.contract;
        assert_eq!(contract.response().as_str(), response, "{method} {path}");
        assert_eq!(
            contract.request_body_kind().as_str(),
            body,
            "{method} {path}"
        );
        assert_eq!(contract.request_shape_name(), request, "{method} {path}");
        assert_eq!(contract.query_shape_name(), query, "{method} {path}");
        assert_eq!(contract.response_shape_name(), answer, "{method} {path}");
        assert_eq!(contract.is_no_store(), no_store, "{method} {path}");
        assert!(!contract.session_cookie_is_set(), "{method} {path}");
        let statuses: &[u16] = if response == "empty" {
            &[204]
        } else if response == "binary-or-empty" {
            &[200, 304]
        } else {
            &[200]
        };
        assert_eq!(contract.success_statuses(), statuses, "{method} {path}");
        assert_eq!(
            contract.csrf_mode(),
            if method == "GET" {
                Csrf::NotApplicable
            } else {
                Csrf::Required
            },
            "{method} {path}"
        );
        let expected_limit = match body {
            "json" => super::super::JSON_BODY_LIMIT,
            "binary" => super::super::UPLOAD_BODY_LIMIT,
            _ => 0,
        };
        assert_eq!(
            contract.limits().max_body_bytes,
            expected_limit,
            "{method} {path}"
        );
        assert_eq!(
            contract.limits().timeout,
            std::time::Duration::from_secs(10)
        );
        if path.contains("{id}") {
            assert_eq!(contract.path_params().len(), 1);
            assert_eq!(contract.path_params()[0].name, "id");
            assert_eq!(contract.path_params()[0].kind, ParamType::String);
        } else {
            assert!(contract.path_params().is_empty());
        }
        if right.is_empty() {
            assert!(info.policy.public_reason().is_some());
        } else {
            let requirement = info.policy.requirement().unwrap();
            assert_eq!(requirement.rights(), [Right::new(right).unwrap()]);
            assert_eq!(requirement.session().as_str(), "any");
            assert_eq!(requirement.mfa().as_str(), "not_required");
            assert_eq!(requirement.fresh_login_within_secs(), None);
        }
        let queries: Vec<(&str, ParamType)> = contract
            .query_params()
            .iter()
            .map(|param| (param.name, param.kind))
            .collect();
        let expected_queries = if path == "/api/v1/store" {
            vec![
                "skip",
                "take",
                "tags",
                "platform",
                "company",
                "companyActions",
                "sort",
                "order",
            ]
        } else {
            vec![]
        };
        assert_eq!(
            queries,
            expected_queries
                .into_iter()
                .map(|name| (name, ParamType::String))
                .collect::<Vec<_>>(),
            "{method} {path}"
        );
    }
}

#[test]
fn catalog_policy_matrix_preserves_browser_store_and_token_access() {
    let inventory = built().inventory();
    for (method, path, name, _, _, _, _, _, _) in CATALOG_ROUTES {
        let policy = &inventory
            .iter()
            .find(|info| info.method == method && info.path == path)
            .unwrap()
            .policy;
        let right = if name.is_empty() { "object:read" } else { name };
        let owner = role_rights(Role::User);
        let granted = RightSet::from_rights(&[Right::new(right).unwrap()]);
        let browser = session(Role::User, SessionStage::Authenticated);
        let admin = session(Role::Admin, SessionStage::Authenticated);
        let pending = session(Role::User, SessionStage::MfaPending);
        let store = Principal::Session(Session::new(
            Role::User,
            SessionStage::Authenticated,
            SessionClass::Store,
            5,
        ));
        let personal = Principal::Token(Token::new(TokenKind::Personal, &granted, &owner));
        let service = Principal::Token(Token::new(TokenKind::Service, &granted, &owner));
        let wrong = token(&["settings:read"], owner.clone());
        let empty = token(&[], owner);
        for (who, principal, allowed) in [
            ("user", browser, true),
            ("admin", admin, true),
            ("pending", pending, false),
            (
                "store",
                store,
                store_session_rights().contains(Right::new(right).unwrap()),
            ),
            (
                "device",
                Principal::Device(server_domain::principal::Device::new(
                    &granted,
                    &role_rights(Role::User),
                )),
                true,
            ),
            ("personal with right", personal, true),
            // Policy admits a right; the later handler must reject a service token's absent account.
            ("service with right", service, true),
            ("wrong right", wrong, false),
            ("empty token", empty, false),
            ("anonymous", Principal::Anonymous, false),
        ] {
            let expected = if name.is_empty() || allowed {
                Decision::Allow
            } else if principal.is_anonymous() {
                Decision::Unauthenticated
            } else {
                Decision::Forbidden
            };
            assert_eq!(policy.decide(&principal), expected, "{method} {path} {who}");
        }
    }
    // Exercise the actual policy gate as well as its pure decision.
    let router = router();
    for (method, path, right, _, _, _, _, _, _) in CATALOG_ROUTES {
        for (who, credential, expected) in [
            ("user", cookie("user"), 501),
            ("admin", cookie("admin"), 501),
            (
                "pending",
                cookie("pending"),
                if right.is_empty() { 501 } else { 403 },
            ),
            (
                "store",
                cookie("store"),
                if right.is_empty() || store_session_rights().contains(Right::new(right).unwrap()) {
                    501
                } else {
                    403
                },
            ),
            (
                "unrelated token",
                bearer("tok-settings"),
                if right.is_empty() { 501 } else { 403 },
            ),
        ] {
            assert_eq!(
                call(&router, method, path, Some(credential))
                    .status
                    .as_u16(),
                expected,
                "{method} {path} {who}"
            );
        }
    }
}

#[test]
fn literal_default_collection_paths_win_over_collection_ids() {
    for (literal, parameter) in [
        ("/api/v1/collection/default", "/api/v1/collection/{id}"),
        (
            "/api/v1/collection/default/entry",
            "/api/v1/collection/{id}/entry",
        ),
    ] {
        let literal = PathTemplate::parse(literal).unwrap();
        let parameter = PathTemplate::parse(parameter).unwrap();
        assert!(!literal.conflicts_with(&parameter));
        assert!(!parameter.conflicts_with(&literal));
    }
    let router = router();
    for (method, path, credential, status) in [
        (
            "POST",
            "/api/v1/collection/default/entry",
            "tok-library-add",
            501,
        ),
        (
            "POST",
            "/api/v1/collection/{id}/entry",
            "tok-library-add",
            403,
        ),
        (
            "POST",
            "/api/v1/collection/default/entry",
            "tok-collection-add",
            403,
        ),
        (
            "POST",
            "/api/v1/collection/{id}/entry",
            "tok-collection-add",
            501,
        ),
        (
            "DELETE",
            "/api/v1/collection/default/entry",
            "tok-library-remove",
            501,
        ),
        (
            "DELETE",
            "/api/v1/collection/{id}/entry",
            "tok-library-remove",
            403,
        ),
        ("DELETE", "/api/v1/collection/default", "user", 405),
        ("DELETE", "/api/v1/collection/{id}", "user", 501),
    ] {
        let credential = if credential == "user" {
            cookie(credential)
        } else {
            bearer(credential)
        };
        assert_eq!(
            call(&router, method, path, Some(credential))
                .status
                .as_u16(),
            status,
            "{method} {path}"
        );
    }
    assert_eq!(
        call(
            &router,
            "GET",
            "/api/v1/collection/default",
            Some(cookie("store"))
        )
        .status,
        StatusCode::NOT_IMPLEMENTED
    );
}

#[test]
fn object_head_uses_the_get_policy_and_has_no_response_body() {
    let router = router();
    for credential in [
        None,
        Some(cookie("user")),
        Some(cookie("pending")),
        Some(cookie("store")),
        Some(bearer("tok-settings")),
    ] {
        let reply = call(&router, "HEAD", "/api/v1/object/{id}", credential);
        assert_eq!(reply.status, StatusCode::NOT_IMPLEMENTED);
        assert!(reply.body.is_empty());
        assert_eq!(cache_control(&reply), Some("no-store"));
    }
    let inventory = built().inventory();
    assert_eq!(
        inventory
            .iter()
            .filter(|info| info.path == "/api/v1/object/{id}")
            .count(),
        3
    );
}

#[test]
fn object_mutations_reject_anonymous_and_unscoped_callers_at_the_gate() {
    let router = router();
    for method in ["POST", "DELETE"] {
        for (credential, status) in [
            (None, StatusCode::UNAUTHORIZED),
            (Some(cookie("pending")), StatusCode::FORBIDDEN),
            (Some(cookie("store")), StatusCode::FORBIDDEN),
            (Some(bearer("tok-settings")), StatusCode::FORBIDDEN),
            (Some(cookie("user")), StatusCode::NOT_IMPLEMENTED),
        ] {
            assert_eq!(
                call(&router, method, "/api/v1/object/{id}", credential).status,
                status
            );
        }
    }
}
