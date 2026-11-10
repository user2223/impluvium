import { initializeSpaOutput } from "../utils/spa-output";

export default defineNitroPlugin(async () => {
  // Production must fail before accepting requests or starting backend services.
  if (!import.meta.dev) await initializeSpaOutput();
});
