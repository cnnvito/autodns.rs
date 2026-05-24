import type { SelectOption } from "../shared/ui";

export type ThemePreference = "system" | "light" | "dark";

export const themeStorageKey = "autodns.theme";

export const themeOptions: SelectOption[] = [
  { value: "system", label: "跟随系统" },
  { value: "light", label: "亮色" },
  { value: "dark", label: "暗色" }
];

export function loadThemePreference(): ThemePreference {
  try {
    return normalizeTheme(window.localStorage.getItem(themeStorageKey) || "system");
  } catch {
    return "system";
  }
}

export function normalizeTheme(value: string): ThemePreference {
  if (value === "light" || value === "dark") {
    return value;
  }
  return "system";
}

export function applyThemePreference(theme: ThemePreference) {
  if (theme === "system") {
    document.documentElement.removeAttribute("data-theme");
  } else {
    document.documentElement.dataset.theme = theme;
  }
  try {
    window.localStorage.setItem(themeStorageKey, theme);
  } catch {
    // 主题偏好只是体验增强，本地存储不可用时不影响配置管理。
  }
}
