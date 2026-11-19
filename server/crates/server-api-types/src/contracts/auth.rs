// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Sign-in, registration, second factors and passkeys.
//!
//! Routes that start or complete a sign-in set the session cookie; the cookie is described by the
//! route, not by a body.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::time::Timestamp;

/// A sign-in method of the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
pub enum AuthProvider {
    /// A name and a password.
    Simple,
    /// Single sign-on with an identity provider.
    #[serde(rename = "OpenID")]
    OpenId,
}

/// `GET /api/v1/auth`: how a visitor can sign in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AuthInfo {
    /// The methods that are on.
    pub enabled_auth_providers: Vec<AuthProvider>,
    /// The name of the identity provider as it should be shown; absent when none is configured.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub oidc_provider_name: Option<String>,
}

/// A second factor an account can have.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
pub enum MfaMethod {
    /// A passkey or security key.
    WebAuthn,
    /// A one-time code from an authenticator app.
    #[serde(rename = "TOTP")]
    Totp,
}

/// `GET /api/v1/auth/mfa`: the second factors of the account; the body is the bare array.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct MfaMethods(pub Vec<MfaMethod>);

/// `POST /api/v1/auth/mfa/totp`: the code the authenticator app shows. The answer is `{}`.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct TotpRequest {
    /// The code.
    pub code: String,
}

/// The kind of a public key credential; always `public-key`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
pub enum CredentialType {
    /// A public key credential.
    #[serde(rename = "public-key")]
    PublicKey,
}

/// How an authenticator is attached to the device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum AuthenticatorAttachment {
    /// A roaming authenticator such as a security key.
    CrossPlatform,
    /// An authenticator built into the device.
    Platform,
}

/// A way an authenticator can be reached.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum AuthenticatorTransport {
    /// Built into the device.
    Internal,
    /// Bluetooth Low Energy.
    Ble,
    /// A caBLE connection.
    Cable,
    /// A hybrid connection.
    Hybrid,
    /// Near-field communication.
    Nfc,
    /// A smart card.
    SmartCard,
    /// USB.
    Usb,
}

/// A hint about the kind of authenticator to offer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum AuthenticatorHint {
    /// A phone or tablet.
    Hybrid,
    /// A security key.
    SecurityKey,
    /// An authenticator of this device.
    ClientDevice,
}

/// How strongly the user must be verified.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum UserVerification {
    /// Verification is not wanted.
    Discouraged,
    /// Verification is wanted when possible.
    Preferred,
    /// Verification is required.
    Required,
}

/// A credential the server accepts for a sign-in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct AllowedCredential {
    /// The credential id, base64url.
    pub id: String,
    /// The ways the authenticator can be reached; absent when unknown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub transports: Option<Vec<AuthenticatorTransport>>,
    /// Always `public-key`.
    #[serde(rename = "type")]
    pub kind: CredentialType,
}

/// The extensions the server asks for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AuthenticationExtensions {
    /// A legacy application id; absent when not asked for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub appid: Option<String>,
    /// Whether to report the credential properties; absent when not asked for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub cred_props: Option<bool>,
    /// Whether to create an HMAC secret; absent when not asked for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub hmac_create_secret: Option<bool>,
    /// Whether to report the minimum PIN length; absent when not asked for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub min_pin_length: Option<bool>,
}

/// The options of a passkey sign-in, answer of the two `start` routes. The browser passes them on to
/// the authenticator.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct WebAuthnRequestOptions {
    /// The accepted credentials; absent or empty accepts any credential of the site.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub allow_credentials: Option<Vec<AllowedCredential>>,
    /// The challenge to sign, base64url.
    pub challenge: String,
    /// The extensions asked for; absent when none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub extensions: Option<AuthenticationExtensions>,
    /// Hints about the authenticator; absent when none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub hints: Option<Vec<AuthenticatorHint>>,
    /// The relying party id; absent leaves it to the browser.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub rp_id: Option<String>,
    /// How long the browser waits, in milliseconds; absent leaves it to the browser.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub timeout: Option<u32>,
    /// How strongly the user must be verified; absent leaves it to the browser.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub user_verification: Option<UserVerification>,
}

/// The signed answer of the authenticator, body of the two `finish` routes. The server reads only
/// the fields below and ignores others.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct WebAuthnAssertion {
    /// How the authenticator is attached; absent when the browser does not say.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub authenticator_attachment: Option<AuthenticatorAttachment>,
    /// The results of the extensions.
    pub client_extension_results: ClientExtensionResults,
    /// The credential id, base64url.
    pub id: String,
    /// The credential id as raw bytes, base64url.
    pub raw_id: String,
    /// The signed data.
    pub response: AssertionResponse,
    /// Always `public-key`.
    #[serde(rename = "type")]
    pub kind: CredentialType,
}

/// The results of the extensions of a sign-in.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ClientExtensionResults {
    /// Whether the legacy application id was used; absent when not reported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub appid: Option<bool>,
    /// The credential properties; absent when not reported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub cred_props: Option<CredentialProperties>,
    /// Whether an HMAC secret was created; absent when not reported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub hmac_create_secret: Option<bool>,
}

/// The properties of a credential.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
pub struct CredentialProperties {
    /// Whether the credential is discoverable; absent when not reported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub rk: Option<bool>,
}

/// The authenticator data and the signature of a sign-in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AssertionResponse {
    /// The authenticator data, base64url.
    pub authenticator_data: String,
    /// The client data, base64url.
    #[serde(rename = "clientDataJSON")]
    pub client_data_json: String,
    /// The signature, base64url.
    pub signature: String,
    /// The user handle, base64url; absent when the authenticator gives none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub user_handle: Option<String>,
}

/// `POST /api/v1/auth/signin/simple`: name and password.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SigninRequest {
    /// The user name.
    pub username: String,
    /// The password.
    pub password: String,
    /// Keep the session for the longer period; absent or `null` means `false`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub remember_me: Option<bool>,
}

/// How far a sign-in got.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
pub enum SigninResult {
    /// The sign-in is complete.
    #[serde(rename = "signin")]
    Signin,
    /// The account has a second factor that is still to be completed.
    #[serde(rename = "2fa")]
    Mfa,
}

/// The answer of a name and password sign-in. The session cookie comes with it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SigninResponse {
    /// Whether the sign-in is complete or needs a second factor.
    pub result: SigninResult,
    /// The id of the account.
    pub user_id: String,
}

/// `GET /api/v1/auth/oidc/callback`: the query the identity provider appends.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct OidcCallbackQuery {
    /// The authorisation code.
    pub code: String,
    /// The state of the sign-in the browser started.
    pub state: String,
}

/// `POST /api/v1/auth/oidc/logout`: the form the identity provider posts (back channel). The
/// field name has an underscore on the wire.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct OidcLogoutForm {
    /// The signed logout token.
    pub logout_token: String,
}

/// The answer to an accepted logout notification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct OidcLogoutResponse {
    /// Always `true`.
    pub success: bool,
}

/// `GET /api/v1/auth/signup/simple`: the invitation to look up.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct SignupQuery {
    /// The id of the invitation.
    pub id: String,
}

/// What a visitor learns about an invitation. The invite link address is not part of it.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SignupInvitation {
    /// The id of the invitation.
    pub id: String,
    /// Whether the new account becomes an administrator.
    pub is_admin: bool,
    /// The user name the invitation fixes; `null` when the invitee chooses.
    pub username: Option<String>,
    /// The email address the invitation fixes; `null` when the invitee chooses.
    pub email: Option<String>,
    /// When the invitation runs out.
    pub expires: Timestamp,
}

/// `POST /api/v1/auth/signup/simple`: registers with an invitation.
///
/// The user name and the email address are required even when the invitation fixes them; the
/// fixed values win.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SignupRequest {
    /// The id of the invitation.
    pub invitation: String,
    /// The user name, at least 5 characters.
    pub username: String,
    /// The email address.
    pub email: String,
    /// The password, at least 8 characters.
    pub password: String,
    /// The name to show; absent or `null` uses the user name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub display_name: Option<String>,
}

/// The account that a registration created.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SignupAccount {
    /// The id of the account.
    pub id: String,
    /// The user name.
    pub username: String,
    /// Whether the account is an administrator.
    pub admin: bool,
    /// Whether the account may sign in.
    pub enabled: bool,
    /// The email address.
    pub email: String,
    /// The name to show.
    pub display_name: String,
    /// The object id of the profile picture.
    pub profile_picture_object_id: String,
}

redacted_debug!(TotpRequest, shown: [], hidden: [code]);

redacted_debug!(OidcCallbackQuery, shown: [], hidden: [code, state]);

redacted_debug!(OidcLogoutForm, shown: [], hidden: [logout_token]);

redacted_debug!(SigninRequest, shown: [username, remember_me], hidden: [password]);

redacted_debug!(SignupQuery, shown: [], hidden: [id]);

redacted_debug!(SignupInvitation, shown: [is_admin, username, email, expires], hidden: [id]);

redacted_debug!(SignupRequest, shown: [username, email, display_name], hidden: [invitation, password]);
