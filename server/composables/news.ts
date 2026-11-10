import type { ApiQuery, ApiResponse } from "~/shared/api/types";

export const useNews = () =>
  useState<ApiResponse<"GET /api/v1/news"> | undefined>(
    "news",
    () => undefined,
  );

export const fetchNews = async (options?: {
  limit?: number;
  skip?: number;
  orderBy?: "asc" | "desc";
  tags?: string[];
  search?: string;
}) => {
  const query: ApiQuery<"GET /api/v1/news"> = {};
  if (options?.limit) query.limit = options.limit.toString();
  if (options?.skip) query.skip = options.skip.toString();
  if (options?.orderBy) query.order = options.orderBy;
  if (options?.tags?.length) query.tags = options.tags;
  if (options?.search) query.search = options.search;

  const news = useNews();
  const newValue = await $apiFetch("GET /api/v1/news", { query });
  news.value = newValue;
  return newValue;
};
