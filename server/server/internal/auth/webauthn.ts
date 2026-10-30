import { systemConfig } from "../config/sys-conf";
import type { AuthenticatorTransportFuture } from "@simplewebauthn/server";

export async function getRpId() {
  const externalUrl =
    process.env.WEBAUTHN_DOMAIN ?? (await systemConfig.getExternalUrl());
  const externalUrlParsed = new URL(externalUrl);

  return externalUrlParsed.hostname;
}

export interface Passkey {
  name: string;
  created: number;
  userId: string;
  webAuthnUserId: string;
  id: string;
  publicKey: string;
  counter: number;
  transports: Array<AuthenticatorTransportFuture> | undefined;
  deviceType: string;
  backedUp: boolean;
}

export interface WebAuthNv1Credentials {
  passkeys: Array<Passkey>;
}
