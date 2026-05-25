import type { HealthState } from "./types";

const healthStateLabels: Record<HealthState, string> = {
  healthy: "健康",
  unhealthy: "异常",
  unused: "未使用",
  unknown: "未知"
};

export function formatDate(value?: string): string {
  if (!value) {
    return "";
  }
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) {
    return "";
  }
  return date.toLocaleString();
}

export function errorMessage(err: unknown): string {
  if (err instanceof Error) {
    return err.message;
  }
  return String(err);
}

export function healthStateLabel(state: HealthState): string {
  return healthStateLabels[state] || healthStateLabels.unknown;
}
