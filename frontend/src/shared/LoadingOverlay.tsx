import { Spin } from "antd";

type LoadingOverlayProps = {
  text: string;
  compact?: boolean;
};

export function LoadingOverlay({ text, compact = false }: LoadingOverlayProps) {
  return (
    <div className={`loadingOverlay ${compact ? "loadingOverlayCompact" : ""}`} role="status" aria-live="polite">
      <Spin size={compact ? "default" : "large"} />
      <span className="loadingOverlayText">{text}</span>
    </div>
  );
}
