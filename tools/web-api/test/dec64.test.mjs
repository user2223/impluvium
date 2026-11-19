// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

import assert from 'node:assert/strict';
import test from 'node:test';
import {
  I64_MAX, I64_MIN, U64_MAX, formatI64, formatU64, isI64, isU64, parseI64, parseU64,
} from '../../../server/shared/api/dec64.ts';

// The same cases as the Rust tests of server-api-types (module dec64).
const UNSIGNED_NOT_CANONICAL = ['', '-', '-1', '-0', '00', '01', '+1', '1e3', '1E3', '1.0', ' 1', '1 ', '1\n', '0x1',
  '1_000', '１', 'NaN', 'null'];
const SIGNED_NOT_CANONICAL = ['', '-', '--1', '-0', '-00', '-01', '00', '01', '+1', '+0', '1e3', '-1e3', ' -1', '-1 ', '- 1'];

test('unsigned boundaries round trip as canonical strings', () => {
  for (const value of [0n, 1n, 9007199254740993n, U64_MAX]) {
    assert.equal(formatU64(value), value.toString());
    assert.equal(parseU64(formatU64(value)), value);
  }
  assert.equal(formatU64(U64_MAX), '18446744073709551615');
});

test('signed boundaries round trip as canonical strings', () => {
  for (const value of [I64_MIN, -9007199254740993n, -1n, 0n, 1n, I64_MAX]) {
    assert.equal(parseI64(formatI64(value)), value);
  }
  assert.equal(formatI64(I64_MIN), '-9223372036854775808');
});

test('unsigned input other than the canonical form is refused', () => {
  for (const text of UNSIGNED_NOT_CANONICAL) {
    assert.throws(() => parseU64(text), TypeError, JSON.stringify(text));
    assert.equal(isU64(text), false);
  }
  for (const text of ['18446744073709551616', '99999999999999999999999']) {
    assert.throws(() => parseU64(text), RangeError, text);
    assert.equal(isU64(text), false);
  }
});

test('signed input other than the canonical form is refused', () => {
  for (const text of SIGNED_NOT_CANONICAL) {
    assert.throws(() => parseI64(text), TypeError, JSON.stringify(text));
    assert.equal(isI64(text), false);
  }
  for (const text of ['9223372036854775808', '-9223372036854775809']) {
    assert.throws(() => parseI64(text), RangeError, text);
  }
});

test('numbers are refused on both sides', () => {
  assert.throws(() => parseU64(1), TypeError);
  assert.throws(() => parseI64(1), TypeError);
  assert.throws(() => formatU64(1), TypeError);
  assert.throws(() => formatU64(-1n), RangeError);
  assert.throws(() => formatU64(U64_MAX + 1n), RangeError);
  assert.throws(() => formatI64(I64_MAX + 1n), RangeError);
  assert.throws(() => formatI64(I64_MIN - 1n), RangeError);
});
