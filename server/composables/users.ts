import type { ApiResponse } from "~/shared/api/types";

export const useUsers = () =>
  useState<ApiResponse<"GET /api/v1/admin/users"> | undefined>(
    "users",
    () => undefined,
  );

export const fetchUsers = async () => {
  const users = useUsers();
  const newValue = await $apiFetch("GET /api/v1/admin/users");
  users.value = newValue;
  return newValue;
};
