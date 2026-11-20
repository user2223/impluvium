// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

use super::*;
use crate::registry::{Csrf, ParamType};
use server_domain::policy::{Decision, SessionKind};
use server_domain::principal::Device;

/// Method, path, scoped right, request/body/answer/response, setup alternative, no-store.
type ExpectedAdmin = (
    &'static str,
    &'static str,
    &'static str,
    Option<&'static str>,
    &'static str,
    Option<&'static str>,
    &'static str,
    bool,
    bool,
);

const ADMIN_ROUTES: [ExpectedAdmin; 30] = [
    (
        "GET",
        "/api/v1/admin/game/{id}",
        "game:read",
        None,
        "none",
        Some("AdminGameResponse"),
        "json",
        false,
        true,
    ),
    (
        "PATCH",
        "/api/v1/admin/game/{id}",
        "game:update",
        Some("AdminGamePatch"),
        "json",
        Some("CatalogGame"),
        "json",
        false,
        true,
    ),
    (
        "DELETE",
        "/api/v1/admin/game/{id}",
        "game:delete",
        None,
        "none",
        Some("EmptyObject"),
        "json",
        false,
        true,
    ),
    (
        "PATCH",
        "/api/v1/admin/game/{id}/age-ratings",
        "game:update",
        Some("AdminAgeRatingsPatch"),
        "json",
        Some("AdminAgeRatings"),
        "json",
        false,
        true,
    ),
    (
        "POST",
        "/api/v1/admin/game/{id}/metadata",
        "game:update",
        Some("AdminGameMetadataForm"),
        "multipart",
        Some("CatalogGame"),
        "json",
        false,
        true,
    ),
    (
        "PATCH",
        "/api/v1/admin/game/{id}/tags",
        "game:update",
        Some("AdminGameTagsPatch"),
        "json",
        None,
        "empty",
        false,
        true,
    ),
    (
        "GET",
        "/api/v1/admin/game/{id}/versions",
        "game:read",
        None,
        "none",
        Some("AdminVersionList"),
        "json",
        false,
        true,
    ),
    (
        "PATCH",
        "/api/v1/admin/game/{id}/versions",
        "game:version:update",
        Some("AdminVersionOrder"),
        "json",
        Some("AdminVersionIds"),
        "json",
        false,
        true,
    ),
    (
        "DELETE",
        "/api/v1/admin/game/{id}/versions",
        "game:version:delete",
        Some("AdminVersionDelete"),
        "json",
        Some("EmptyObject"),
        "json",
        false,
        true,
    ),
    (
        "POST",
        "/api/v1/admin/game/image",
        "game:image:new",
        Some("AdminGameImageForm"),
        "multipart",
        Some("CatalogGame"),
        "json-or-empty",
        false,
        true,
    ),
    (
        "DELETE",
        "/api/v1/admin/game/image",
        "game:image:delete",
        Some("AdminGameImageDelete"),
        "json",
        Some("AdminGameImages"),
        "json-or-empty",
        false,
        true,
    ),
    (
        "GET",
        "/api/v1/admin/company",
        "company:read",
        None,
        "none",
        Some("AdminCompanyList"),
        "json",
        false,
        true,
    ),
    (
        "POST",
        "/api/v1/admin/company",
        "company:create",
        Some("AdminCompanyCreate"),
        "json",
        Some("CatalogCompany"),
        "json",
        false,
        true,
    ),
    (
        "GET",
        "/api/v1/admin/company/{id}",
        "company:read",
        None,
        "none",
        Some("AdminCompanyResponse"),
        "json",
        false,
        true,
    ),
    (
        "PATCH",
        "/api/v1/admin/company/{id}",
        "company:update",
        Some("AdminCompanyPatch"),
        "json",
        Some("CatalogCompany"),
        "json",
        false,
        true,
    ),
    (
        "DELETE",
        "/api/v1/admin/company/{id}",
        "company:delete",
        None,
        "none",
        None,
        "empty",
        false,
        true,
    ),
    (
        "POST",
        "/api/v1/admin/company/{id}/banner",
        "company:update",
        Some("AdminCompanyImageForm"),
        "multipart",
        Some("AdminCompanyImage"),
        "json",
        false,
        true,
    ),
    (
        "POST",
        "/api/v1/admin/company/{id}/icon",
        "company:update",
        Some("AdminCompanyImageForm"),
        "multipart",
        Some("AdminCompanyImage"),
        "json",
        false,
        true,
    ),
    (
        "POST",
        "/api/v1/admin/company/{id}/game",
        "company:update",
        Some("AdminCompanyGameCreate"),
        "json",
        Some("AdminCompanyGame"),
        "json",
        false,
        true,
    ),
    (
        "PATCH",
        "/api/v1/admin/company/{id}/game",
        "company:update",
        Some("AdminCompanyGamePatch"),
        "json",
        None,
        "empty",
        false,
        true,
    ),
    (
        "DELETE",
        "/api/v1/admin/company/{id}/game",
        "company:update",
        Some("AdminCompanyGameDelete"),
        "json",
        None,
        "empty",
        false,
        true,
    ),
    (
        "GET",
        "/api/v1/admin/tags",
        "tags:read",
        None,
        "none",
        Some("CatalogTags"),
        "json",
        false,
        false,
    ),
    (
        "POST",
        "/api/v1/admin/tags",
        "tags:read",
        Some("AdminTagCreate"),
        "json",
        Some("CatalogTag"),
        "json",
        false,
        false,
    ),
    (
        "DELETE",
        "/api/v1/admin/tags/{id}",
        "tags:delete",
        None,
        "none",
        None,
        "empty",
        false,
        false,
    ),
    (
        "GET",
        "/api/v1/admin/library",
        "library:read",
        None,
        "none",
        Some("AdminLibraryResponse"),
        "json",
        false,
        true,
    ),
    (
        "GET",
        "/api/v1/admin/library/libraries",
        "library:read",
        None,
        "none",
        Some("AdminLibrariesResponse"),
        "json",
        false,
        true,
    ),
    (
        "GET",
        "/api/v1/admin/library/sources",
        "library:sources:read",
        None,
        "none",
        Some("LibrarySourceList"),
        "json",
        true,
        true,
    ),
    (
        "POST",
        "/api/v1/admin/library/sources",
        "library:sources:new",
        Some("LibrarySourceCreate"),
        "json",
        Some("WorkingLibrarySource"),
        "json",
        true,
        true,
    ),
    (
        "PATCH",
        "/api/v1/admin/library/sources",
        "library:sources:update",
        Some("LibrarySourcePatch"),
        "json",
        Some("WorkingLibrarySource"),
        "json",
        true,
        true,
    ),
    (
        "DELETE",
        "/api/v1/admin/library/sources",
        "library:sources:delete",
        Some("LibrarySourceDelete"),
        "json",
        None,
        "empty",
        true,
        true,
    ),
];

#[test]
fn admin_catalog_contracts_register_exactly_thirty_keys() {
    let inventory = built().inventory();
    let actual = inventory
        .iter()
        .filter(|info| {
            [
                "/api/v1/admin/game/",
                "/api/v1/admin/company",
                "/api/v1/admin/tags",
                "/api/v1/admin/library",
            ]
            .iter()
            .any(|prefix| info.path.starts_with(prefix))
        })
        .count();
    assert_eq!(actual, ADMIN_ROUTES.len());
    for (method, path, name, request, body, answer, response, setup, no_store) in ADMIN_ROUTES {
        let info = inventory
            .iter()
            .find(|info| info.method == method && info.path == path)
            .unwrap();
        let contract = &info.contract;
        assert_eq!(contract.request_shape_name(), request, "{method} {path}");
        assert_eq!(contract.response_shape_name(), answer, "{method} {path}");
        assert_eq!(
            contract.request_body_kind().as_str(),
            body,
            "{method} {path}"
        );
        assert_eq!(contract.response().as_str(), response, "{method} {path}");
        assert_eq!(contract.is_no_store(), no_store, "{method} {path}");
        assert!(!contract.session_cookie_is_set());
        let statuses: &[u16] = match response {
            "empty" => &[204],
            "json-or-empty" => &[200, 204],
            _ => &[200],
        };
        assert_eq!(contract.success_statuses(), statuses);
        assert_eq!(
            contract.csrf_mode(),
            if method == "GET" {
                Csrf::NotApplicable
            } else {
                Csrf::Required
            }
        );
        assert_eq!(
            contract.limits().max_body_bytes,
            match body {
                "json" if method == "PATCH" && path == "/api/v1/admin/game/{id}" => {
                    super::super::GAME_PATCH_BODY_LIMIT
                }
                "json" => super::super::JSON_BODY_LIMIT,
                "multipart" => super::super::UPLOAD_BODY_LIMIT,
                _ => 0,
            }
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
        let query: Vec<&str> = contract
            .query_params()
            .iter()
            .map(|param| {
                assert_eq!(param.kind, ParamType::String);
                param.name
            })
            .collect();
        if path == "/api/v1/admin/library" {
            assert_eq!(
                query,
                ["query", "skip", "limit", "sort", "order", "filters"]
            );
            assert_eq!(contract.query_shape_name(), Some("AdminLibraryQuery"));
        } else {
            assert!(query.is_empty());
            assert_eq!(contract.query_shape_name(), None);
        }
        let requirement = info.policy.requirement().unwrap();
        assert_eq!(requirement.session(), SessionKind::Administration);
        assert_eq!(requirement.mfa().as_str(), "not_required");
        assert_eq!(requirement.fresh_login_within_secs(), None);
        let mut rights = vec![];
        if !setup {
            rights.push(Right::new(name).unwrap());
        }
        rights.sort();
        assert_eq!(requirement.rights(), rights);
        let mut alternatives = if setup {
            vec![Right::new(name).unwrap(), Right::new("setup:run").unwrap()]
        } else {
            vec![]
        };
        alternatives.sort();
        assert_eq!(requirement.any_rights(), alternatives);
    }
}

#[test]
fn admin_catalog_policy_matrix_preserves_system_scope_and_browser_access() {
    let inventory = built().inventory();
    for (method, path, name, _, _, _, _, setup, _) in ADMIN_ROUTES {
        let policy = &inventory
            .iter()
            .find(|info| info.method == method && info.path == path)
            .unwrap()
            .policy;
        let owner = role_rights(Role::Admin);
        let grant = RightSet::from_rights(&[Right::new(name).unwrap()]);
        let system =
            |rights: &RightSet| Principal::Token(Token::new(TokenKind::Service, rights, &owner));
        let setup_principal = table().0.get("setup").unwrap().clone();
        for (who, principal, allowed) in [
            (
                "user",
                session(Role::User, SessionStage::Authenticated),
                false,
            ),
            (
                "admin",
                session(Role::Admin, SessionStage::Authenticated),
                true,
            ),
            (
                "pending admin",
                session(Role::Admin, SessionStage::MfaPending),
                false,
            ),
            (
                "store admin",
                Principal::Session(Session::new(
                    Role::Admin,
                    SessionStage::Authenticated,
                    SessionClass::Store,
                    5,
                )),
                false,
            ),
            (
                "device admin",
                Principal::Device(Device::new(&grant, &owner)),
                false,
            ),
            (
                "personal admin token",
                Principal::Token(Token::new(TokenKind::Personal, &grant, &owner)),
                false,
            ),
            ("scoped system token", system(&grant), true),
            (
                "system token missing admin",
                system(&RightSet::from_rights(&[Right::new(name).unwrap()])),
                true,
            ),
            (
                "system token missing scope",
                system(&RightSet::from_rights(&[
                    Right::new("admin:manage").unwrap()
                ])),
                false,
            ),
            (
                "wrong scope",
                system(&RightSet::from_rights(&[
                    Right::new("admin:manage").unwrap(),
                    Right::new("settings:read").unwrap(),
                ])),
                false,
            ),
            ("setup system token", setup_principal, setup),
            ("empty token", system(&RightSet::empty()), false),
            ("anonymous", Principal::Anonymous, false),
        ] {
            let expected = if allowed {
                Decision::Allow
            } else if principal.is_anonymous() {
                Decision::Unauthenticated
            } else {
                Decision::Forbidden
            };
            assert_eq!(policy.decide(&principal), expected, "{method} {path} {who}");
        }
    }
}

#[test]
fn admin_catalog_router_gates_every_route_before_the_placeholder() {
    let router = router();
    for (method, path, _, _, _, _, _, setup, _) in ADMIN_ROUTES {
        for (who, credential, status) in [
            ("admin", Some(cookie("admin")), 501),
            ("scoped system", Some(bearer("tok-catalog-admin")), 501),
            ("personal admin", Some(bearer("tok-catalog-personal")), 403),
            ("device admin", Some(bearer("device-catalog-admin")), 403),
            (
                "setup system",
                Some(bearer("setup")),
                if setup { 501 } else { 403 },
            ),
            ("user", Some(cookie("user")), 403),
            ("pending", Some(cookie("admin-pending")), 403),
            ("store", Some(cookie("store")), 403),
            ("unscoped admin token", Some(bearer("tok-admin")), 403),
            ("anonymous", None, 401),
        ] {
            let reply = call(&router, method, path, credential);
            assert_eq!(reply.status.as_u16(), status, "{method} {path} {who}");
        }
    }
}

#[test]
fn scoped_system_and_setup_tokens_cannot_manage_accounts_or_invitations() {
    let router = router();
    for (method, path) in [
        ("GET", "/api/v1/admin/users/{id}"),
        ("DELETE", "/api/v1/admin/users/{id}"),
        ("POST", "/api/v1/admin/auth/invitation"),
    ] {
        assert_eq!(
            call(&router, method, path, Some(cookie("admin"))).status,
            StatusCode::NOT_IMPLEMENTED,
            "{method} {path} admin browser"
        );
        for credential in ["tok-game-read", "tok-catalog-admin", "setup"] {
            assert_eq!(
                call(&router, method, path, Some(bearer(credential))).status,
                StatusCode::FORBIDDEN,
                "{method} {path} {credential}"
            );
        }
    }
    assert_eq!(
        call(
            &router,
            "GET",
            "/api/v1/admin/game/{id}",
            Some(bearer("tok-game-read"))
        )
        .status,
        StatusCode::NOT_IMPLEMENTED
    );
}
