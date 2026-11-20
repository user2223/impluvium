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
use super::admin_users::{AdminUser, AdminUserList, UserDeleted};
use super::auth::{
    AuthInfo, AuthProvider, MfaMethod, MfaMethods, OidcCallbackQuery, OidcLogoutForm,
    OidcLogoutResponse, SigninRequest, SigninResponse, SignupAccount, SignupInvitation,
    SignupQuery, SignupRequest, TotpRequest, WebAuthnAssertion, WebAuthnRequestOptions,
};
use super::server::{GeneralSettings, ServerInfo, ServerSettings, StoreSettings};
use super::token::{
    AclDescriptions, ApiTokenList, GrantedAcls, NewApiToken, TokenCreate, TokenMode,
};
use super::user::{
    ClientCapability, ClientList, CurrentUser, MfaCredentials, MfaStatus, PasskeyDelete,
    PasskeyName, SuperlevelStatus, TotpSetup, UserAccount, UserAuth, UserProfile,
    WebAuthnCreationOptions, WebAuthnRegistration,
};

/// Reads the body, checks the typed value writes back to exactly the same JSON, and returns it.
pub(super) fn round_trip<T: Serialize + DeserializeOwned>(body: Value) -> T {
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

const STAMP: &str = "2030-01-02T03:04:05.006Z";

fn account() -> Value {
    json!({
        "admin": false, "displayName": "Alice", "email": "a@example.test", "enabled": true,
        "id": "u1", "profilePictureObjectId": "o1", "username": "alice",
    })
}

#[test]
fn the_current_user_is_an_account_or_null() {
    let none: CurrentUser = round_trip(json!(null));
    assert_eq!(none.0, None);
    let some: CurrentUser = round_trip(account());
    assert_eq!(
        some.0.as_ref().map(|user| user.username.as_str()),
        Some("alice")
    );
    assert!(serde_json::from_value::<CurrentUser>(json!({})).is_err());
    assert!(serde_json::from_value::<CurrentUser>(json!("alice")).is_err());
}

#[test]
fn accounts_and_profiles_round_trip_with_camel_case_names() {
    let user: UserAccount = round_trip(account());
    assert!(user.enabled && !user.admin);
    let profile: UserProfile = round_trip(json!({
        "displayName": "Alice", "id": "u1", "profilePictureObjectId": "o1", "username": "alice",
    }));
    assert_eq!(profile.display_name, "Alice");
    // A profile does not carry the email address.
    assert!(
        serde_json::from_value::<UserAccount>(json!({
            "displayName": "Alice", "id": "u1", "profilePictureObjectId": "o1", "username": "alice",
        }))
        .is_err()
    );
    let listed: AdminUserList = round_trip(json!([
        {
            "admin": true, "authMecs": [{ "mec": "Simple" }, { "mec": "OpenID" }],
            "displayName": "Alice", "email": "a@example.test", "enabled": true, "id": "u1",
            "profilePictureObjectId": "o1", "username": "alice",
        },
    ]));
    let first: &AdminUser = &listed.0[0];
    assert_eq!(first.auth_mecs.len(), 2);
    let _: AdminUserList = round_trip(json!([]));
    assert!(
        serde_json::from_value::<AdminUserList>(json!([{
            "admin": true, "authMecs": [{ "mec": "Other" }], "displayName": "A", "email": "e",
            "enabled": true, "id": "u", "profilePictureObjectId": "o", "username": "a",
        }]))
        .is_err()
    );
    let deleted: UserDeleted = round_trip(json!({ "success": true }));
    assert!(deleted.success);
}

#[test]
fn linked_sign_in_methods_have_the_method_names_as_keys() {
    let none: UserAuth = round_trip(json!({ "available": ["Simple", "OpenID"], "mecs": {} }));
    assert!(none.mecs.simple.is_none() && none.mecs.open_id.is_none());
    let both: UserAuth = round_trip(json!({
        "available": ["Simple", "OpenID"],
        "mecs": {
            "Simple": { "enabled": true, "mec": "Simple", "userId": "u1", "version": 1 },
            "OpenID": { "enabled": false, "mec": "OpenID", "userId": "u1", "version": 1 },
        },
    }));
    assert!(both.mecs.simple.unwrap().enabled);
    assert!(!both.mecs.open_id.unwrap().enabled);
    // The stored credentials are never part of the answer; a field with them is read and dropped.
    let extra: UserAuth = serde_json::from_value(json!({
        "available": [],
        "mecs": { "Simple": { "enabled": true, "mec": "Simple", "userId": "u", "version": 1, "credentials": {} } },
    }))
    .unwrap();
    assert!(
        serde_json::to_value(&extra).unwrap()["mecs"]["Simple"]
            .get("credentials")
            .is_none()
    );
    assert!(
        serde_json::from_value::<UserAuth>(json!({
            "available": [], "mecs": { "Other": {} },
        }))
        .unwrap()
        .mecs
        .simple
        .is_none()
    );
}

#[test]
fn clients_are_a_bare_array_with_canonical_timestamps() {
    let list: ClientList = round_trip(json!([
        {
            "capabilities": ["PeerAPI", "UserStatus", "CloudSaves", "TrackPlaytime"],
            "id": "c1", "lastConnected": STAMP, "name": "Desktop", "platform": "macOS", "userId": "u1",
        },
        {
            "capabilities": [], "id": "c2", "lastConnected": STAMP, "name": "Deck",
            "platform": "Linux", "userId": "u1",
        },
    ]));
    assert_eq!(list.0[0].capabilities[0], ClientCapability::PeerAPI);
    assert!(
        serde_json::from_value::<ClientList>(json!([{
            "capabilities": [], "id": "c", "lastConnected": "2030-01-02T03:04:05+00:00",
            "name": "n", "platform": "Linux", "userId": "u",
        }]))
        .is_err()
    );
    assert!(
        serde_json::from_value::<ClientList>(json!([{
            "capabilities": ["Other"], "id": "c", "lastConnected": STAMP,
            "name": "n", "platform": "Linux", "userId": "u",
        }]))
        .is_err()
    );
}

#[test]
fn second_factors_list_no_secret_and_the_credentials_follow_the_kind() {
    let none: MfaStatus = round_trip(json!({ "available": ["WebAuthn", "TOTP"], "mecs": {} }));
    assert!(none.mecs.totp.is_none() && none.mecs.web_authn.is_none());
    let both: MfaStatus = round_trip(json!({
        "available": ["WebAuthn", "TOTP"],
        "mecs": {
            "TOTP": { "credentials": {}, "enabled": false, "mec": "TOTP", "userId": "u1", "version": 1 },
            "WebAuthn": {
                "credentials": [
                    { "created": STAMP, "id": "k1", "name": "Laptop" },
                    { "created": STAMP, "id": "k2", "name": "Phone" },
                ],
                "enabled": true, "mec": "WebAuthn", "userId": "u1", "version": 1,
            },
        },
    }));
    assert!(matches!(
        both.mecs.totp.unwrap().credentials,
        MfaCredentials::Totp(_)
    ));
    match both.mecs.web_authn.unwrap().credentials {
        MfaCredentials::Passkeys(keys) => assert_eq!(keys.len(), 2),
        other => panic!("{other:?}"),
    }
    let empty: MfaStatus = round_trip(json!({
        "available": [],
        "mecs": { "WebAuthn": {
            "credentials": [], "enabled": true, "mec": "WebAuthn", "userId": "u", "version": 1,
        } },
    }));
    assert!(matches!(
        empty.mecs.web_authn.unwrap().credentials,
        MfaCredentials::Passkeys(keys) if keys.is_empty()
    ));
    // A stored public key is not a listed passkey field and is dropped.
    let listed: MfaStatus = serde_json::from_value(json!({
        "available": [],
        "mecs": { "WebAuthn": {
            "credentials": [{ "created": STAMP, "id": "k", "name": "n", "publicKey": "secret" }],
            "enabled": true, "mec": "WebAuthn", "userId": "u", "version": 1,
        } },
    }))
    .unwrap();
    assert!(!serde_json::to_string(&listed).unwrap().contains("secret"));
}

#[test]
fn the_totp_setup_has_the_secret_and_the_address() {
    let setup: TotpSetup = round_trip(json!({
        "secret": "JBSWY3DPEHPK3PXP",
        "url": "otpauth://totp/impluvium:u1?secret=JBSWY3DPEHPK3PXP&issuer=impluvium",
    }));
    assert_eq!(setup.secret, "JBSWY3DPEHPK3PXP");
    assert!(serde_json::from_value::<TotpSetup>(json!({ "secret": "x" })).is_err());
}

#[test]
fn passkey_creation_options_round_trip_minimal_and_full() {
    let name: PasskeyName = round_trip(json!({ "name": "Laptop" }));
    assert_eq!(name.name, "Laptop");
    assert!(serde_json::from_value::<PasskeyName>(json!({})).is_err());
    let delete: PasskeyDelete = round_trip(json!({ "id": "k1" }));
    assert_eq!(delete.id, "k1");
    let minimal: WebAuthnCreationOptions = round_trip(json!({
        "challenge": "abc",
        "pubKeyCredParams": [{ "alg": -7, "type": "public-key" }, { "alg": -257, "type": "public-key" }],
        "rp": { "name": "impluvium" },
        "user": { "displayName": "Alice", "id": "dTE", "name": "alice" },
    }));
    assert_eq!(minimal.pub_key_cred_params[0].alg, -7);
    assert!(minimal.rp.id.is_none() && minimal.attestation.is_none());
    let full: WebAuthnCreationOptions = round_trip(json!({
        "attestation": "none",
        "attestationFormats": ["fido-u2f", "android-safetynet", "android-key", "tpm", "apple", "packed", "none"],
        "authenticatorSelection": {
            "authenticatorAttachment": "platform", "requireResidentKey": true,
            "residentKey": "required", "userVerification": "preferred",
        },
        "challenge": "abc",
        "excludeCredentials": [{ "id": "k1", "type": "public-key", "transports": ["usb"] }],
        "extensions": { "credProps": true },
        "hints": ["security-key"],
        "pubKeyCredParams": [{ "alg": -7, "type": "public-key" }],
        "rp": { "id": "games.example.test", "name": "impluvium" },
        "timeout": 60000,
        "user": { "displayName": "Alice", "id": "dTE", "name": "alice" },
    }));
    assert_eq!(full.timeout, Some(60000));
    assert!(
        serde_json::from_value::<WebAuthnCreationOptions>(json!({
            "attestation": "always", "challenge": "a", "pubKeyCredParams": [],
            "rp": { "name": "n" }, "user": { "displayName": "d", "id": "i", "name": "n" },
        }))
        .is_err()
    );
}

#[test]
fn passkey_registration_round_trips_and_ignores_unknown_fields() {
    let minimal: WebAuthnRegistration = round_trip(json!({
        "clientExtensionResults": {}, "id": "k1", "rawId": "k1", "type": "public-key",
        "response": { "attestationObject": "a", "clientDataJSON": "b" },
    }));
    assert!(minimal.response.transports.is_none() && minimal.response.public_key.is_none());
    let full: WebAuthnRegistration = round_trip(json!({
        "authenticatorAttachment": "cross-platform",
        "clientExtensionResults": { "credProps": { "rk": true } },
        "id": "k1", "rawId": "k1", "type": "public-key",
        "response": {
            "attestationObject": "a", "authenticatorData": "d", "clientDataJSON": "b",
            "publicKey": "p", "publicKeyAlgorithm": -7, "transports": ["usb", "hybrid"],
        },
    }));
    assert_eq!(full.response.public_key_algorithm, Some(-7));
    let unknown: WebAuthnRegistration = round_trip(json!({
        "clientExtensionResults": {}, "id": "k1", "rawId": "k1", "type": "public-key",
        "response": {
            "attestationObject": "a", "clientDataJSON": "b",
            "transports": ["usb", "some-future-transport"],
        },
    }));
    assert_eq!(
        unknown.response.transports,
        Some(vec!["usb".to_owned(), "some-future-transport".to_owned()])
    );
    let extra: WebAuthnRegistration = serde_json::from_value(json!({
        "clientExtensionResults": {}, "id": "k", "rawId": "k", "type": "public-key",
        "response": { "attestationObject": "a", "clientDataJSON": "b", "other": 1 }, "extra": true,
    }))
    .unwrap();
    assert!(!serde_json::to_string(&extra).unwrap().contains("extra"));
    assert!(
        serde_json::from_value::<WebAuthnRegistration>(json!({
            "clientExtensionResults": {}, "id": "k", "rawId": "k", "type": "public-key",
            "response": { "clientDataJSON": "b" },
        }))
        .is_err()
    );
}

#[test]
fn the_superlevel_check_is_a_bare_boolean() {
    let on: SuperlevelStatus = round_trip(json!(true));
    let off: SuperlevelStatus = round_trip(json!(false));
    assert!(on.0 && !off.0);
    assert!(serde_json::from_value::<SuperlevelStatus>(json!("true")).is_err());
}

#[test]
fn tokens_are_listed_without_their_value_and_with_canonical_timestamps() {
    let list: ApiTokenList = round_trip(json!([
        {
            "acls": ["read", "clients:read"], "clientId": null, "expiresAt": null,
            "id": "t1", "mode": "User", "name": "Backup", "userId": "u1",
        },
        {
            "acls": ["user:read"], "clientId": null, "expiresAt": STAMP,
            "id": "t2", "mode": "System", "name": "Monitoring", "userId": null,
        },
        {
            "acls": [], "clientId": "c1", "expiresAt": null,
            "id": "t3", "mode": "Client", "name": "Desktop", "userId": "u1",
        },
    ]));
    assert_eq!(list.0[1].mode, TokenMode::System);
    assert!(list.0[0].expires_at.is_none() && list.0[1].expires_at.is_some());
    // The value is not a field of the list; one that arrives is dropped.
    let with_value: ApiTokenList = serde_json::from_value(json!([{
        "acls": [], "clientId": null, "expiresAt": null, "id": "t", "mode": "User",
        "name": "n", "userId": "u", "token": "tok-s3cret",
    }]))
    .unwrap();
    assert!(
        !serde_json::to_string(&with_value)
            .unwrap()
            .contains("tok-s3cret")
    );
    assert!(
        serde_json::from_value::<ApiTokenList>(json!([{
            "acls": [], "clientId": null, "expiresAt": "2030-01-02", "id": "t", "mode": "User",
            "name": "n", "userId": "u",
        }]))
        .is_err()
    );
    assert!(
        serde_json::from_value::<ApiTokenList>(json!([{
            "acls": [], "clientId": null, "expiresAt": null, "id": "t", "mode": "Admin",
            "name": "n", "userId": "u",
        }]))
        .is_err()
    );
}

#[test]
fn a_new_token_carries_its_value_and_the_request_an_optional_expiry() {
    let created: NewApiToken = round_trip(json!({
        "acls": ["read"], "clientId": null, "expiresAt": STAMP, "id": "t1", "mode": "User",
        "name": "Backup", "token": "tok-value", "userId": "u1",
    }));
    assert_eq!(created.token, "tok-value");
    assert!(
        serde_json::from_value::<NewApiToken>(json!({
            "acls": [], "clientId": null, "expiresAt": null, "id": "t", "mode": "User",
            "name": "n", "userId": "u",
        }))
        .is_err()
    );
    let minimal: TokenCreate = round_trip(json!({ "acls": ["read"], "name": "Backup" }));
    assert!(minimal.expiry.is_none());
    let dated: TokenCreate = round_trip(json!({
        "acls": ["read", "clients:read"], "expiry": "2030-01-02T04:04:05.006+01:00", "name": "n",
    }));
    assert_eq!(dated.acls.len(), 2);
    // `null` reads as absent and is written back as absent.
    let null: TokenCreate =
        serde_json::from_value(json!({ "acls": ["read"], "expiry": null, "name": "n" })).unwrap();
    assert!(null.expiry.is_none());
    assert_eq!(
        serde_json::to_value(&null).unwrap(),
        json!({ "acls": ["read"], "name": "n" })
    );
    assert!(serde_json::from_value::<TokenCreate>(json!({ "name": "n" })).is_err());
}

#[test]
fn rights_are_a_map_of_descriptions_and_the_granted_rights_an_array_or_null() {
    let described: AclDescriptions = round_trip(json!({
        "clients:read": "Read the clients connected to this account",
        "read": "Fetch user information",
    }));
    assert_eq!(described.0.len(), 2);
    let _: AclDescriptions = round_trip(json!({}));
    assert!(serde_json::from_value::<AclDescriptions>(json!({ "read": 1 })).is_err());
    let none: GrantedAcls = round_trip(json!(null));
    assert_eq!(none.0, None);
    let some: GrantedAcls = round_trip(json!(["user:read", "system:setup"]));
    assert_eq!(some.0.unwrap().len(), 2);
    assert!(serde_json::from_value::<GrantedAcls>(json!({})).is_err());
}

#[test]
fn debug_hides_the_token_value_and_the_totp_secret() {
    let created: NewApiToken = serde_json::from_value(json!({
        "acls": ["read"], "clientId": null, "expiresAt": null, "id": "t1", "mode": "User",
        "name": "Backup", "token": "tok-s3cret", "userId": "u1",
    }))
    .unwrap();
    assert_redacted(&created, &["tok-s3cret"]);
    assert!(format!("{created:?}").contains("Backup"));
    let marker = "zzmarker";
    let setup = TotpSetup {
        secret: marker.into(),
        url: format!("otpauth://totp/x?secret={marker}"),
    };
    assert_redacted(&setup, &[marker]);
}
