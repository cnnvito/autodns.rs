import { Card, Typography } from "antd";

/** Placeholder card shown while the managed config document is still loading. */
export function LoadingPanel({ title, text }: { title: string; text: string }) {
  return (
    <Card title={title}>
      <Typography.Text type="secondary">{text}</Typography.Text>
    </Card>
  );
}
