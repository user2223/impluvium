// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

// 64-bit integers travel as canonical decimal strings (server-api-types, module dec64).
// Canonical: digits only, no leading zeros (except "0"), no "+", "-" only for non-zero
// negatives, no whitespace, no exponent. Anything else is refused, as the server does.

/** An unsigned 64-bit integer as it appears on the wire. */
export type DecU64 = string;
/** A signed 64-bit integer as it appears on the wire. */
export type DecI64 = string;

export const U64_MAX = 18446744073709551615n;
export const I64_MIN = -9223372036854775808n;
export const I64_MAX = 9223372036854775807n;

const UNSIGNED = /^(0|[1-9][0-9]*)$/;
const SIGNED = /^(0|-?[1-9][0-9]*)$/;

function parse(
  text: unknown,
  pattern: RegExp,
  min: bigint,
  max: bigint,
  what: string,
): bigint {
  if (typeof text !== "string" || !pattern.test(text)) {
    throw new TypeError(`not a canonical ${what} decimal string`);
  }
  const value = BigInt(text);
  if (value < min || value > max) {
    throw new RangeError(`${what} decimal string out of range`);
  }
  return value;
}

function format(
  value: unknown,
  min: bigint,
  max: bigint,
  what: string,
): string {
  if (typeof value !== "bigint") {
    throw new TypeError(`${what} values must be bigint`);
  }
  if (value < min || value > max) {
    throw new RangeError(`${what} value out of range`);
  }
  return value.toString();
}

/** Reads a wire string; throws TypeError when not canonical and RangeError when out of range. */
export function parseU64(text: string): bigint {
  return parse(text, UNSIGNED, 0n, U64_MAX, "u64");
}

/** Reads a wire string; throws TypeError when not canonical and RangeError when out of range. */
export function parseI64(text: string): bigint {
  return parse(text, SIGNED, I64_MIN, I64_MAX, "i64");
}

/** Writes the canonical wire string; throws RangeError outside the u64 range. */
export function formatU64(value: bigint): DecU64 {
  return format(value, 0n, U64_MAX, "u64");
}

/** Writes the canonical wire string; throws RangeError outside the i64 range. */
export function formatI64(value: bigint): DecI64 {
  return format(value, I64_MIN, I64_MAX, "i64");
}

export function isU64(text: unknown): text is DecU64 {
  try {
    parse(text, UNSIGNED, 0n, U64_MAX, "u64");
    return true;
  } catch {
    return false;
  }
}

export function isI64(text: unknown): text is DecI64 {
  try {
    parse(text, SIGNED, I64_MIN, I64_MAX, "i64");
    return true;
  } catch {
    return false;
  }
}
