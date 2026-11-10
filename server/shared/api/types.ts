import type { ApiRouteMap } from "./generated/routes";

export type ApiResponse<K extends keyof ApiRouteMap> =
  ApiRouteMap[K]["response"];
export type ApiBody<K extends keyof ApiRouteMap> = ApiRouteMap[K]["body"];
export type ApiQuery<K extends keyof ApiRouteMap> = ApiRouteMap[K]["query"];

export type UserModel = NonNullable<ApiResponse<"GET /api/v1/user">>;
export type UserProfile = ApiResponse<"GET /api/v1/user/:id">;
export type GameModel = ApiResponse<"GET /api/v1/store">["results"][number];
export type GameTagModel = ApiResponse<"GET /api/v1/store/tags">[number];
export type NotificationModel =
  ApiResponse<"GET /api/v1/notifications">[number];
export type ArticleModel = ApiResponse<"GET /api/v1/news">[number];
export type CompanyModel = ApiResponse<"GET /api/v1/admin/company">[number];
export type CollectionModel = ApiResponse<"GET /api/v1/collection">[number];
export type CollectionEntryModel = CollectionModel["entries"][number];
export type InvitationModel =
  ApiResponse<"GET /api/v1/admin/auth/invitation">[number];
export type AdminFetchGameType =
  ApiResponse<"GET /api/v1/admin/game/:id">["game"];
export type AdminLibraryGame =
  ApiResponse<"GET /api/v1/admin/library">["results"][number];
export type WorkingLibrarySource =
  ApiResponse<"GET /api/v1/admin/library/sources">[number];
export type GameMetadataSearchResult =
  ApiResponse<"GET /api/v1/admin/search/game">[number];
export type VersionGuess =
  ApiResponse<"GET /api/v1/admin/import/version/preload">[number];
export type ImportVersion = ApiBody<"POST /api/v1/admin/import/version">;
export type Settings = ApiResponse<"GET /api/v1/settings">;

// Socket messages use the same JSON forms as the existing server channels.
export interface SystemData {
  totalRam: number;
  freeRam: number;
  cpuLoad: number;
  cpuCores: number;
}

export type TaskGroup = ApiResponse<"GET /api/v1/admin/task">["other"][number];
export interface TaskMessage {
  id: string;
  name: string;
  success: boolean;
  progress: number;
  error: { title: string; description: string } | null | undefined;
  log: string[];
  reset?: boolean;
  actions: `${string}:${string}`[];
}

export interface TaskLog {
  time: string;
  msg: string;
  level: string;
  prefix?: string;
}
