import enUS from "antd/locale/en_US";
import zhCN from "antd/locale/zh_CN";
import type { Locale } from "antd/es/locale";

export type LanguagePreference = "system" | "zh-CN" | "en-US";
export type ResolvedLanguage = Exclude<LanguagePreference, "system">;

export const languageStorageKey = "autodns.language";

export function normalizeLanguage(value: string): LanguagePreference {
  if (value === "zh-CN" || value === "en-US") {
    return value;
  }
  return "system";
}

export function loadLanguagePreference(): LanguagePreference {
  try {
    return normalizeLanguage(window.localStorage.getItem(languageStorageKey) || "system");
  } catch {
    return "system";
  }
}

export function saveLanguagePreference(language: LanguagePreference) {
  try {
    window.localStorage.setItem(languageStorageKey, language);
  } catch {
    // Language preference is best-effort and should not block configuration management.
  }
}

export function getSystemLanguage(): ResolvedLanguage {
  const language = window.navigator.language.toLowerCase();
  return language.startsWith("zh") ? "zh-CN" : "en-US";
}

export function resolveLanguage(language: LanguagePreference, systemLanguage: ResolvedLanguage): ResolvedLanguage {
  return language === "system" ? systemLanguage : language;
}

export function antdLocaleFor(language: ResolvedLanguage): Locale {
  return language === "zh-CN" ? zhCN : enUS;
}
