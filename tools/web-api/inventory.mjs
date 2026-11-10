import { readdir, readFile } from 'node:fs/promises';
import { extname, join } from 'node:path';

// This is the explicit inventory of the existing HTTP API, not a Rust router registry.
export const handlers = `
admin/auth/index.get
admin/auth/invitation/index.delete
admin/auth/invitation/index.get
admin/auth/invitation/index.post
admin/company/[id]/banner.post
admin/company/[id]/game.delete
admin/company/[id]/game.patch
admin/company/[id]/game.post
admin/company/[id]/icon.post
admin/company/[id]/index.delete
admin/company/[id]/index.get
admin/company/[id]/index.patch
admin/company/index.get
admin/company/index.post
admin/depot/index.get
admin/depot/index.post
admin/depot/upload.post
admin/game/[id]/age-ratings.patch
admin/game/[id]/index.delete
admin/game/[id]/index.get
admin/game/[id]/index.patch
admin/game/[id]/metadata.post
admin/game/[id]/tags.patch
admin/game/[id]/versions/index.delete
admin/game/[id]/versions/index.get
admin/game/[id]/versions/index.patch
admin/game/image/index.delete
admin/game/image/index.post
admin/groups/[id]/index.delete
admin/groups/[id]/index.get
admin/groups/[id]/index.patch
admin/groups/[id]/members.patch
admin/groups/[id]/ratings.patch
admin/groups/index.get
admin/groups/index.post
admin/home/index.get
admin/import/game/index.get
admin/import/game/index.post
admin/import/game/search.get
admin/import/massversion/index.get
admin/import/massversion/index.post
admin/import/version/index.get
admin/import/version/index.post
admin/import/version/preload.get
admin/index.get
admin/library/index.get
admin/library/libraries.get
admin/library/sources/index.delete
admin/library/sources/index.get
admin/library/sources/index.patch
admin/library/sources/index.post
admin/news/[id]/index.delete
admin/news/[id]/index.get
admin/news/index.get
admin/news/index.post
admin/search/game.get
admin/services/index.get
admin/settings/dummy-data.get
admin/settings/index.patch
admin/settings/logo.post
admin/system-data/ws.get
admin/tags/[id]/index.delete
admin/tags/index.get
admin/tags/index.post
admin/task/index.get
admin/task/index.post
admin/token/[id]/index.delete
admin/token/acls.get
admin/token/index.get
admin/token/index.post
admin/users/[id]/index.delete
admin/users/[id]/index.get
admin/users/index.get
auth/index.get
auth/mfa/index.get
auth/mfa/totp.post
auth/mfa/webauthn/finish.post
auth/mfa/webauthn/start.post
auth/oidc/callback.get
auth/oidc/logout.post
auth/passkey/finish.post
auth/passkey/start.post
auth/signin/simple.post
auth/signup/simple.get
auth/signup/simple.post
client/auth/callback/index.post
client/auth/code/index.get
client/auth/code/index.post
client/auth/code/ws.get
client/auth/handshake.post
client/auth/index.get
client/auth/initiate.post
client/auth/session.post
client/capability/index.post
client/collection/[id]/entry.delete
client/collection/[id]/entry.post
client/collection/[id]/index.delete
client/collection/[id]/index.get
client/collection/default/entry.delete
client/collection/default/entry.post
client/collection/default/index.get
client/collection/index.get
client/collection/index.post
client/depots/index.get
client/game/[id]/index.get
client/game/[id]/version/[versionid]/index.get
client/game/[id]/versions.get
client/game/manifest.get
client/news/[id]/index.get
client/news/index.get
client/object/[id]/index.get
client/playtime/[gameid]/index.get
client/playtime/[gameid]/index.post
client/playtime/index.get
client/saves/[gameid]/[slotindex]/index.delete
client/saves/[gameid]/[slotindex]/index.get
client/saves/[gameid]/[slotindex]/push.post
client/saves/[gameid]/index.get
client/saves/[gameid]/index.post
client/saves/index.get
client/saves/settings.get
client/user/index.get
client/user/library.get
client/user/webtoken.post
collection/[id]/entry.delete
collection/[id]/entry.post
collection/[id]/index.delete
collection/[id]/index.get
collection/default/entry.delete
collection/default/entry.post
collection/default/index.get
collection/index.get
collection/index.post
companies/[id]/index.get
emoji/[codepoint]/index.get
games/[id]/index.get
index.get
news/[id]/index.get
news/index.get
notifications/[id]/index.delete
notifications/[id]/index.get
notifications/[id]/read.post
notifications/clear.post
notifications/index.get
notifications/readall.post
notifications/ws.get
object/[id]/index.delete
object/[id]/index.get
object/[id]/index.head
object/[id]/index.post
screenshots/[id]/index.delete
screenshots/[id]/index.get
screenshots/game/[id]/index.get
screenshots/game/[id]/index.post
screenshots/index.get
settings/index.get
setup.post
store/featured.get
store/index.get
store/tags.get
tags/[id]/index.get
task/index.get
token.get
user/[id]/index.get
user/auth/index.get
user/client/[id]/index.delete
user/client/index.get
user/index.get
user/mfa/index.get
user/mfa/totp/finish.post
user/mfa/totp/start.post
user/mfa/webauthn/finish.post
user/mfa/webauthn/index.delete
user/mfa/webauthn/start.post
user/superlevel
user/token/[id]/index.delete
user/token/acls.get
user/token/index.get
user/token/index.post
`.trim().split('\n').map((name) => `server/server/api/v1/${name}.ts`);

export const extraHandlers = [
  'server/server/routes/auth/oidc.get.ts',
  'server/server/routes/auth/signout.get.ts',
];

export const redirectRules = [{
  path: '/auth/callback/oidc',
  target: '/api/v1/auth/oidc/callback',
  kind: 'redirect',
}];

const empty = new Set(`
admin/company/[id]/game.delete
admin/company/[id]/game.patch
admin/company/[id]/index.delete
admin/game/[id]/tags.patch
admin/library/sources/index.delete
admin/tags/[id]/index.delete
admin/token/[id]/index.delete
client/auth/code/index.post
client/auth/session.post
client/saves/[gameid]/[slotindex]/index.delete
client/saves/[gameid]/[slotindex]/push.post
notifications/clear.post
notifications/readall.post
object/[id]/index.head
screenshots/[id]/index.delete
screenshots/game/[id]/index.post
setup.post
user/client/[id]/index.delete
user/mfa/totp/finish.post
user/mfa/webauthn/finish.post
user/mfa/webauthn/index.delete
user/token/[id]/index.delete
`.trim().split('\n'));
const binary = new Set(['object/[id]/index.get', 'client/object/[id]/index.get', 'emoji/[codepoint]/index.get']);
const websocket = new Set(['task/index.get', 'notifications/ws.get', 'admin/system-data/ws.get', 'client/auth/code/ws.get']);
const text = new Set(['client/auth/code/index.get', 'client/auth/initiate.post', 'client/user/webtoken.post']);

export function normalize(file) {
  const api = file.startsWith('server/server/api/');
  const relative = file.replace(/^server\/server\/(api|routes)\//, '').replace(/\.ts$/, '');
  const match = relative.match(/\.(get|head|post|put|patch|delete|options|connect|trace)$/);
  const method = match ? match[1].toUpperCase() : '*';
  const segments = (match ? relative.slice(0, -match[0].length) : relative).split('/');
  if (segments.at(-1) === 'index') segments.pop();
  const path = `${api ? '/api' : ''}/${segments.map((segment) => {
    if (/^\[\w+\]$/.test(segment)) return `:${segment.slice(1, -1)}`;
    if (/[\[\]]/.test(segment)) throw new Error(`Unsupported route segment: ${file}`);
    return segment;
  }).join('/')}`;
  return { file, method, path };
}

export function describe(file) {
  const route = normalize(file);
  const name = file.replace('server/server/api/v1/', '').replace(/\.ts$/, '');
  const kind = extraHandlers.includes(file) || name === 'auth/oidc/callback.get' ? 'redirect'
    : websocket.has(name) ? 'websocket' : text.has(name) ? 'text'
      : name === 'object/[id]/index.get' ? 'binary-or-empty' : binary.has(name) ? 'binary'
      : empty.has(name) ? 'empty' : name === 'user/index.get' ? 'json-or-empty' : 'json';
  return { ...route, kind };
}

export async function filesBelow(directory) {
  const entries = await readdir(directory, { withFileTypes: true });
  return (await Promise.all(entries.map(async (entry) => {
    const path = join(directory, entry.name);
    return entry.isDirectory() ? filesBelow(path) : [path];
  }))).flat().sort();
}

export function validateInventory(expected, actual) {
  const files = new Set();
  const pairs = new Set();
  for (const route of expected) {
    if (files.has(route.file)) throw new Error(`Duplicate handler: ${route.file}`);
    files.add(route.file);
    const pair = `${route.method} ${route.path}`;
    if (pairs.has(pair)) throw new Error(`Duplicate method/path: ${pair}`);
    pairs.add(pair);
  }
  for (const file of actual) {
    if (!files.delete(file)) throw new Error(`Unmapped handler: ${file}`);
  }
  if (files.size) throw new Error(`Missing handler: ${[...files].join(', ')}`);
}

export function validateRedirectRules(config) {
  const matches = [...config.matchAll(/["'](\/[^"']*)["']\s*:\s*\{\s*redirect:\s*["']([^"']*)["']/g)]
    .map((match) => ({ path: match[1], target: match[2], kind: 'redirect' }));
  const count = [...config.matchAll(/\bredirect\s*:/g)].length;
  if (count !== matches.length || JSON.stringify(matches) !== JSON.stringify(redirectRules)) {
    throw new Error('Missing, additional or changed redirect rule. Update the explicit route inventory.');
  }
}

export async function readInventory(root) {
  // Keep this set aligned with Nitro's handler scan, including extensions the
  // transition generator does not support. Unsupported handlers must fail the
  // inventory instead of silently escaping the generated contract map.
  const extensions = new Set(['.js', '.mjs', '.cjs', '.ts', '.mts', '.cts', '.tsx', '.jsx']);
  const actual = (await Promise.all(['api', 'routes'].map((dir) => filesBelow(join(root, 'server/server', dir)))))
    .flat().filter((file) => extensions.has(extname(file))).map((file) => file.slice(root.length + 1));
  const unsupported = actual.filter((file) => extname(file) !== '.ts');
  if (unsupported.length) throw new Error(`Unsupported Nitro handler extensions; transition contracts currently require .ts: ${unsupported.join(', ')}`);
  const routes = [...handlers, ...extraHandlers].map(describe).sort((a, b) => a.file.localeCompare(b.file, 'en'));
  validateInventory(routes, actual);
  const config = await readFile(join(root, 'server/nuxt.config.ts'), 'utf8');
  validateRedirectRules(config);
  return routes;
}
