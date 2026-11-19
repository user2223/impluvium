// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use super::EmptyObject;
use super::admin_auth::{
    AdminAuthInfo, Invitation, InvitationCreate, InvitationDelete, InvitationList,
    OidcConfiguration, UsernameClaim,
};
use super::auth::{
    AuthInfo, AuthProvider, MfaMethod, MfaMethods, OidcCallbackQuery, OidcLogoutForm,
    OidcLogoutResponse, SigninRequest, SigninResponse, SignupAccount, SignupInvitation,
    SignupQuery, SignupRequest, TotpRequest, WebAuthnAssertion, WebAuthnRequestOptions,
};
use super::server::{GeneralSettings, ServerInfo, ServerSettings, StoreSettings};

/// Reads the body, checks the typed value writes back to exactly the same JSON, and returns it.
fn round_trip<T: Serialize + DeserializeOwned>(body: Value) -> T {
    let value: T = serde_json::from_value(body.clone()).unwrap();
    assert_eq!(serde_json::to_value(&value).unwrap(), body);
    value
}

#[test]
fn the_empty_object_is_braces() {
    let value: EmptyObject = round_trip(json!({}));
    assert_eq!(value, EmptyObject {});
    assert!(serde_json::from_value::<EmptyObject>(json!("x")).is_err());
}

#[test]
fn server_info_keeps_its_camel_case_names_and_the_null_logo() {
    for logo in [json!(null), json!("0b1f")] {
        let info: ServerInfo = round_trip(json!({
            "appName": "impluvium", "version": "0.1.0", "gitRef": "#abc",
            "external": "https://games.example.test", "serverName": "Games", "mLogoObjectId": logo,
        }));
        assert_eq!(info.m_logo_object_id.is_none(), logo.is_null());
    }
    // A logo field that is left out reads like `null`.
    let missing = json!({
        "appName": "a", "version": "v", "gitRef": "#g", "external": "e", "serverName": "s",
    });
    assert_eq!(
        serde_json::from_value::<ServerInfo>(missing)
            .unwrap()
            .m_logo_object_id,
        None
    );
}

#[test]
fn settings_round_trip() {
    let settings: ServerSettings = round_trip(json!({
        "generalSettings": { "serverName": "Games", "mLogoObjectId": null },
        "store": { "showGamePanelTextDecoration": true },
    }));
    assert_eq!(
        settings,
        ServerSettings {
            general_settings: GeneralSettings {
                server_name: "Games".into(),
                m_logo_object_id: None
            },
            store: StoreSettings {
                show_game_panel_text_decoration: true
            },
        }
    );
}

#[test]
fn admin_auth_info_has_the_method_names_as_keys_and_omits_a_missing_single_sign_on() {
    let off: AdminAuthInfo = round_trip(json!({ "Simple": true }));
    assert!(off.simple && off.open_id.is_none());
    let on: AdminAuthInfo = round_trip(json!({
        "Simple": false,
        "OpenID": {
            "authorizationUrl": "https://idp.example.test/auth", "scopes": "openid, profile",
            "usernameClaim": "preferred_username", "externalUrl": "https://games.example.test",
        },
    }));
    let configuration: &OidcConfiguration = on.open_id.as_ref().unwrap();
    assert_eq!(
        configuration.username_claim,
        UsernameClaim::PreferredUsername
    );
    assert_eq!(configuration.admin_group, None);
    let with_group: OidcConfiguration = round_trip(json!({
        "authorizationUrl": "u", "scopes": "s", "adminGroup": "admins",
        "usernameClaim": "sub", "externalUrl": "e",
    }));
    assert_eq!(with_group.admin_group.as_deref(), Some("admins"));
    assert!(
        serde_json::from_value::<OidcConfiguration>(json!({
            "authorizationUrl": "u", "scopes": "s", "usernameClaim": "nickname", "externalUrl": "e",
        }))
        .is_err()
    );
}

#[test]
fn invitations_are_listed_as_a_bare_array_with_canonical_timestamps() {
    let list: InvitationList = round_trip(json!([
        {
            "id": "i1", "isAdmin": true, "username": null, "email": null,
            "expires": "2030-01-02T03:04:05.006Z", "inviteUrl": "https://g.example.test/auth/register?id=i1",
        },
        {
            "id": "i2", "isAdmin": false, "username": "alice", "email": "a@example.test",
            "expires": "2030-01-02T03:04:05.006Z", "inviteUrl": "u",
        },
    ]));
    let first: &Invitation = &list.0[0];
    assert!(first.is_admin && first.username.is_none() && first.email.is_none());
    assert!(
        serde_json::from_value::<Invitation>(json!({
            "id": "i", "isAdmin": false, "username": null, "email": null,
            "expires": "2030-01-02T03:04:05+00:00", "inviteUrl": "u",
        }))
        .is_err()
    );
    let _: InvitationList = round_trip(json!([]));
}

#[test]
fn invitation_create_has_three_optional_fields_and_a_free_form_expiry() {
    let minimal: InvitationCreate =
        round_trip(json!({ "expires": "2030-01-02T04:04:05.006+01:00" }));
    assert!(minimal.username.is_none() && minimal.email.is_none() && minimal.is_admin.is_none());
    let full: InvitationCreate = round_trip(json!({
        "username": "alice", "email": "a@example.test", "expires": "e", "isAdmin": true,
    }));
    assert_eq!(full.is_admin, Some(true));
    // `null` reads as absent and is written back as absent.
    let null: InvitationCreate =
        serde_json::from_value(json!({ "expires": "e", "isAdmin": null, "email": null })).unwrap();
    assert!(null.is_admin.is_none() && null.email.is_none());
    assert_eq!(
        serde_json::to_value(&null).unwrap(),
        json!({ "expires": "e" })
    );
    assert!(serde_json::from_value::<InvitationCreate>(json!({})).is_err());
    let _: InvitationDelete = round_trip(json!({ "id": "i1" }));
    assert!(serde_json::from_value::<InvitationDelete>(json!({})).is_err());
}

#[test]
fn auth_info_lists_providers_by_their_names() {
    let info: AuthInfo = round_trip(json!({ "enabledAuthProviders": ["Simple", "OpenID"] }));
    assert_eq!(
        info.enabled_auth_providers,
        [AuthProvider::Simple, AuthProvider::OpenId]
    );
    assert_eq!(info.oidc_provider_name, None);
    let named: AuthInfo =
        round_trip(json!({ "enabledAuthProviders": ["OpenID"], "oidcProviderName": "Company" }));
    assert_eq!(named.oidc_provider_name.as_deref(), Some("Company"));
    assert!(
        serde_json::from_value::<AuthInfo>(json!({ "enabledAuthProviders": ["Other"] })).is_err()
    );
}

#[test]
fn second_factors_are_a_bare_array_and_the_code_a_string() {
    let methods: MfaMethods = round_trip(json!(["WebAuthn", "TOTP"]));
    assert_eq!(methods.0, [MfaMethod::WebAuthn, MfaMethod::Totp]);
    let _: MfaMethods = round_trip(json!([]));
    assert!(serde_json::from_value::<MfaMethods>(json!(["totp"])).is_err());
    let code: TotpRequest = round_trip(json!({ "code": "123456" }));
    assert_eq!(code.code, "123456");
    assert!(serde_json::from_value::<TotpRequest>(json!({ "code": 123456 })).is_err());
}

#[test]
fn passkey_options_round_trip_minimal_and_full() {
    let minimal: WebAuthnRequestOptions = round_trip(json!({ "challenge": "abc" }));
    assert!(minimal.allow_credentials.is_none() && minimal.timeout.is_none());
    let full: WebAuthnRequestOptions = round_trip(json!({
        "allowCredentials": [
            { "id": "c1", "type": "public-key" },
            { "id": "c2", "type": "public-key", "transports": ["usb", "smart-card", "hybrid"] },
        ],
        "challenge": "abc",
        "extensions": { "appid": "x", "credProps": true, "hmacCreateSecret": false, "minPinLength": true },
        "hints": ["security-key", "client-device", "hybrid"],
        "rpId": "games.example.test",
        "timeout": 60000,
        "userVerification": "preferred",
    }));
    assert_eq!(full.timeout, Some(60000));
    assert!(
        serde_json::from_value::<WebAuthnRequestOptions>(
            json!({ "challenge": "a", "userVerification": "always" })
        )
        .is_err()
    );
}

#[test]
fn passkey_answers_round_trip_with_their_exact_key_names() {
    let minimal: WebAuthnAssertion = round_trip(json!({
        "clientExtensionResults": {},
        "id": "c1", "rawId": "c1", "type": "public-key",
        "response": { "authenticatorData": "a", "clientDataJSON": "b", "signature": "c" },
    }));
    assert!(minimal.authenticator_attachment.is_none());
    assert!(minimal.response.user_handle.is_none());
    let full: WebAuthnAssertion = round_trip(json!({
        "authenticatorAttachment": "cross-platform",
        "clientExtensionResults": { "appid": true, "credProps": { "rk": false }, "hmacCreateSecret": true },
        "id": "c1", "rawId": "c1", "type": "public-key",
        "response": { "authenticatorData": "a", "clientDataJSON": "b", "signature": "c", "userHandle": "u" },
    }));
    assert_eq!(full.response.user_handle.as_deref(), Some("u"));
    // Fields of newer browsers are read and dropped.
    let extra: WebAuthnAssertion = serde_json::from_value(json!({
        "clientExtensionResults": { "largeBlob": {} }, "id": "c", "rawId": "c", "type": "public-key",
        "response": { "authenticatorData": "a", "clientDataJSON": "b", "signature": "c", "transports": [] },
    }))
    .unwrap();
    assert!(extra.client_extension_results.appid.is_none());
    assert!(
        serde_json::from_value::<WebAuthnAssertion>(json!({
            "clientExtensionResults": {}, "id": "c", "rawId": "c", "type": "password",
            "response": { "authenticatorData": "a", "clientDataJSON": "b", "signature": "c" },
        }))
        .is_err()
    );
}

#[test]
fn sign_in_round_trips_and_names_both_results() {
    let plain: SigninRequest = round_trip(json!({ "username": "alice", "password": "pw" }));
    assert!(plain.remember_me.is_none());
    let remembered: SigninRequest =
        round_trip(json!({ "username": "alice", "password": "pw", "rememberMe": false }));
    assert_eq!(remembered.remember_me, Some(false));
    for result in ["signin", "2fa"] {
        let response: SigninResponse = round_trip(json!({ "result": result, "userId": "u1" }));
        assert_eq!(response.user_id, "u1");
    }
    assert!(
        serde_json::from_value::<SigninResponse>(json!({ "result": "fail", "userId": "u" }))
            .is_err()
    );
}

#[test]
fn single_sign_on_bodies_round_trip() {
    let query: OidcCallbackQuery = round_trip(json!({ "code": "c", "state": "s" }));
    assert_eq!(query.state, "s");
    assert!(serde_json::from_value::<OidcCallbackQuery>(json!({ "code": "c" })).is_err());
    let form: OidcLogoutForm = round_trip(json!({ "logout_token": "t" }));
    assert_eq!(form.logout_token, "t");
    assert!(serde_json::from_value::<OidcLogoutForm>(json!({ "logoutToken": "t" })).is_err());
    let answer: OidcLogoutResponse = round_trip(json!({ "success": true }));
    assert!(answer.success);
}

#[test]
fn registration_bodies_round_trip() {
    let query: SignupQuery = round_trip(json!({ "id": "i1" }));
    assert_eq!(query.id, "i1");
    for (username, email) in [
        (json!(null), json!(null)),
        (json!("alice"), json!("a@example.test")),
    ] {
        let invitation: SignupInvitation = round_trip(json!({
            "id": "i1", "isAdmin": false, "username": username, "email": email,
            "expires": "2030-01-02T03:04:05.006Z",
        }));
        assert_eq!(invitation.username.is_none(), username.is_null());
    }
    let minimal: SignupRequest = round_trip(json!({
        "invitation": "i1", "username": "alice", "email": "a@example.test", "password": "password1",
    }));
    assert!(minimal.display_name.is_none());
    let named: SignupRequest = round_trip(json!({
        "invitation": "i1", "username": "alice", "email": "a@example.test",
        "password": "password1", "displayName": "Alice",
    }));
    assert_eq!(named.display_name.as_deref(), Some("Alice"));
    let account: SignupAccount = round_trip(json!({
        "id": "u1", "username": "alice", "admin": false, "enabled": true,
        "email": "a@example.test", "displayName": "Alice", "profilePictureObjectId": "o1",
    }));
    assert!(account.enabled);
}

/// The debug text of `value` shows `[redacted]` and none of the secrets.
fn assert_redacted(value: &impl std::fmt::Debug, secrets: &[&str]) {
    let text = format!("{value:?}");
    assert!(text.contains("[redacted]"), "{text}");
    for secret in secrets {
        assert!(!text.contains(secret), "{text}");
    }
}

#[test]
fn debug_hides_the_totp_code() {
    assert_redacted(
        &TotpRequest {
            code: "123456".into(),
        },
        &["123456"],
    );
}

#[test]
fn debug_hides_the_single_sign_on_secrets() {
    let query = OidcCallbackQuery {
        code: "code-s3cret".into(),
        state: "state-s3cret".into(),
    };
    assert_redacted(&query, &["code-s3cret", "state-s3cret"]);
    let form = OidcLogoutForm {
        logout_token: "token-s3cret".into(),
    };
    assert_redacted(&form, &["token-s3cret"]);
}

#[test]
fn debug_hides_the_sign_in_password_but_not_the_user_name() {
    let request = SigninRequest {
        username: "alice".into(),
        password: "pw-s3cret".into(),
        remember_me: Some(true),
    };
    assert_redacted(&request, &["pw-s3cret"]);
    assert!(format!("{request:?}").contains("alice"));
}

#[test]
fn debug_hides_the_registration_secrets() {
    let request = SignupRequest {
        invitation: "inv-s3cret".into(),
        username: "alice".into(),
        email: "a@example.test".into(),
        password: "pw-s3cret".into(),
        display_name: None,
    };
    assert_redacted(&request, &["inv-s3cret", "pw-s3cret"]);
    assert_redacted(
        &SignupQuery {
            id: "inv-s3cret".into(),
        },
        &["inv-s3cret"],
    );
    let invitation: SignupInvitation = serde_json::from_value(json!({
        "id": "inv-s3cret", "isAdmin": false, "username": null, "email": null,
        "expires": "2030-01-02T03:04:05.006Z",
    }))
    .unwrap();
    assert_redacted(&invitation, &["inv-s3cret"]);
}

#[test]
fn debug_hides_the_invitation_id() {
    let invitation: Invitation = serde_json::from_value(json!({
        "id": "inv-s3cret", "isAdmin": true, "username": null, "email": null,
        "expires": "2030-01-02T03:04:05.006Z",
        "inviteUrl": "https://g.example.test/auth/register?id=inv-s3cret",
    }))
    .unwrap();
    assert_redacted(&invitation, &["inv-s3cret"]);
    assert_redacted(
        &InvitationDelete {
            id: "inv-s3cret".into(),
        },
        &["inv-s3cret"],
    );
}
