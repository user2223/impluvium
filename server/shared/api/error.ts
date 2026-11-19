// SPDX-FileCopyrightText: 2026 the impluvium contributors
// SPDX-License-Identifier: AGPL-3.0-only

import { type ErrorCodeName, errorCodeNames } from "./error-codes.ts";

/** An error body that passed validation against the server catalog. */
export interface ApiError {
  code: ErrorCodeName;
  message: string;
  requestId: string;
  fields?: { field: string; code: string }[];
}

export type ForeignReason =
  | "empty"
  | "not-json"
  | "not-object"
  | "code-not-string"
  | "unknown-code"
  | "message-not-string"
  | "message-too-long"
  | "request-id-invalid"
  | "bad-fields";

export type ApiErrorReading =
  | { kind: "api"; error: ApiError }
  | {
      kind: "foreign";
      status: number;
      /** Only ever taken from the response header, never from the body. */
      requestId: string | undefined;
      reason: ForeignReason;
    };

interface HeaderReader {
  get(name: string): string | null;
}

const MAX_MESSAGE = 1000;
const MAX_FIELDS = 32;
const MAX_FIELD_TEXT = 64;
const REQUEST_ID = /^[A-Za-z0-9._-]{1,64}$/;

function headerRequestId(headers: HeaderReader | undefined) {
  const value = headers?.get("x-request-id");
  return typeof value === "string" && REQUEST_ID.test(value)
    ? value
    : undefined;
}

function isPlainObject(value: unknown): value is Record<string, unknown> {
  if (value === null || typeof value !== "object" || Array.isArray(value)) {
    return false;
  }
  const prototype = Object.getPrototypeOf(value);
  return prototype === Object.prototype || prototype === null;
}

/**
 * Accepts a response body only when it is an error of the server catalog:
 * an object with a known `code`, a string `message`, a valid `request_id`
 * and, optionally, a list of `{ field, code }` strings. Anything else is
 * foreign (a proxy page, a plain text, a body of another protocol) and is
 * never shown or trusted. `body` may be the parsed value, or a string that
 * holds JSON.
 */
export function readApiError(
  status: number,
  body: unknown,
  headers?: HeaderReader,
): ApiErrorReading {
  const foreign = (reason: ForeignReason): ApiErrorReading => ({
    kind: "foreign",
    status,
    requestId: headerRequestId(headers),
    reason,
  });
  let value = body;
  if (value === undefined || value === "") return foreign("empty");
  if (typeof value === "string") {
    try {
      value = JSON.parse(value);
    } catch {
      return foreign("not-json");
    }
  }
  if (!isPlainObject(value)) return foreign("not-object");
  const { code, message, request_id: requestId, fields } = value;
  if (typeof code !== "string") return foreign("code-not-string");
  if (!errorCodeNames.has(code)) return foreign("unknown-code");
  if (typeof message !== "string") return foreign("message-not-string");
  if (message.length > MAX_MESSAGE) return foreign("message-too-long");
  if (typeof requestId !== "string" || !REQUEST_ID.test(requestId)) {
    return foreign("request-id-invalid");
  }
  const error: ApiError = {
    code: code as ErrorCodeName,
    message,
    requestId,
  };
  if (fields !== undefined) {
    if (
      !Array.isArray(fields) ||
      fields.length > MAX_FIELDS ||
      !fields.every(
        (entry) =>
          isPlainObject(entry) &&
          typeof entry.field === "string" &&
          typeof entry.code === "string" &&
          entry.field.length <= MAX_FIELD_TEXT &&
          entry.code.length <= MAX_FIELD_TEXT,
      )
    ) {
      return foreign("bad-fields");
    }
    error.fields = fields.map((entry) => ({
      field: entry.field as string,
      code: entry.code as string,
    }));
  }
  return { kind: "api", error };
}

interface FailureLike {
  data?: unknown;
  response?: { status?: unknown; _data?: unknown; headers?: HeaderReader };
}

/**
 * Reads the failure of an HTTP request (an ofetch error). Only a failure
 * that carries a real response counts; errors without one, such as network
 * errors or errors created by the application (including Nuxt `createError`
 * with a status code), give `undefined`.
 */
export function readFailure(error: unknown): ApiErrorReading | undefined {
  if (error === null || typeof error !== "object") return undefined;
  const failure = error as FailureLike;
  const response = failure.response;
  if (
    response === null ||
    typeof response !== "object" ||
    typeof response.status !== "number"
  ) {
    return undefined;
  }
  const headers = response.headers;
  return readApiError(
    response.status,
    failure.data ?? response._data,
    typeof headers?.get === "function" ? headers : undefined,
  );
}

const FALLBACK = "The request could not be completed. Please try again.";

/** The text for the failure notice; foreign bodies never contribute text. */
export function failureMessage(reading: ApiErrorReading | undefined): string {
  if (!reading) return FALLBACK;
  if (reading.kind === "api") return reading.error.message;
  const id = reading.requestId ? ` Request id: ${reading.requestId}.` : "";
  return `The server returned an unexpected response (HTTP ${reading.status}).${id}`;
}

/**
 * The text to show for a failed request: the message of an accepted catalog
 * error, a generic text with status and request id for any other HTTP
 * failure, and `fallback` when the failure has no HTTP status. The raw body
 * of a foreign response is never part of it. Works for any ofetch error.
 */
export function failureText(error: unknown, fallback: string): string {
  const reading = readFailure(error);
  return reading ? failureMessage(reading) : fallback;
}
