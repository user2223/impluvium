import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';
import { comparisonPolicy, validateNonComparable } from '../../web-api/non-comparable.mjs';

const entry = (path) => ({ method: 'GET', path, reason: 'The handler wrapper erases its response type to any.' });

test('Nitro comparison exclusions require unique method/path entries with reasons', () => {
  const first = entry('/first');
  assert.equal(validateNonComparable([first]).size, 1);
  assert.throws(() => validateNonComparable([first, first]), /Duplicate/);
  for (const invalid of [
    { ...first, method: 'get' }, { ...first, path: 'first' }, { ...first, path: '/first?query=1' },
    { ...first, reason: '' }, { ...first, reason: 'first\nsecond' }, { ...first, extra: true },
  ]) assert.throws(() => validateNonComparable([invalid]), /method, a path and a single-line reason/);
});

test('Nitro comparison exclusions may be removed but never added or restored', () => {
  const before = [entry('/first'), entry('/second')];
  assert.doesNotThrow(() => validateNonComparable(before, before));
  assert.doesNotThrow(() => validateNonComparable([before[0]], before));
  assert.doesNotThrow(() => validateNonComparable([], before));
  assert.throws(() => validateNonComparable([...before, entry('/third')], before), /may only shrink.*GET \/third/);
  assert.throws(() => validateNonComparable(before, [before[0]]), /may only shrink.*GET \/second/);
});

test('only documented root any responses may skip comparison and every entry must stay necessary', () => {
  const policy = comparisonPolicy([entry('/first'), entry('/removed')]);
  assert.deepEqual(policy.check('GET /first', true), { skip: true, problems: [] });
  assert.deepEqual(policy.check('GET /typed', false), { skip: false, problems: [] });
  const unlisted = policy.check('GET /new', true);
  assert.equal(unlisted.skip, false);
  assert.match(unlisted.problems.join('\n'), /without a documented/);
  const stale = policy.check('GET /first', false);
  assert.equal(stale.skip, false);
  assert.match(stale.problems.join('\n'), /no longer returns any/);
  assert.deepEqual(policy.finish(), ['Unused Nitro comparison exclusion: GET /removed']);
});

test('the checked-in exclusions are a subset of the original ten erased handlers', async () => {
  const original = new Set([
    'DELETE /api/v1/admin/auth/invitation',
    'DELETE /api/v1/admin/game/:id/versions',
    'DELETE /api/v1/admin/game/image',
    'DELETE /api/v1/admin/library/sources',
    'PATCH /api/v1/admin/library/sources',
    'PATCH /api/v1/admin/settings',
    'POST /api/v1/admin/import/game',
    'POST /api/v1/admin/library/sources',
    'POST /api/v1/auth/signin/simple',
    'POST /api/v1/auth/signup/simple',
  ]);
  const entries = JSON.parse(await readFile(new URL('../../web-api/non-comparable.json', import.meta.url), 'utf8'));
  for (const key of validateNonComparable(entries).keys()) assert.ok(original.has(key), key);
});
