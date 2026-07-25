import { Space, Typography } from "antd";
import type { ReactNode } from "react";

/**
 * Wraps a form control and renders its validation error as text below it.
 * The control stays in a fixed tree position whether or not an error shows,
 * so toggling an error never remounts it (which would drop focus mid-typing).
 */
export function ValidatedField({ error, children }: { error?: string; children: ReactNode }) {
  return (
    <Space orientation="vertical" size={4} className="pageFill">
      {children}
      {error ? <Typography.Text type="danger">{error}</Typography.Text> : null}
    </Space>
  );
}
