import { dirname, resolve } from "node:path";
import { validateSpaManifest, type SpaManifest } from "./spa-contract";

// Resolve from the deployed entry point, independently of Nitro's chunk layout.
export const spaRoot = resolve(dirname(process.argv[1]), "../public");
let manifest: SpaManifest | undefined;
export async function initializeSpaOutput() {
  manifest = await validateSpaManifest(spaRoot);
}
export function spaOutputManifest() {
  if (!manifest) throw new Error("SPA output was not validated at startup");
  return manifest;
}
