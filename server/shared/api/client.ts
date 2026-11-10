import type { FetchHooks, FetchOptions } from "ofetch";
import type { ApiRouteMap } from "./generated/routes";

type ApiRouteKey = Extract<keyof ApiRouteMap, string>;
type EmptyOptions = Record<string, never>;

export type ApiFetchKey = {
  [K in ApiRouteKey]: ApiRouteMap[K]["kind"] extends
    | "json"
    | "json-or-empty"
    | "empty"
    | "binary"
    | "binary-or-empty"
    | "text"
    | "text-or-empty"
    ? K
    : never;
}[ApiRouteKey];

type Field<Name extends string, Value> = [Value] extends [never]
  ? { [K in Name]?: never }
  : EmptyOptions extends Value
    ? { [K in Name]?: Value }
    : { [K in Name]: Value };

// A validated object body is required even when all of its fields are optional.
// Only a body type that explicitly includes undefined permits omission.
type Body<Value> = [Value] extends [never]
  ? { body?: never }
  : undefined extends Value
    ? { body?: Value }
    : { body: Value };

type FixedOptions =
  | keyof FetchHooks
  | "method"
  | "params"
  | "responseType"
  | "baseURL"
  | "parseResponse"
  | "ignoreResponseError"
  | "dispatcher"
  | "agent";

export type ApiOptions<K extends ApiFetchKey> = Omit<
  FetchOptions,
  FixedOptions | "body" | "query" | "retryDelay"
> & { [Name in FixedOptions]?: never } & {
  failTitle?: string;
  retryDelay?: number;
} & Field<"path", ApiRouteMap[K]["path"]> &
  Field<"query", ApiRouteMap[K]["query"]> &
  Body<ApiRouteMap[K]["body"]>;

export type ApiArguments<K extends ApiFetchKey> =
  EmptyOptions extends ApiOptions<K>
    ? [options?: ApiOptions<K>]
    : [options: ApiOptions<K>];

export type ApiFetch = <K extends ApiFetchKey>(
  key: K,
  ...args: ApiArguments<NoInfer<K>>
) => Promise<ApiRouteMap[K]["response"]>;
