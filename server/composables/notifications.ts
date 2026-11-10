import type { NotificationModel } from "~/shared/api/types";

const ws = new WebSocketHandler("/api/v1/notifications/ws");

export const useNotifications = () =>
  useState<NotificationModel[]>("notifications", () => []);

ws.listen((e) => {
  const notification = JSON.parse(e) as NotificationModel;
  const notifications = useNotifications();
  notifications.value.push(notification);
});
