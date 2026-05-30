export type ThemePreference = "system" | "light" | "dark";

export const themeStorageKey = "autodns.theme";

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
  try {
    window.localStorage.setItem(themeStorageKey, theme);
  } catch {
    // Theme preference is best-effort and should not block configuration management.
  }
}
