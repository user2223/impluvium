// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: 0BSD

#[test]
fn facade_preserves_protocol_and_transport_type_boundaries() {
    use impluvium_client::{ClientConfig, ServiceTransport, connect, proto};

    assert!(std::mem::size_of::<ClientConfig>() > 0);
    let _: proto::impluvium::v2::StartPairingRequest = Default::default();
    let _: proto::impluvium::manifest::v3::Manifest = Default::default();
    let _: proto::impluvium::signed::v1::Envelope = Default::default();
    assert!(
        std::mem::size_of::<connect::impluvium::v2::AuthServiceClient<ServiceTransport<()>>>() > 0
    );
}
