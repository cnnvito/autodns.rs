import { Tooltip } from "antd";
import type { ReactNode } from "react";

/**
 * Wraps a table-cell control with a hover tooltip for its validation error.
 * The wrapper structure is identical with and without an error: swapping
 * between bare children and a Tooltip-wrapped subtree would remount the
 * control, which drops focus while the user is typing.
 */
export function FieldWithError({ error, children }: { error?: string; children: ReactNode }) {
  return (
    <Tooltip title={error} color="red">
      <span className="fieldErrorTooltip">{children}</span>
    </Tooltip>
  );
}
