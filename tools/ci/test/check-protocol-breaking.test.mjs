import test from 'node:test';
import assert from 'node:assert/strict';
import { diagnostics } from '../check-protocol-breaking.mjs';
import { baseSelection } from '../protocol-base.mjs';

test('breaking base follows actual PR/push history and reports unavailable history explicitly', () => {
  const pr = 'a'.repeat(40), push = 'b'.repeat(40);
  assert.equal(baseSelection('pull_request', 'refs/pull/1', pr, push).sha, pr);
  assert.equal(baseSelection('push', 'refs/heads/main', pr, push).sha, push);
  assert.equal(baseSelection('workflow_dispatch', 'refs/heads/main', pr, push).status, 'unavailable');
  assert.equal(baseSelection('push', 'refs/heads/new', pr, '0'.repeat(40)).status, 'unavailable');
  assert.throws(() => baseSelection('pull_request', 'refs/pull/1', 'HEAD', push));
});
test('breaking diagnostics require structured file annotations, not any failing tool output', () => {
  const row = { path: 'impluvium/v2/auth.proto', type: 'FIELD_NO_DELETE', message: 'field deleted' };
  assert.deepEqual(diagnostics('', JSON.stringify(row)), [row]);
  assert.throws(() => diagnostics('', 'download failed'), /malformed/);
  assert.throws(() => diagnostics('', '{}'), /invalid/);
});
