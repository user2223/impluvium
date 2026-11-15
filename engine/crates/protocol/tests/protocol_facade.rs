// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: 0BSD

use buffa::Message;
use impluvium_protocol::proto::impluvium::{manifest::v3, signed::v1, v2};
use sha2::{Digest, Sha256};

#[test]
fn message_facade_includes_service_less_packages() {
    let request = v2::StartPairingRequest {
        device_name: "synthetic device".into(),
        ..Default::default()
    };
    let bytes = request.encode_to_vec();
    let decoded = v2::StartPairingRequest::decode(&mut bytes.as_slice()).unwrap();
    assert_eq!(decoded.device_name, request.device_name);
    let manifest = v3::Manifest::default();
    assert!(v3::Manifest::decode(&mut manifest.encode_to_vec().as_slice()).is_ok());
    let envelope = v1::Envelope::default();
    assert!(v1::Envelope::decode(&mut envelope.encode_to_vec().as_slice()).is_ok());
}

#[test]
fn both_content_hash_dependencies_are_available() {
    assert_eq!(Sha256::digest(b"facade smoke test").len(), 32);
    assert_eq!(blake3::hash(b"facade smoke test").as_bytes().len(), 32);
}

#[cfg(any(feature = "server", feature = "client"))]
#[test]
fn connect_facade_resolves_with_all_schema_packages() {
    use impluvium_protocol::connect::impluvium::v2::AUTH_SERVICE_SERVICE_NAME;

    assert_eq!(AUTH_SERVICE_SERVICE_NAME, "impluvium.v2.AuthService");
}
