import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import { catalogFile, clientFile, expected } from '../error-codes.mjs';
import { createBrowserTransport } from '../../../server/composables/browser-transport.ts';
import { errorCodes } from '../../../server/shared/api/error-codes.ts';
import { failureMessage, failureText, readApiError, readFailure } from '../../../server/shared/api/error.ts';

const valid = { code: 'not_found', message: 'The requested resource does not exist.', request_id: 'abc-0001' };
const headers = (values) => new Headers(values);

test('the client catalog equals the server catalog in order and content', () => {
  const catalog = JSON.parse(readFileSync(catalogFile, 'utf8'));
  assert.deepEqual(errorCodes.map(({ code, status }) => ({ code, status })),
    catalog.map(({ code, status }) => ({ code, status })));
  assert.equal(readFileSync(clientFile, 'utf8'), expected(), 'run node tools/web-api/error-codes.mjs');
});

test('a catalog error is accepted, from an object or from JSON text', () => {
  for (const body of [valid, JSON.stringify(valid)]) {
    assert.deepEqual(readApiError(404, body), {
      kind: 'api',
      error: { code: 'not_found', message: valid.message, requestId: 'abc-0001' },
    });
  }
});

test('field errors are accepted when well formed', () => {
  const body = { ...valid, code: 'invalid_request_body', fields: [{ field: 'username', code: 'too_short' }] };
  const reading = readApiError(400, body);
  assert.equal(reading.kind, 'api');
  assert.deepEqual(reading.error.fields, [{ field: 'username', code: 'too_short' }]);
  assert.deepEqual(readApiError(400, { ...valid, fields: [] }).error.fields, []);
});

const foreign = [
  ['empty', undefined], ['empty', ''],
  ['not-json', '<html><body>Bad gateway</body></html>'],
  ['not-json', 'plain text'], ['not-json', '{"code":'],
  ['not-object', null], ['not-object', []], ['not-object', 5], ['not-object', 'null'], ['not-object', '[1]'],
  ['not-object', new Map()],
  ['code-not-string', { ...valid, code: 5 }], ['code-not-string', { message: 'x', request_id: 'a' }],
  ['unknown-code', { ...valid, code: 'something_new' }],
  ['unknown-code', { ...valid, code: 'toString' }],
  ['unknown-code', { ...valid, code: 'NOT_FOUND' }],
  ['message-not-string', { ...valid, message: 5 }], ['message-not-string', { ...valid, message: { text: 'x' } }],
  ['message-not-string', { code: valid.code, request_id: 'a' }],
  ['message-too-long', { ...valid, message: 'x'.repeat(1001) }],
  ['request-id-invalid', { ...valid, request_id: 5 }], ['request-id-invalid', { code: valid.code, message: 'm' }],
  ['request-id-invalid', { ...valid, request_id: '<script>' }], ['request-id-invalid', { ...valid, request_id: 'a'.repeat(65) }],
  ['bad-fields', { ...valid, fields: 'x' }], ['bad-fields', { ...valid, fields: null }],
  ['bad-fields', { ...valid, fields: [1] }], ['bad-fields', { ...valid, fields: [{ field: 'a' }] }],
  ['bad-fields', { ...valid, fields: [{ field: 'a', code: 5 }] }],
  ['bad-fields', { ...valid, fields: Array.from({ length: 33 }, () => ({ field: 'a', code: 'b' })) }],
  ['bad-fields', { ...valid, fields: [{ field: 'a'.repeat(65), code: 'b' }] }],
  ['bad-fields', { ...valid, fields: [{ field: 'a', code: 'b'.repeat(65) }] }],
];
for (const [reason, body] of foreign) {
  test(`foreign body rejected as ${reason}: ${typeof body === 'string' ? body.slice(0, 20) : JSON.stringify(body)?.slice(0, 40)}`, () => {
    const reading = readApiError(502, body);
    assert.equal(reading.kind, 'foreign');
    assert.equal(reading.reason, reason);
    assert.equal(reading.status, 502);
    assert.equal(reading.requestId, undefined);
  });
}

test('the request id of a foreign response comes from the header only', () => {
  const body = { code: 'something_new', message: 'm', request_id: 'from-body' };
  assert.equal(readApiError(500, body, headers({ 'X-Request-Id': 'from-header' })).requestId, 'from-header');
  assert.equal(readApiError(500, body).requestId, undefined);
  assert.equal(readApiError(500, '<html>', headers({ 'X-Request-Id': 'bad id <b>' })).requestId, undefined);
  assert.equal(readApiError(500, '<html>', headers({ 'X-Request-Id': 'a'.repeat(65) })).requestId, undefined);
  assert.equal(readApiError(500, '<html>', headers({})).requestId, undefined);
  // An accepted error uses the id of its body.
  assert.equal(readApiError(404, valid, headers({ 'X-Request-Id': 'other' })).error.requestId, 'abc-0001');
});

test('failure messages show server text only for accepted errors', () => {
  assert.equal(failureMessage(readApiError(404, valid)), valid.message);
  const html = '<html><script>alert(1)</script></html>';
  const text = failureMessage(readApiError(502, html, headers({ 'X-Request-Id': 'abc-7' })));
  assert.equal(text, 'The server returned an unexpected response (HTTP 502). Request id: abc-7.');
  assert.equal(failureMessage(readApiError(502, html)), 'The server returned an unexpected response (HTTP 502).');
  assert.doesNotMatch(text, /html|script/);
  assert.equal(failureMessage(undefined), 'The request could not be completed. Please try again.');
});

test('readFailure reads ofetch-like errors and ignores failures without a status', () => {
  const error = { status: 404, data: valid, response: { status: 404, headers: headers({}) } };
  assert.equal(readFailure(error).kind, 'api');
  const fromResponse = { response: { status: 502, _data: '<html>', headers: headers({ 'x-request-id': 'r1' }) } };
  assert.deepEqual(readFailure(fromResponse), { kind: 'foreign', status: 502, requestId: 'r1', reason: 'not-json' });
  assert.equal(readFailure({ response: { status: 500 } }).reason, 'empty');
  // Errors made by the application are not HTTP failures, even with a status code.
  assert.equal(readFailure({ statusCode: 500 }), undefined);
  assert.equal(readFailure({ statusCode: 400, status: 400, message: 'Passkey sign-in cancelled.' }), undefined);
  assert.equal(failureText({ statusCode: 400, status: 400, message: 'Passkey sign-in cancelled.' }, 'fallback'), 'fallback');
  assert.equal(readFailure({ response: null, status: 500 }), undefined);
  assert.equal(readFailure({ response: { status: '500' } }), undefined);
  assert.equal(readFailure(new TypeError('network')), undefined);
  assert.equal(readFailure(null), undefined);
  assert.equal(readFailure('text'), undefined);
});

test('failureText works on getter-only ofetch errors and falls back without a status', () => {
  const error = new Error('x');
  for (const [name, value] of [['data', valid], ['response', { status: 404, headers: headers({}) }]]) {
    Object.defineProperty(error, name, { get: () => value });
  }
  assert.equal(failureText(error, 'fallback'), valid.message);
  const proxied = new Error('x');
  Object.defineProperty(proxied, 'response', { get: () => ({ status: 502, headers: headers({}) }) });
  Object.defineProperty(proxied, 'data', { get: () => '<html>' });
  assert.equal(failureText(proxied, 'fallback'), 'The server returned an unexpected response (HTTP 502).');
  assert.equal(failureText(new TypeError('network'), 'fallback'), 'fallback');
  assert.equal(failureText(undefined, 'fallback'), 'fallback');
});

test('the transport still turns 204 and empty successful bodies into null', async () => {
  for (const [status, data] of [[204, undefined], [200, ''], [200, undefined]]) {
    const transport = createBrowserTransport(async () => ({ status, _data: data }));
    assert.equal(await transport('/api/v1/x', { method: 'DELETE' }), null);
  }
});
