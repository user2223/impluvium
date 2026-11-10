import type { ApiFetch } from "../../../server/shared/api/client";

declare const fetchApi: ApiFetch;

async function representativeConsumers() {
  const info = await fetchApi("GET /api/v1");
  info.external.trim();
  info.serverName.trim();
  info.mLogoObjectId?.trim();
  info.version.toString();
  info.gitRef.toString();
  const auth = await fetchApi("GET /api/v1/auth");
  auth.enabledAuthProviders.includes("Simple");
  auth.oidcProviderName?.trim();
}
void representativeConsumers;

async function libraryResponses() {
  const games = await fetchApi("GET /api/v1/client/user/library");
  for (const game of games) {
    game.mName.trim();
    game.mReleased.trim();
    game.created.trim();
    for (const playtime of game.playtime) {
      playtime.seconds.toFixed();
      playtime.updatedAt.trim();
      playtime.createdAt.trim();
    }
  }
  const library = await fetchApi("GET /api/v1/client/collection/default");
  library.entries[0]?.game.playtime[0]?.seconds.toFixed();
  const collections = await fetchApi("GET /api/v1/client/collection");
  collections[0]?.entries[0]?.game.mName.trim();
  // @ts-expect-error Wire dates are strings, not Date instances.
  games[0]?.mReleased.toISOString();
  // @ts-expect-error Playtime is numeric, not a string.
  games[0]?.playtime[0]?.seconds.toUpperCase();
  // @ts-expect-error Ordinary collection queries do not include the playtime relation.
  collections[0]?.entries[0]?.game.playtime;
}
void libraryResponses;

fetchApi("PATCH /api/v1/admin/game/:id/versions", {
  path: { id: "sample-game" },
  body: { versions: ["sample-version"] },
});
fetchApi("GET /api/v1/admin/search/game", { query: { q: "sample" } });
fetchApi("POST /api/v1/collection", { body: { name: "Sample" } });
fetchApi("POST /api/v1/admin/auth/invitation", {
  body: { expires: "2030-01-01T00:00:00.000Z" },
});
fetchApi("POST /api/v1/admin/import/version", {
  body: {
    id: "sample-game",
    version: { type: "local", identifier: "sample-version", name: "Sample" },
    launches: [],
    setups: [],
  },
});
fetchApi("POST /api/v1/user/token", {
  body: { name: "Sample", acls: ["read"], expiry: "2030-01-01T00:00:00.000Z" },
});
fetchApi("GET /api/v1/store", { query: { take: "10" } });
fetchApi("PATCH /api/v1/admin/settings", { body: {} });
fetchApi("GET /api/v1", { retryDelay: 25 });

// @ts-expect-error Unknown route.
fetchApi("GET /api/v1/missing");
// @ts-expect-error The route does not accept this method.
fetchApi("DELETE /api/v1/auth");
// @ts-expect-error Path parameters are required.
fetchApi("GET /api/v1/games/:id");
// @ts-expect-error Path parameter names must match the explicit template.
fetchApi("GET /api/v1/games/:id", { path: { gameId: "sample" } });
// @ts-expect-error Query values do not satisfy path parameters.
fetchApi("GET /api/v1/games/:id", { query: { id: "sample" } });
// @ts-expect-error Request bodies have concrete field types.
fetchApi("POST /api/v1/collection", { body: { name: 42 } });
// @ts-expect-error A required body may not be omitted.
fetchApi("POST /api/v1/collection");
// @ts-expect-error Read-only endpoints have no request body.
fetchApi("GET /api/v1", { body: { name: "sample" } });
// @ts-expect-error Query fields must match the route contract.
fetchApi("GET /api/v1/admin/search/game", { query: { search: "sample" } });
// @ts-expect-error Raw URLs are not route templates.
fetchApi("GET /api/v1/games/sample");
// @ts-expect-error Redirects use browser navigation, not a JSON fetch.
fetchApi("GET /auth/oidc");
// @ts-expect-error WebSockets use their own transport.
fetchApi("GET /api/v1/task");
// @ts-expect-error The caller cannot replace the route's method.
fetchApi("GET /api/v1", { method: "POST" });
// @ts-expect-error Defaulted fields retain their concrete input types.
fetchApi("POST /api/v1/admin/auth/invitation", { body: { expires: "sample", isAdmin: "yes" } });
// @ts-expect-error Date parsers accept wire strings, not Date instances.
fetchApi("POST /api/v1/user/token", { body: { name: "Sample", acls: ["read"], expiry: new Date() } });

// @ts-expect-error An object body remains required when all its fields are optional.
fetchApi("PATCH /api/v1/admin/settings");
// @ts-expect-error Supplying options does not make a validated object body optional.
fetchApi("PATCH /api/v1/admin/settings", {});
// @ts-expect-error Undefined is not a validated object body.
fetchApi("PATCH /api/v1/admin/settings", { body: undefined });
// @ts-expect-error Request hooks could change the route, method, response kind or credentials.
fetchApi("GET /api/v1", { onRequest: () => {} });
// @ts-expect-error Request-error hooks also receive mutable fetch options.
fetchApi("GET /api/v1", { onRequestError: () => {} });
// @ts-expect-error Response hooks could replace the promised response data.
fetchApi("GET /api/v1", { onResponse: () => {} });
// @ts-expect-error Response-error hooks could change a retried request.
fetchApi("GET /api/v1", { onResponseError: () => {} });
// @ts-expect-error Retry-delay callbacks receive mutable fetch context.
fetchApi("GET /api/v1", { retryDelay: () => 25 });
const hookedOptions = { headers: { Authorization: "Bearer sample" }, onRequest: () => {} };
// @ts-expect-error Mutable hooks are forbidden even through an existing options variable.
fetchApi("GET /api/v1", hookedOptions);
const overriddenOptions = { headers: { Authorization: "Bearer sample" }, responseType: "text" as const };
// @ts-expect-error Fixed response kinds cannot be overridden through an options variable.
fetchApi("GET /api/v1", overriddenOptions);
// @ts-expect-error Custom dispatchers could replace the bound HTTP request.
fetchApi("GET /api/v1", { dispatcher: {} });
// @ts-expect-error Custom agents cannot replace the typed transport.
fetchApi("GET /api/v1", { agent: {} });
