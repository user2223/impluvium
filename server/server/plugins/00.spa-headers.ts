import { spaSecurityHeaders } from "../utils/spa-contract";

export default defineNitroPlugin((nitroApp) => {
  if (import.meta.dev) return;
  const headers = spaSecurityHeaders([]);
  // Nitro calls this before route rules, including their early redirects.
  nitroApp.hooks.hook("request", (event) => {
    setHeaders(event, headers);
  });
  nitroApp.hooks.hook("beforeResponse", (event) => {
    if (event.node.res.headersSent) return;
    // Preserve the delivery middleware's per-HTML script hashes.
    for (const [name, value] of Object.entries(headers)) {
      if (name !== "Content-Security-Policy") setHeader(event, name, value);
    }
    removeResponseHeader(event, "X-Powered-By");
  });
});
