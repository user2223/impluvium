// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: 0BSD

import assert from "node:assert/strict";
import test from "node:test";
import { create, fromBinary, toBinary } from "@bufbuild/protobuf";
import * as protocol from "impluvium-protocol";
import { StartPairingRequestSchema, AuthService } from "impluvium-protocol/generated/impluvium/v2/auth_pb";
import { ManifestSchema } from "impluvium-protocol/generated/impluvium/manifest/v3/manifest_pb";
import { EnvelopeSchema } from "impluvium-protocol/generated/impluvium/signed/v1/envelope_pb";

test("package exports preserve message and service identities", () => {
  assert.equal(protocol.StartPairingRequestSchema, StartPairingRequestSchema);
  assert.equal(protocol.ManifestSchema, ManifestSchema);
  assert.equal(protocol.EnvelopeSchema, EnvelopeSchema);
  assert.equal(protocol.AuthService, AuthService);
  assert.equal(AuthService.typeName, "impluvium.v2.AuthService");
  assert.equal(AuthService.method.startPairing.input, StartPairingRequestSchema);
  const request = create(StartPairingRequestSchema, { deviceName: "synthetic device" });
  assert.equal(fromBinary(StartPairingRequestSchema, toBinary(StartPairingRequestSchema, request)).deviceName, request.deviceName);
  for (const schema of [ManifestSchema, EnvelopeSchema]) {
    assert.equal(fromBinary(schema, toBinary(schema, create(schema))).$typeName, schema.typeName);
  }
});
