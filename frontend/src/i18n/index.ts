import i18n from "i18next";
import { initReactI18next } from "react-i18next";

import { getSystemLanguage, loadLanguagePreference, resolveLanguage } from "./language";
import { resources } from "./resources";

void i18n.use(initReactI18next).init({
  resources,
  lng: resolveLanguage(loadLanguagePreference(), getSystemLanguage()),
  fallbackLng: "zh-CN",
  interpolation: {
    escapeValue: false
  }
});

export { i18n };
