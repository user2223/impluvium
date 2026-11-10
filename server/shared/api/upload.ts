import type { ApiBody, ApiResponse } from "./types";
import type { ApiRouteMap } from "./generated/routes";

type UploadKey =
  | "POST /api/v1/admin/game/image"
  | "POST /api/v1/admin/company/:id/icon"
  | "POST /api/v1/admin/company/:id/banner"
  | "POST /api/v1/admin/settings/logo";

type UploadPath<K extends UploadKey> = [ApiRouteMap[K]["path"]] extends [never]
  ? { path?: never }
  : { path: ApiRouteMap[K]["path"] };

export type UploadTarget = {
  [K in UploadKey]: ApiBody<K> extends FormData
    ? { key: K } & UploadPath<K>
    : never;
}[UploadKey];

export type GameUpload = ApiResponse<"POST /api/v1/admin/game/image">;
export type CompanyUpload = ApiResponse<
  | "POST /api/v1/admin/company/:id/icon"
  | "POST /api/v1/admin/company/:id/banner"
>;
export type LogoUpload = ApiResponse<"POST /api/v1/admin/settings/logo">;
