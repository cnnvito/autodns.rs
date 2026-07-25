import { Tooltip } from "antd";
import { QuestionCircleOutlined } from "@ant-design/icons";

/**
 * A "?" icon shown after a label; the descriptive text appears as a hover tooltip
 * instead of occupying layout space below the label. Renders nothing without a hint,
 * so callers can pass optional copy directly.
 */
export function HintTooltip({ hint }: { hint?: string }) {
  if (!hint) {
    return null;
  }
  return (
    <Tooltip title={hint}>
      <QuestionCircleOutlined className="labelHintIcon" />
    </Tooltip>
  );
}
