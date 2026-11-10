// Explicit contracts for unvalidated inputs and responses whose handler wrappers
// or Object.fromEntries calls erase their types. Service return types preserve
// the existing payloads; these contracts add no server validation rules.
export const bodies = {
  'auth/signin/simple.post': { validator: 'signinValidator' },
  'admin/company/[id]/index.patch': { expression: 'Partial<Omit<RawResponse, "id">>' },
  'admin/game/[id]/index.patch': { expression: 'Partial<Omit<RawResponse, "id">>' },
  'client/auth/handshake.post': { expression: '{ clientId: string; token: string }' },
  'client/auth/callback/index.post': { expression: '{ id: string }' },
  'client/auth/code/index.post': { expression: '{ id: string }' },
  'client/capability/index.post': { expression: '{ capability: "peerAPI" | "userStatus" | "cloudSaves" | "trackPlaytime"; configuration: Record<string, JsonValue> }' },
  'client/collection/index.post': { expression: '{ name: string }' },
  'auth/passkey/finish.post': { expression: 'Parameters<typeof import("@simplewebauthn/server").verifyAuthenticationResponse>[0]["response"]' },
  'auth/mfa/webauthn/finish.post': { expression: 'Parameters<typeof import("@simplewebauthn/server").verifyAuthenticationResponse>[0]["response"]' },
  'user/mfa/webauthn/finish.post': { expression: 'Parameters<typeof import("@simplewebauthn/server").verifyRegistrationResponse>[0]["response"]' },
};

for (const prefix of ['collection', 'client/collection']) {
  for (const collection of ['default', '[id]']) {
    for (const method of ['post', 'delete']) {
      bodies[`${prefix}/${collection}/entry.${method}`] = { expression: '{ id: string }' };
    }
  }
}

export const binaryBodies = new Set([
  'object/[id]/index.post',
  'screenshots/game/[id]/index.post',
  'client/saves/[gameid]/[slotindex]/push.post',
]);

// Export every member of the enums used by browser consumers, including members
// that are not currently selected by a particular view.
export const browserEnums = [
  'AgeRatingOrganization', 'AuthMec', 'GameType', 'LibraryBackend', 'MFAMec', 'Platform',
];

export const formBodies = new Set([
  'admin/company/[id]/banner.post',
  'admin/company/[id]/icon.post',
  'admin/game/[id]/metadata.post',
  'admin/game/image/index.post',
  'admin/news/index.post',
  'admin/settings/logo.post',
  'auth/oidc/logout.post',
]);

const newsQuery = '{ order?: "asc" | "desc"; tags?: string[]; limit?: string; skip?: string; search?: string }';
export const queries = {
  'store/index.get': { validator: 'StoreRead' },
  'news/index.get': { expression: newsQuery },
  'client/news/index.get': { expression: newsQuery },
  'admin/news/index.get': { expression: newsQuery },
  'auth/signup/simple.get': { expression: '{ id: string }' },
  'auth/oidc/callback.get': { expression: '{ code: string; state: string }' },
  'client/auth/index.get': { expression: '{ id: string }' },
  'client/auth/code/index.get': { expression: '{ code: string }' },
  'admin/import/version/index.get': { expression: '{ id: string }' },
  'admin/import/game/search.get': { expression: '{ q: string }' },
  'routes/auth/oidc.get': { expression: '{ redirect?: string }' },
};

// fetchLibrary includes each game's playtime; the other collection queries do not.
const libraryResponse = 'Awaited<ReturnType<typeof import("./server/internal/userlibrary").default.fetchLibrary>>';

export const responses = {
  'client/user/library.get': `Array<${libraryResponse}["entries"][number]["game"]>`,
  'client/collection/default/index.get': libraryResponse,
  'collection/default/index.get': libraryResponse,
  'client/collection/index.get': 'Awaited<ReturnType<typeof import("./server/internal/userlibrary").default.fetchCollections>>',
  'client/collection/index.post': 'Awaited<ReturnType<typeof import("./server/internal/userlibrary").default.collectionCreate>>',
  'client/collection/[id]/index.get': 'NonNullable<Awaited<ReturnType<typeof import("./server/internal/userlibrary").default.fetchCollection>>>',
  'client/collection/[id]/entry.post': 'Awaited<ReturnType<typeof import("./server/internal/userlibrary").default.collectionAdd>>',
  'user/auth/index.get': `{
    mecs: Record<string, Omit<import("./prisma/client/models").LinkedAuthMecModel, "credentials">>;
    available: string[];
  }`,
};

// The MFA handler retains a concrete string-keyed dictionary of model rows.
// Sanitizing credentials in place preserves their declared Prisma JsonValue
// type, not a discriminated union correlated with individual dictionary keys.
// Use its inferred response instead of claiming that narrower static contract.
