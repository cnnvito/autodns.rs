import { CloseOutlined } from "@ant-design/icons";
import { Alert, Button } from "antd";

export type NotificationKind = "success" | "error" | "warning" | "info";

export type AppNotification = {
  id: number;
  kind: NotificationKind;
  title: string;
  description?: string;
};

export function NotificationCenter({
  notifications,
  onDismiss
}: {
  notifications: AppNotification[];
  onDismiss: (id: number) => void;
}) {
  return (
    <div className="notificationViewport" role="region" aria-label="通知">
      {notifications.map((item) => (
        <Alert
          key={item.id}
          type={item.kind}
          title={item.title}
          description={item.description}
          showIcon
          action={
            <Button
              type="text"
              size="small"
              icon={<CloseOutlined />}
              aria-label="关闭通知"
              onClick={() => onDismiss(item.id)}
            />
          }
          role={item.kind === "error" ? "alert" : "status"}
        />
      ))}
    </div>
  );
}
