export type ThemePreference = "light" | "dark";

type SelectOption = {
  value: string;
  label: string;
};

export const themeStorageKey = "autodns.theme";

export const themeOptions: SelectOption[] = [
  { value: "light", label: "亮色" },
  { value: "dark", label: "暗色" }
];

export function loadThemePreference(): ThemePreference {
  try {
    return normalizeTheme(window.localStorage.getItem(themeStorageKey) || "light");
  } catch {
    return "light";
  }
}

export function normalizeTheme(value: string): ThemePreference {
  return value === "dark" ? "dark" : "light";
}

export function applyThemePreference(theme: ThemePreference) {
  try {
    window.localStorage.setItem(themeStorageKey, theme);
  } catch {
    // 主题偏好只是体验增强，本地存储不可用时不影响配置管理。
  }
}
