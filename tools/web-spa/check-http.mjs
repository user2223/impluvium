import assert from 'node:assert/strict';
import { request as httpRequest } from 'node:http';
import { request as httpsRequest } from 'node:https';
import { brotliDecompressSync, gunzipSync } from 'node:zlib';

const base = new URL(process.argv[2] ?? 'http://127.0.0.1:3000');
if (!['http:', 'https:'].includes(base.protocol) || base.username || base.password || base.pathname !== '/') {
  throw new Error('Usage: node tools/web-spa/check-http.mjs http[s]://HOST:PORT');
}
function request(path, headers = {}, method = 'GET') {
  return new Promise((resolve, reject) => {
    // The raw path is deliberately not passed through URL normalization.
    const outgoing = (base.protocol === 'https:' ? httpsRequest : httpRequest)({
      protocol: base.protocol, hostname: base.hostname, port: base.port, path, method,
      headers: { Accept: 'text/html', ...headers }, timeout: 15000,
    }, (response) => {
      const chunks = [];
      response.on('data', (bytes) => chunks.push(bytes));
      response.on('end', () => resolve({ status: response.statusCode, headers: response.headers, bytes: Buffer.concat(chunks) }));
      response.on('error', reject);
    });
    outgoing.on('error', reject);
    outgoing.on('timeout', () => outgoing.destroy(new Error('HTTP check timed out')));
    outgoing.end();
  });
}

const shell = await request('/');
assert.equal(shell.status, 200);
assert.match(shell.headers['content-type'], /text\/html/);
assert.equal(shell.headers['cache-control'], 'no-cache');
const html = shell.bytes.toString('utf8');
assert.match(html, /impluvium-loading/);
assert.match(html, /\/loading\.css/);
assert.doesNotMatch(html, /<style\b|\sstyle\s*=/i);
const csp = shell.headers['content-security-policy'];
assert.ok(csp?.includes("script-src 'self'"));
assert.ok(csp.includes("style-src 'self'"));
assert.doesNotMatch(csp, /unsafe-inline|unsafe-eval/);

for (const path of ['/admin/users', '/store/synthetic-id', '/unknown-client-page', '/store/a.b', '//store/x']) {
  const response = await request(path);
  assert.equal(response.status, 200, path);
  assert.deepEqual(response.bytes, shell.bytes, path);
  assert.equal(response.headers['content-security-policy'], csp, path);
}
for (const path of ['/missing.JS?v=1', '//_nuxt/missing', '/_nuxt/./missing', '/./_nuxt/missing',
  '/%2e/_nuxt/missing', '/_fonts/', '/missing.png', '/csp-hashes.json']) {
  const response = await request(path);
  assert.equal(response.status, 404, path);
  assert.doesNotMatch(response.headers['content-type'], /text\/html/, path);
}
for (const path of ['/_nuxt/missing%252Ejs', '/missing%252Ejs', '/%255Fnuxt/missing',
  '/missing.js;v=1', '/_nuxt/%2E%2E/csp-hashes.json', '/asset%2fpart', '/asset%5cpart', '/%00']) {
  assert.equal((await request(path)).status, 400, path);
}
for (const path of ['/api', '/api/v1/unknown-handler']) {
  const response = await request(path);
  assert.equal(response.status, 404, path);
  assert.match(response.headers['content-type'], /application\/json/, path);
}
const callback = await request('/auth/callback/oidc?code=synthetic&state=synthetic%2Fstate');
assert.equal(callback.status, 307);
const target = new URL(callback.headers.location, base);
assert.equal(target.pathname, '/api/v1/auth/oidc/callback');
assert.deepEqual(Object.fromEntries(target.searchParams), { code: 'synthetic', state: 'synthetic/state' });
for (const [name, value] of Object.entries(shell.headers)) {
  if (['referrer-policy', 'permissions-policy', 'x-content-type-options', 'x-frame-options',
    'cross-origin-resource-policy', 'cross-origin-opener-policy', 'cross-origin-embedder-policy',
    'origin-agent-cluster', 'x-dns-prefetch-control', 'x-download-options',
    'x-permitted-cross-domain-policies', 'x-xss-protection'].includes(name)) {
    assert.equal(callback.headers[name], value, `Redirect lost ${name}`);
  }
}
assert.equal(callback.headers['referrer-policy'], 'no-referrer');
assert.equal(callback.headers['x-powered-by'], undefined);
for (const path of ['/auth/oidc', '/auth/signout']) {
  const response = await request(path);
  assert.ok([302, 303, 307].includes(response.status), path);
  assert.ok(response.headers.location, path);
}
const asset = html.match(/(?:src|href)="([^"<>]*\/_nuxt\/[^"<>]+\.js)"/)?.[1];
assert.ok(asset, 'No JavaScript asset was found in the generated shell');
const identity = await request(asset);
assert.equal(identity.status, 200);
assert.match(identity.headers['cache-control'], /immutable/);
for (const encoding of ['br', 'gzip']) {
  const compressed = await request(asset, { 'Accept-Encoding': encoding });
  assert.equal(compressed.status, 200);
  assert.match(compressed.headers.vary, /Accept-Encoding/i);
  if (compressed.headers['content-encoding']) {
    assert.equal(compressed.headers['content-encoding'], encoding);
    assert.deepEqual(encoding === 'br' ? brotliDecompressSync(compressed.bytes) : gunzipSync(compressed.bytes), identity.bytes);
  } else assert.deepEqual(compressed.bytes, identity.bytes);
}
const noCompression = await request(asset, { 'Accept-Encoding': 'br;q=0, gzip;q=0' });
assert.equal(noCompression.headers['content-encoding'], undefined);
const head = await request(asset, {}, 'HEAD');
assert.equal(head.status, 200);
assert.equal(head.bytes.length, 0);
assert.equal(Number(head.headers['content-length']), identity.bytes.length);
console.log('Static HTTP checks passed: shell, raw paths, API/auth boundaries, cache, CSP headers and compression.');
