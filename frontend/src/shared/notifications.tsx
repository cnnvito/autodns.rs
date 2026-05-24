import * as Toast from "@radix-ui/react-toast";
import { AlertTriangle, CheckCircle2, Info, X, XCircle } from "lucide-react";

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
    <Toast.Provider swipeDirection="right" duration={3800}>
      {notifications.map((item) => (
        <Toast.Root
          className={`notificationToast ${item.kind}`}
          defaultOpen
          key={item.id}
          onOpenChange={(open) => {
            if (!open) {
              onDismiss(item.id);
            }
          }}
        >
          <div className="notificationIcon" aria-hidden="true">
            <NotificationIcon kind={item.kind} />
          </div>
          <div className="notificationBody">
            <Toast.Title className="notificationTitle">{item.title}</Toast.Title>
            {item.description ? <Toast.Description className="notificationDescription">{item.description}</Toast.Description> : null}
          </div>
          <Toast.Close className="notificationClose" aria-label="关闭通知">
            <X size={14} />
          </Toast.Close>
        </Toast.Root>
      ))}
      <Toast.Viewport className="notificationViewport" />
    </Toast.Provider>
  );
}

function NotificationIcon({ kind }: { kind: NotificationKind }) {
  if (kind === "success") {
    return <CheckCircle2 size={18} />;
  }
  if (kind === "error") {
    return <XCircle size={18} />;
  }
  if (kind === "warning") {
    return <AlertTriangle size={18} />;
  }
  return <Info size={18} />;
}
