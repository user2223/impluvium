// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! The signed-in account: its profile, linked sign-in methods, devices and second factors.
//!
//! The routes that add or remove a second factor need a recent sign-in; the shapes here do not
//! say so, the route policy does.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::EmptyObject;
use super::auth::{
    AllowedCredential, AuthProvider, AuthenticationExtensions, AuthenticatorAttachment,
    AuthenticatorHint, ClientExtensionResults, CredentialType, MfaMethod, UserVerification,
};
use crate::enums::Platform;
use crate::time::Timestamp;

/// An account as the account owner and the administrators see it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct UserAccount {
    /// Whether the account is an administrator.
    pub admin: bool,
    /// The name to show.
    pub display_name: String,
    /// The email address.
    pub email: String,
    /// Whether the account may sign in.
    pub enabled: bool,
    /// The id of the account.
    pub id: String,
    /// The object id of the profile picture.
    pub profile_picture_object_id: String,
    /// The user name.
    pub username: String,
}

/// `GET /api/v1/user`: the account of the caller, or `null` when nobody is signed in or the token
/// does not allow reading the account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct CurrentUser(pub Option<UserAccount>);

/// `GET /api/v1/user/:id`: what any signed-in caller may know about an account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct UserProfile {
    /// The name to show.
    pub display_name: String,
    /// The id of the account.
    pub id: String,
    /// The object id of the profile picture.
    pub profile_picture_object_id: String,
    /// The user name.
    pub username: String,
}

/// A sign-in method linked to the account. The stored credentials are never part of it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct LinkedAuth {
    /// Whether the method may be used.
    pub enabled: bool,
    /// The method.
    pub mec: AuthProvider,
    /// The id of the account.
    pub user_id: String,
    /// The version of the stored credential format.
    pub version: u32,
}

/// The linked sign-in methods, keyed by the method names. A method that is not linked has no key.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
pub struct LinkedAuths {
    /// A name and a password.
    #[serde(rename = "Simple", default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub simple: Option<LinkedAuth>,
    /// Single sign-on.
    #[serde(rename = "OpenID", default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub open_id: Option<LinkedAuth>,
}

/// `GET /api/v1/user/auth`: the sign-in methods of the account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct UserAuth {
    /// The methods that are all supported by the server.
    pub available: Vec<AuthProvider>,
    /// The methods linked to the account.
    pub mecs: LinkedAuths,
}

/// What a client of the desktop application can do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
pub enum ClientCapability {
    /// Peer-to-peer downloads.
    PeerAPI,
    /// Reporting the status of the user.
    UserStatus,
    /// Storing saves on the server.
    CloudSaves,
    /// Recording play time.
    TrackPlaytime,
}

/// A desktop client that is registered to the account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ClientInfo {
    /// What the client can do.
    pub capabilities: Vec<ClientCapability>,
    /// The id of the client.
    pub id: String,
    /// When the client last connected.
    pub last_connected: Timestamp,
    /// The name of the client.
    pub name: String,
    /// The operating system of the client.
    pub platform: Platform,
    /// The id of the account.
    pub user_id: String,
}

/// `GET /api/v1/user/client`: the clients of the account; the body is the bare array.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ClientList(pub Vec<ClientInfo>);

/// A passkey of the account. The key itself never leaves the server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct PasskeySummary {
    /// When the passkey was added, as a timestamp string; the handlers of today answer
    /// milliseconds, so the handler converts. A deliberate change of the wire form.
    pub created: Timestamp,
    /// The credential id, base64url.
    pub id: String,
    /// The name the owner gave the passkey.
    pub name: String,
}

/// What a listed second factor holds: nothing for an authenticator app, the passkeys otherwise.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(untagged)]
pub enum MfaCredentials {
    /// The passkeys of a `WebAuthn` entry.
    Passkeys(Vec<PasskeySummary>),
    /// The `{}` of a `TOTP` entry; the secret is never listed.
    Totp(EmptyObject),
}

/// A second factor of the account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct MfaEntry {
    /// The passkeys, or `{}` for an authenticator app.
    pub credentials: MfaCredentials,
    /// Whether the second factor is set up completely.
    pub enabled: bool,
    /// The kind of second factor.
    pub mec: MfaMethod,
    /// The id of the account.
    pub user_id: String,
    /// The version of the stored credential format.
    pub version: u32,
}

/// The second factors, keyed by their names. A kind that is not set up has no key.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
pub struct MfaEntries {
    /// An authenticator app.
    #[serde(rename = "TOTP", default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub totp: Option<MfaEntry>,
    /// Passkeys and security keys.
    #[serde(rename = "WebAuthn", default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub web_authn: Option<MfaEntry>,
}

/// `GET /api/v1/user/mfa`: the second factors of the account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct MfaStatus {
    /// The kinds the server supports.
    pub available: Vec<MfaMethod>,
    /// The second factors of the account.
    pub mecs: MfaEntries,
}

/// `POST /api/v1/user/mfa/totp/start`: the secret of a new authenticator app entry. It is shown
/// once; the entry stays unfinished until a code is confirmed.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct TotpSetup {
    /// The secret in base32, for apps that take it typed in.
    pub secret: String,
    /// The `otpauth` address for the QR code.
    pub url: String,
}

/// `POST /api/v1/user/mfa/webauthn/start`: the name to give the new passkey.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct PasskeyName {
    /// The name.
    pub name: String,
}

/// `DELETE /api/v1/user/mfa/webauthn`: the passkey to remove. The answer is empty.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct PasskeyDelete {
    /// The credential id, base64url.
    pub id: String,
}

/// How the server wants to attest a new passkey.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum AttestationConveyance {
    /// The authenticator's statement is passed on as is.
    Direct,
    /// The statement of an enterprise authenticator is passed on.
    Enterprise,
    /// The statement may be changed by the browser.
    Indirect,
    /// No statement is wanted.
    None,
}

/// The formats of an attestation statement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum AttestationFormat {
    /// No statement.
    None,
    /// The U2F format.
    FidoU2f,
    /// The packed format.
    Packed,
    /// The Android SafetyNet format.
    AndroidSafetynet,
    /// The Android key format.
    AndroidKey,
    /// The TPM format.
    Tpm,
    /// The Apple format.
    Apple,
}

/// Whether the passkey is stored on the authenticator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum ResidentKey {
    /// A stored key is not wanted.
    Discouraged,
    /// A stored key is wanted when possible.
    Preferred,
    /// A stored key is required.
    Required,
}

/// What kind of authenticator the server accepts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AuthenticatorSelection {
    /// The way the authenticator is attached; absent accepts any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub authenticator_attachment: Option<AuthenticatorAttachment>,
    /// Whether a stored key is required; absent when not asked for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub require_resident_key: Option<bool>,
    /// Whether the key is stored on the authenticator; absent when not asked for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub resident_key: Option<ResidentKey>,
    /// How strongly the user must be verified; absent leaves it to the browser.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub user_verification: Option<UserVerification>,
}

/// A key algorithm the server accepts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct PublicKeyParameters {
    /// The algorithm number of the COSE registry; some are negative.
    pub alg: i32,
    /// Always `public-key`.
    #[serde(rename = "type")]
    pub kind: CredentialType,
}

/// The server as the passkey knows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct RelyingParty {
    /// The domain of the server; absent leaves it to the browser.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub id: Option<String>,
    /// The name of the server.
    pub name: String,
}

/// The account as the passkey knows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PasskeyUser {
    /// The name to show.
    pub display_name: String,
    /// The handle of the account for the authenticator, base64url.
    pub id: String,
    /// The user name.
    pub name: String,
}

/// The options for creating a passkey, answer of `POST /api/v1/user/mfa/webauthn/start`. The
/// browser passes them on to the authenticator.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct WebAuthnCreationOptions {
    /// How the authenticator attests the key; absent leaves it to the browser.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub attestation: Option<AttestationConveyance>,
    /// The attestation formats accepted; absent accepts any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub attestation_formats: Option<Vec<AttestationFormat>>,
    /// What kind of authenticator is accepted; absent accepts any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub authenticator_selection: Option<AuthenticatorSelection>,
    /// The challenge to sign, base64url.
    pub challenge: String,
    /// Credentials the authenticator must not register again; absent when none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub exclude_credentials: Option<Vec<AllowedCredential>>,
    /// The extensions asked for; absent when none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub extensions: Option<AuthenticationExtensions>,
    /// Hints about the authenticator; absent when none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub hints: Option<Vec<AuthenticatorHint>>,
    /// The key algorithms accepted, best first.
    pub pub_key_cred_params: Vec<PublicKeyParameters>,
    /// The server.
    pub rp: RelyingParty,
    /// How long the browser waits, in milliseconds; absent leaves it to the browser.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub timeout: Option<u32>,
    /// The account.
    pub user: PasskeyUser,
}

/// The authenticator's answer to the creation options, body of
/// `POST /api/v1/user/mfa/webauthn/finish`. The server reads only the fields below and ignores
/// others.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct WebAuthnRegistration {
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
    /// The attestation.
    pub response: AttestationResponse,
    /// Always `public-key`.
    #[serde(rename = "type")]
    pub kind: CredentialType,
}

/// The attestation of a new passkey.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AttestationResponse {
    /// The attestation object, base64url.
    pub attestation_object: String,
    /// The authenticator data, base64url; absent when the browser does not give it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub authenticator_data: Option<String>,
    /// The client data, base64url.
    #[serde(rename = "clientDataJSON")]
    pub client_data_json: String,
    /// The public key, base64url; absent when the browser does not give it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub public_key: Option<String>,
    /// The algorithm of the public key; absent when the browser does not give it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub public_key_algorithm: Option<i32>,
    /// The ways the authenticator can be reached, as the browser names them; absent when
    /// unknown. Plain strings on purpose: browsers add transports over time, and a value this
    /// server does not know must not make the registration fail. The handler keeps the ones it
    /// knows and drops the rest.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub transports: Option<Vec<String>>,
}

/// `GET /api/v1/user/superlevel`: whether the sign-in is recent enough to change second factors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct SuperlevelStatus(pub bool);

// The secret and the setup address both carry the secret.
redacted_debug!(TotpSetup, shown: [], hidden: [secret, url]);
