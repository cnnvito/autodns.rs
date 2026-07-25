import { listen } from "@tauri-apps/api/event";
import { getVersion } from "@tauri-apps/api/app";
import { isPermissionGranted, requestPermission, sendNotification } from "@tauri-apps/plugin-notification";
import {
  ApiOutlined,
  CheckCircleOutlined,
  DashboardOutlined,
  DatabaseOutlined,
  HistoryOutlined,
  PlayCircleOutlined,
  ReloadOutlined,
  RollbackOutlined,
  SaveOutlined,
  SearchOutlined,
  SettingOutlined,
  StopOutlined
} from "@ant-design/icons";
import { App as AntdApp, Button, ConfigProvider, Layout, Menu, Modal, Space, Tag, Typography, notification, theme as antdTheme } from "antd";
import { Suspense, lazy, useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";

import {
  applySystemDns,
  checkUpstreamHealth,
  clearDnsCache,
  hideWindow,
  loadManagedConfig,
  loadPreferences,
  loadStatus,
  loadSystemDnsStatus,
  normalizeStatus,
  quitApp,
  restoreSystemDns,
  saveConfig,
  savePreferences,
  saveSystemDnsSettings,
  showMainWindow,
  startAutodns,
  stopAutodns,
  validateConfig
} from "../shared/api";
import { emptyConfigValidation, flattenValidationMessages, hasValidationErrors, validateDesktopConfig } from "../features/config/validation";
import {
  hasAutoSaveChanges,
  hasManualConfigChanges,
  mergeAutoSaveBaseline,
  mergeAutoSaveDocument
} from "../features/config/autosave";
import { errorMessage, formatDate, localizedMessageText } from "../shared/format";
import type { ConfigDocument, DesktopPreferences, DesktopStatus, HealthState, SystemDnsSettings, SystemDnsStatus } from "../shared/types";
import type { SettingsSection } from "../pages/SettingsPage";
import { LoadingOverlay } from "../shared/LoadingOverlay";
import {
  antdLocaleFor,
  getSystemLanguage,
  loadLanguagePreference,
  normalizeLanguage,
  resolveLanguage,
  saveLanguagePreference,
  type LanguagePreference,
  type ResolvedLanguage
} from "../i18n/language";
import { applyThemePreference, loadThemePreference, normalizeTheme, type ThemePreference } from "./theme";

type NavigationItem = {
  key: string;
  labelKey: string;
  icon: ReactNode;
};

type NotificationKind = "success" | "error" | "warning" | "info";
type AutoSaveState = "idle" | "waiting" | "saving" | "saved" | "blocked" | "error";

const notificationConfig = {
  placement: "bottomRight" as const,
  bottom: 18,
  duration: 4,
  maxCount: 4,
  pauseOnHover: true
};

const AUTO_SAVE_DEBOUNCE_MS = 700;

const defaultPreferences: DesktopPreferences = {
  closeBehavior: "ask",
  language: "system",
  historyEnabled: true,
  startAtLogin: false,
  startAtLoginSupported: false,
  traySupported: false,
  trayMessage: ""
};

const { Header, Sider, Content, Footer } = Layout;

const HistoryPage = lazy(() => import("../pages/HistoryPage").then((module) => ({ default: module.HistoryPage })));
const LookupPage = lazy(() => import("../pages/LookupPage").then((module) => ({ default: module.LookupPage })));
const OverviewPage = lazy(() => import("../pages/OverviewPage").then((module) => ({ default: module.OverviewPage })));
const RulesPage = lazy(() => import("../pages/RulesPage").then((module) => ({ default: module.RulesPage })));
const SettingsPage = lazy(() => import("../pages/SettingsPage").then((module) => ({ default: module.SettingsPage })));
const UpstreamsPage = lazy(() => import("../pages/UpstreamsPage").then((module) => ({ default: module.UpstreamsPage })));

const navigationItems: NavigationItem[] = [
  { key: "overview", labelKey: "nav.overview", icon: <DashboardOutlined /> },
  { key: "rules", labelKey: "nav.rules", icon: <DatabaseOutlined /> },
  { key: "upstreams", labelKey: "nav.upstreams", icon: <ApiOutlined /> },
  { key: "lookup", labelKey: "nav.lookup", icon: <SearchOutlined /> },
  { key: "history", labelKey: "nav.history", icon: <HistoryOutlined /> },
  { key: "settings", labelKey: "nav.settings", icon: <SettingOutlined /> }
];

function applyOptimisticSystemDnsSettings(status: SystemDnsStatus, settings: SystemDnsSettings): SystemDnsStatus {
  const selectedAdapterIds = new Set(settings.selectedAdapterIds);
  return {
    ...status,
    settings,
    adapters: status.adapters.map((adapter) => ({
      ...adapter,
      selected: selectedAdapterIds.has(adapter.id)
    }))
  };
}

function needsRuntimeRestart(current: ConfigDocument | null, saved: ConfigDocument | null): boolean {
  if (!current || !saved) {
    return false;
  }
  const currentServer = current.config.server;
  const savedServer = saved.config.server;
  return currentServer.mode !== savedServer.mode
    || currentServer.listen !== savedServer.listen
    || currentServer.tlsSource !== savedServer.tlsSource
    || currentServer.certFile !== savedServer.certFile
    || currentServer.keyFile !== savedServer.keyFile
    || currentServer.certPem !== savedServer.certPem
    || currentServer.keyPem !== savedServer.keyPem
    || (currentServer.mode === "doh" && currentServer.path !== savedServer.path);
}

function enqueueSerial<T>(queue: { current: Promise<unknown> }, task: () => Promise<T>): Promise<T> {
  const next = queue.current.catch(() => undefined).then(task);
  queue.current = next.catch(() => undefined);
  return next;
}

function getSystemDarkPreference(): boolean {
  return window.matchMedia?.("(prefers-color-scheme: dark)").matches ?? false;
}

export function App() {
  const { t, i18n } = useTranslation();
  const [status, setStatus] = useState<DesktopStatus | null>(null);
  const [configDoc, setConfigDoc] = useState<ConfigDocument | null>(null);
  const [savedConfigDoc, setSavedConfigDoc] = useState<ConfigDocument | null>(null);
  const [busy, setBusy] = useState(false);
  const [busyText, setBusyText] = useState("");
  const [initializing, setInitializing] = useState(true);
  const [theme, setTheme] = useState<ThemePreference>(() => loadThemePreference());
  const [systemDark, setSystemDark] = useState(() => getSystemDarkPreference());
  const [language, setLanguage] = useState<LanguagePreference>(() => loadLanguagePreference());
  const [systemLanguage, setSystemLanguage] = useState<ResolvedLanguage>(() => getSystemLanguage());
  const [preferences, setPreferences] = useState<DesktopPreferences>(defaultPreferences);
  const [systemDns, setSystemDns] = useState<SystemDnsStatus | null>(null);
  const [systemDnsLoading, setSystemDnsLoading] = useState(false);
  const [activeTab, setActiveTab] = useState("overview");
  const [settingsSection, setSettingsSection] = useState<SettingsSection>("general");
  const [closePromptOpen, setClosePromptOpen] = useState(false);
  const [appVersion, setAppVersion] = useState("");
  const [checkingUpstreams, setCheckingUpstreams] = useState<Set<string>>(() => new Set());
  const [autoSaveState, setAutoSaveState] = useState<AutoSaveState>("idle");
  const [quitting, setQuitting] = useState(false);
  const lastRuntimeError = useRef("");
  const lastUpstreamHealth = useRef<Map<string, HealthState> | null>(null);
  const systemNotificationPermission = useRef<boolean | null>(null);
  const configDocRef = useRef<ConfigDocument | null>(null);
  const savedConfigDocRef = useRef<ConfigDocument | null>(null);
  const configSaveQueueRef = useRef<Promise<unknown>>(Promise.resolve());
  const autoSaveTimerRef = useRef<number | undefined>(undefined);
  const autoSaveRevisionRef = useRef(0);
  const autoSaveTaskRef = useRef<Promise<boolean> | null>(null);
  const configSaveFlushRef = useRef(false);
  const configRequestIdRef = useRef(0);
  const mutationLockRef = useRef(false);
  const quitHandlerRef = useRef<() => void>(() => undefined);
  const preferencesRef = useRef<DesktopPreferences>(defaultPreferences);
  const persistedPreferencesRef = useRef<DesktopPreferences>(defaultPreferences);
  const preferencesSaveQueueRef = useRef<Promise<unknown>>(Promise.resolve());
  const preferencesSaveRevisionRef = useRef(0);
  const systemDnsRef = useRef<SystemDnsStatus | null>(null);
  const persistedSystemDnsRef = useRef<SystemDnsStatus | null>(null);
  const systemDnsSaveQueueRef = useRef<Promise<unknown>>(Promise.resolve());
  const systemDnsSaveRevisionRef = useRef(0);
  const systemDnsSaveFailedRef = useRef(false);
  const pendingSystemDnsSave = useRef(0);
  const systemDnsLoadingRef = useRef(false);
  const systemDnsAdaptersRequested = useRef(false);
  const [notificationApi, notificationContextHolder] = notification.useNotification(notificationConfig);
  const notificationApiRef = useRef(notificationApi);
  notificationApiRef.current = notificationApi;
  configDocRef.current = configDoc;
  systemDnsRef.current = systemDns;
  quitHandlerRef.current = () => {
    void handleQuitApp();
  };

  useEffect(() => {
    let secondFrame: number | undefined;
    const firstFrame = window.requestAnimationFrame(() => {
      secondFrame = window.requestAnimationFrame(() => {
        void showMainWindow().catch(() => undefined);
      });
    });
    return () => {
      window.cancelAnimationFrame(firstFrame);
      if (secondFrame !== undefined) {
        window.cancelAnimationFrame(secondFrame);
      }
    };
  }, []);

  useEffect(() => {
    let cancelled = false;
    getVersion()
      .then((version) => {
        if (!cancelled) {
          setAppVersion(version);
        }
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
    };
  }, []);

  const notify = useCallback((kind: NotificationKind, title: string, description?: string) => {
    notificationApiRef.current.open({
      type: kind,
      message: title,
      description,
      duration: kind === "error" ? 0 : 4
    });
  }, []);

  const translateError = useCallback((err: unknown) => errorMessage(err, (key, values) => t(key, values)), [t]);

  const notifyError = useCallback((title: string, err: unknown) => {
    notify("error", title, translateError(err));
  }, [notify, translateError]);
  const notifyErrorRef = useRef(notifyError);
  const translateRef = useRef(t);
  notifyErrorRef.current = notifyError;
  translateRef.current = t;

  const notifySystem = useCallback(async (title: string, body: string) => {
    try {
      let granted = systemNotificationPermission.current;
      if (granted === null) {
        granted = await isPermissionGranted();
        if (!granted) {
          granted = await requestPermission() === "granted";
        }
        systemNotificationPermission.current = granted;
      }
      if (granted) {
        sendNotification({ title, body });
      }
    } catch {
      systemNotificationPermission.current = null;
    }
  }, []);

  async function refreshSystemDns(force = false) {
    if (systemDnsLoadingRef.current) {
      return;
    }
    systemDnsLoadingRef.current = true;
    setSystemDnsLoading(true);
    try {
      const nextSystemDns = await loadSystemDnsStatus(force);
      if (pendingSystemDnsSave.current === 0) {
        systemDnsSaveFailedRef.current = false;
        setSystemDns(nextSystemDns);
        systemDnsRef.current = nextSystemDns;
        persistedSystemDnsRef.current = nextSystemDns;
      }
    } finally {
      systemDnsLoadingRef.current = false;
      setSystemDnsLoading(false);
    }
  }

  useEffect(() => {
    bootstrap()
      .catch((err: unknown) => notifyErrorRef.current(translateRef.current("notifications.bootstrapFailed"), err))
      .finally(() => setInitializing(false));
  }, []);

  async function bootstrap() {
    const [doc, prefs, nextStatus, nextSystemDns] = await Promise.all([
      loadManagedConfig(),
      loadPreferences(),
      loadStatus(),
      loadSystemDnsStatus(false)
    ]);
    setConfigDoc(doc);
    setSavedConfigDoc(doc);
    configDocRef.current = doc;
    savedConfigDocRef.current = doc;
    setPreferences(prefs);
    preferencesRef.current = prefs;
    persistedPreferencesRef.current = prefs;
    setLanguage(normalizeLanguage(prefs.language));
    setStatus(nextStatus);
    setSystemDns(nextSystemDns);
    systemDnsRef.current = nextSystemDns;
    persistedSystemDnsRef.current = nextSystemDns;
  }

  useEffect(() => {
    let unlistenStatus: (() => void) | undefined;
    listen<DesktopStatus>("desktop:status", (event) => {
      setStatus(normalizeStatus(event.payload));
    }).then((nextUnlisten) => {
      unlistenStatus = nextUnlisten;
    }).catch(() => undefined);
    return () => {
      unlistenStatus?.();
    };
  }, []);

  useEffect(() => {
    if (activeTab === "settings" && settingsSection === "system-dns" && !systemDnsAdaptersRequested.current) {
      systemDnsAdaptersRequested.current = true;
      refreshSystemDns(true).catch((err: unknown) => notifyError(t("notifications.systemDnsStatusFailed"), err));
    }
  }, [activeTab, notifyError, settingsSection, t]);

  useEffect(() => {
    applyThemePreference(theme);
  }, [theme]);

  const resolvedLanguage = resolveLanguage(language, systemLanguage);

  useEffect(() => {
    saveLanguagePreference(language);
  }, [language]);

  useEffect(() => {
    if (preferences.language !== language) {
      handlePreferencesChange({ language }).catch(() => undefined);
    }
  }, [language]);

  useEffect(() => {
    if (i18n.language !== resolvedLanguage) {
      void i18n.changeLanguage(resolvedLanguage);
    }
  }, [i18n, resolvedLanguage]);

  useEffect(() => {
    const media = window.matchMedia?.("(prefers-color-scheme: dark)");
    if (!media) {
      return;
    }
    const syncSystemTheme = () => setSystemDark(media.matches);
    syncSystemTheme();
    media.addEventListener("change", syncSystemTheme);
    return () => media.removeEventListener("change", syncSystemTheme);
  }, []);

  useEffect(() => {
    const syncSystemLanguage = () => setSystemLanguage(getSystemLanguage());
    window.addEventListener("languagechange", syncSystemLanguage);
    return () => window.removeEventListener("languagechange", syncSystemLanguage);
  }, []);

  useEffect(() => {
    const runtimeError = status?.lastErrorMessage
      ? localizedMessageText(status.lastErrorMessage, (key, values) => t(key, values))
      : status?.lastError || "";
    if (runtimeError && runtimeError !== lastRuntimeError.current) {
      notify("error", t("notifications.runtimeError"), runtimeError);
    }
    lastRuntimeError.current = runtimeError;
  }, [notify, status?.lastError, status?.lastErrorMessage, t]);

  useEffect(() => {
    if (!status?.running) {
      lastUpstreamHealth.current = null;
      return;
    }

    const nextHealth = new Map(status.upstreamHealth.map((item) => [item.name, item.health]));
    const previousHealth = lastUpstreamHealth.current;
    if (previousHealth) {
      for (const item of status.upstreamHealth) {
        const previous = previousHealth.get(item.name);
        if (previous === "healthy" && item.health === "unhealthy") {
          const error = item.lastErrorMessage
            ? localizedMessageText(item.lastErrorMessage, (key, values) => t(key, values))
            : item.lastError || "";
          void notifySystem(
            t("notifications.upstreamUnhealthy"),
            error
              ? t("notifications.upstreamUnhealthyDescriptionWithError", { name: item.name, error })
              : t("notifications.upstreamUnhealthyDescription", { name: item.name })
          );
        } else if (previous === "unhealthy" && item.health === "healthy") {
          void notifySystem(
            t("notifications.upstreamRecovered"),
            t("notifications.upstreamRecoveredDescription", { name: item.name })
          );
        }
      }
    }
    lastUpstreamHealth.current = nextHealth;
  }, [notifySystem, status?.running, status?.upstreamHealth, t]);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    listen<"ask" | "quit">("desktop:close-requested", (event) => {
      if (event.payload === "quit") {
        quitHandlerRef.current();
        return;
      }
      setClosePromptOpen(true);
    }).then((nextUnlisten) => {
      unlisten = nextUnlisten;
    }).catch(() => undefined);
    return () => unlisten?.();
  }, []);

  const running = status?.running ?? false;
  const validation = useMemo(() => configDoc ? validateDesktopConfig(configDoc.config, (key, values) => t(key, values)) : emptyConfigValidation(), [configDoc, t]);
  const validationMessages = useMemo(() => flattenValidationMessages(validation), [validation]);
  const validationErrorCount = validationMessages.length;
  const lastStarted = useMemo(() => formatDate(status?.startedAt, resolvedLanguage), [status?.startedAt, resolvedLanguage]);
  const restartRequired = useMemo(() => needsRuntimeRestart(configDoc, savedConfigDoc), [configDoc, savedConfigDoc]);
  const autoDirty = useMemo(() => hasAutoSaveChanges(configDoc, savedConfigDoc), [configDoc, savedConfigDoc]);
  const manualDirty = useMemo(() => hasManualConfigChanges(configDoc, savedConfigDoc), [configDoc, savedConfigDoc]);
  const autoSaveDocument = useMemo(
    () => configDoc && savedConfigDoc ? mergeAutoSaveDocument(savedConfigDoc, configDoc) : null,
    [configDoc, savedConfigDoc]
  );
  const autoSaveValidation = useMemo(
    () => autoSaveDocument
      ? validateDesktopConfig(autoSaveDocument.config, (key, values) => t(key, values))
      : emptyConfigValidation(),
    [autoSaveDocument, t]
  );
  const autoSaveEligible = autoDirty && Boolean(autoSaveDocument) && !hasValidationErrors(autoSaveValidation) && !busy && !quitting;
  const effectiveDark = theme === "system" ? systemDark : theme === "dark";
  const antdLocale = useMemo(() => antdLocaleFor(resolvedLanguage), [resolvedLanguage]);
  const menuItems = useMemo(() => navigationItems.map((item) => ({
    key: item.key,
    label: (
      <span className="desktopNavLabel">
        {item.icon}
        <span>{t(item.labelKey)}</span>
      </span>
    )
  })), [t]);
  const languageOptions = useMemo(() => [
    { value: "system", label: t("settings.languageSystem") },
    { value: "zh-CN", label: t("settings.languageZhCN") },
    { value: "en-US", label: t("settings.languageEnUS") }
  ], [t]);
  const themeOptions = useMemo(() => [
    { value: "system", label: t("settings.themeSystem") },
    { value: "light", label: t("settings.themeLight") },
    { value: "dark", label: t("settings.themeDark") }
  ], [t]);
  const listenLine = running ? `${status?.listen || t("status.listenLocal")} · ${(status?.mode || "udp").toUpperCase()}` : t("status.serviceNotRunning");
  const healthyUpstreams = status?.upstreamHealth.filter((item) => item.health === "healthy").length ?? 0;
  const unhealthyUpstreams = status?.upstreamHealth.filter((item) => item.health === "unhealthy").length ?? 0;
  const systemDnsState = systemDnsLoading
    ? t("status.systemDnsLoading")
    : systemDns?.settings.enabled
      ? t("status.systemDnsApplyingAllowed")
      : systemDns?.supported
        ? t("status.systemDnsUnmanaged")
        : t("status.systemDnsUnavailable");
  const runtimeStatusError = status?.lastErrorMessage
    ? localizedMessageText(status.lastErrorMessage, (key, values) => t(key, values))
    : status?.lastError || "";
  const dirtyHint = running
    ? restartRequired
      ? t("config.dirtyHintRestart")
      : t("config.dirtyHintHotReload")
    : t("config.dirtyHintStopped");
  const configStatusText = manualDirty
    ? t("config.unsaved")
    : autoSaveState === "waiting"
      ? t("config.autoSaveWaiting")
      : autoSaveState === "saving"
        ? t("config.autoSaving")
        : autoSaveState === "blocked"
          ? t("config.autoSaveBlocked")
          : autoSaveState === "error"
            ? t("config.autoSaveFailed")
            : autoSaveState === "saved"
              ? t("config.autoSaved")
              : t("config.saved");
  const configStatusType = manualDirty || autoSaveState === "error" || autoSaveState === "blocked" ? "warning" : "secondary";
  const workspaceLoadingText = initializing ? t("busy.loadingConfig") : busy ? busyText || t("busy.processing") : "";

  const handleConfigDocChange = useCallback((doc: ConfigDocument) => {
    setConfigDoc(doc);
  }, []);

  function commitSavedConfig(doc: ConfigDocument) {
    savedConfigDocRef.current = doc;
    setSavedConfigDoc(doc);
  }

  function cancelScheduledAutoSave() {
    autoSaveRevisionRef.current += 1;
    if (autoSaveTimerRef.current !== undefined) {
      window.clearTimeout(autoSaveTimerRef.current);
      autoSaveTimerRef.current = undefined;
    }
  }

  function runAutoSaveNow(revision = autoSaveRevisionRef.current): Promise<boolean> {
    const current = configDocRef.current;
    const baseline = savedConfigDocRef.current;
    if (!current || !baseline || revision !== autoSaveRevisionRef.current || !hasAutoSaveChanges(current, baseline)) {
      return Promise.resolve(true);
    }

    const snapshot = mergeAutoSaveDocument(baseline, current);
    const snapshotValidation = validateDesktopConfig(
      snapshot.config,
      (key, values) => translateRef.current(key, values)
    );
    if (hasValidationErrors(snapshotValidation)) {
      setAutoSaveState("blocked");
      return Promise.resolve(true);
    }
    const requestId = configRequestIdRef.current + 1;
    configRequestIdRef.current = requestId;
    setAutoSaveState("saving");

    const task = enqueueSerial(configSaveQueueRef, async () => {
      try {
        const result = await saveConfig(snapshot);
        const persistedBaseline = savedConfigDocRef.current;
        commitSavedConfig(persistedBaseline ? mergeAutoSaveBaseline(persistedBaseline, snapshot) : snapshot);
        if (requestId === configRequestIdRef.current && revision === autoSaveRevisionRef.current && !quitting) {
          setStatus(result.status);
          setAutoSaveState("saved");
        }
        return true;
      } catch (err) {
        if (
          requestId === configRequestIdRef.current
          && (revision === autoSaveRevisionRef.current || configSaveFlushRef.current)
        ) {
          setAutoSaveState("error");
          notifyErrorRef.current(translateRef.current("notifications.saveFailed"), err);
        }
        return false;
      }
    });
    autoSaveTaskRef.current = task;
    void task.then(() => {
      if (autoSaveTaskRef.current === task) {
        autoSaveTaskRef.current = null;
      }
    });
    return task;
  }

  async function flushPendingConfigSaves(): Promise<boolean> {
    configSaveFlushRef.current = true;
    try {
      let success = true;
      if (autoSaveTimerRef.current !== undefined) {
        window.clearTimeout(autoSaveTimerRef.current);
        autoSaveTimerRef.current = undefined;
      }
      if (autoSaveTaskRef.current) {
        success = await autoSaveTaskRef.current;
      }
      const current = configDocRef.current;
      const baseline = savedConfigDocRef.current;
      if (current && baseline && hasAutoSaveChanges(current, baseline)) {
        success = await runAutoSaveNow(autoSaveRevisionRef.current);
      }
      await configSaveQueueRef.current;
      return success;
    } finally {
      configSaveFlushRef.current = false;
    }
  }

  useEffect(() => {
    cancelScheduledAutoSave();

    if (!autoDirty) {
      setAutoSaveState((current) => current === "saved" ? current : "idle");
      return;
    }

    if (!autoSaveEligible) {
      setAutoSaveState(hasValidationErrors(autoSaveValidation) ? "blocked" : "idle");
      return;
    }

    const revision = autoSaveRevisionRef.current;
    setAutoSaveState("waiting");
    autoSaveTimerRef.current = window.setTimeout(() => {
      autoSaveTimerRef.current = undefined;
      if (!configDocRef.current || revision !== autoSaveRevisionRef.current) {
        return;
      }

      void runAutoSaveNow(revision);
    }, AUTO_SAVE_DEBOUNCE_MS);

    return () => {
      if (autoSaveTimerRef.current !== undefined) {
        window.clearTimeout(autoSaveTimerRef.current);
        autoSaveTimerRef.current = undefined;
      }
    };
  }, [autoDirty, autoSaveEligible, autoSaveValidation, busy, configDoc, manualDirty, quitting, savedConfigDoc]);

  useEffect(() => () => {
    cancelScheduledAutoSave();
  }, []);

  function beginBusy(text: string) {
    setBusyText(text);
    setBusy(true);
  }

  function finishBusy() {
    setBusy(false);
    setBusyText("");
  }

  async function handleStart() {
    if (mutationLockRef.current) {
      return;
    }
    mutationLockRef.current = true;
    beginBusy(t("busy.startingService"));
    try {
      await flushPendingConfigSaves();
      const nextStatus = await startAutodns("");
      setStatus(nextStatus);
      notify("success", t("notifications.serviceStarted"), nextStatus.listen || t("notifications.serviceStartedDescription"));
    } catch (err) {
      notifyError(t("notifications.serviceStartFailed"), err);
    } finally {
      mutationLockRef.current = false;
      finishBusy();
    }
  }

  async function handleValidateConfig() {
    if (!configDoc) {
      return;
    }
    if (hasValidationErrors(validation)) {
      notify("warning", t("notifications.validateRejected"), validationMessages.slice(0, 3).join("\n"));
      return;
    }
    beginBusy(t("busy.validatingConfig"));
    try {
      await validateConfig(configDoc.config);
      notify("success", t("notifications.validatePassed"), t("notifications.validatePassedDescription"));
    } catch (err) {
      notifyError(t("notifications.validateFailed"), err);
    } finally {
      finishBusy();
    }
  }

  async function handleSaveConfig() {
    if (!configDoc || mutationLockRef.current) {
      return;
    }
    if (hasValidationErrors(validation)) {
      notify("warning", t("notifications.validateRequired"), validationMessages.slice(0, 3).join("\n"));
      return;
    }
    const snapshot = configDoc;
    cancelScheduledAutoSave();
    const requestId = configRequestIdRef.current + 1;
    configRequestIdRef.current = requestId;
    mutationLockRef.current = true;
    beginBusy(restartRequired ? t("busy.savingConfigAndRestarting") : t("busy.savingConfig"));
    try {
      const result = await enqueueSerial(configSaveQueueRef, () => saveConfig(snapshot));
      if (requestId !== configRequestIdRef.current) {
        return;
      }
      commitSavedConfig(snapshot);
      setStatus(result.status);
      setAutoSaveState("idle");
      if (result.action === "restarted") {
        notify("success", t("notifications.configSavedRestarted"), t("notifications.configSavedRestartedDescription"));
      } else if (result.action === "hotReloaded") {
        notify("success", t("notifications.configSavedHotReloaded"), t("notifications.configSavedHotReloadedDescription"));
      } else {
        notify("success", t("notifications.configSaved"), t("notifications.configSavedDescription"));
      }
    } catch (err) {
      notifyError(t("notifications.saveFailed"), err);
    } finally {
      mutationLockRef.current = false;
      finishBusy();
    }
  }

  async function handleStop() {
    if (mutationLockRef.current) {
      return;
    }
    mutationLockRef.current = true;
    beginBusy(t("busy.stoppingService"));
    try {
      await flushPendingConfigSaves();
      const nextStatus = await stopAutodns();
      setStatus(nextStatus);
      notify("info", t("notifications.serviceStopped"));
    } catch (err) {
      notifyError(t("notifications.serviceStopFailed"), err);
    } finally {
      mutationLockRef.current = false;
      finishBusy();
    }
  }

  async function handleRestart() {
    if (!running || mutationLockRef.current) {
      return;
    }
    mutationLockRef.current = true;
    beginBusy(t("busy.restartingService"));
    try {
      await flushPendingConfigSaves();
      await stopAutodns();
      const nextStatus = await startAutodns("");
      setStatus(nextStatus);
      notify("success", t("notifications.serviceRestarted"), t("notifications.serviceRestartedDescription"));
    } catch (err) {
      notifyError(t("notifications.restartFailed"), err);
    } finally {
      mutationLockRef.current = false;
      finishBusy();
    }
  }

  function handleDiscardConfig() {
    if (mutationLockRef.current) {
      return;
    }
    const persisted = savedConfigDocRef.current;
    if (!persisted) {
      return;
    }
    const current = configDocRef.current;
    const next = current ? mergeAutoSaveDocument(persisted, current) : persisted;
    cancelScheduledAutoSave();
    setConfigDoc(next);
    configDocRef.current = next;
    setAutoSaveState("idle");
    notify("info", t("notifications.configDiscarded"), t("notifications.configDiscardedDescription"));
  }

  function handlePreferencesChange(patch: Partial<DesktopPreferences>): Promise<void> {
    const next = { ...preferencesRef.current, ...patch };
    const revision = preferencesSaveRevisionRef.current + 1;
    preferencesSaveRevisionRef.current = revision;
    preferencesRef.current = next;
    setPreferences(next);

    return enqueueSerial(preferencesSaveQueueRef, async () => {
      try {
        const saved = await savePreferences(next);
        persistedPreferencesRef.current = saved;
        if (revision === preferencesSaveRevisionRef.current) {
          preferencesRef.current = saved;
          setPreferences(saved);
        }
      } catch (err) {
        if (revision === preferencesSaveRevisionRef.current) {
          const persisted = persistedPreferencesRef.current;
          preferencesRef.current = persisted;
          setPreferences(persisted);
          notifyError(t("notifications.desktopBehaviorSaveFailed"), err);
        }
      }
    });
  }

  function handleSystemDnsSettingsChange(settings: SystemDnsSettings): Promise<void> {
    const current = systemDnsRef.current;
    const optimistic = current ? applyOptimisticSystemDnsSettings(current, settings) : current;
    const revision = systemDnsSaveRevisionRef.current + 1;
    systemDnsSaveRevisionRef.current = revision;
    pendingSystemDnsSave.current += 1;
    if (optimistic) {
      systemDnsRef.current = optimistic;
      setSystemDns(optimistic);
    }

    return enqueueSerial(systemDnsSaveQueueRef, async () => {
      try {
        const saved = await saveSystemDnsSettings(settings);
        persistedSystemDnsRef.current = saved;
        if (revision === systemDnsSaveRevisionRef.current) {
          systemDnsSaveFailedRef.current = false;
          systemDnsRef.current = saved;
          setSystemDns(saved);
        }
      } catch (err) {
        if (revision === systemDnsSaveRevisionRef.current) {
          systemDnsSaveFailedRef.current = true;
          const persisted = persistedSystemDnsRef.current;
          systemDnsRef.current = persisted;
          setSystemDns(persisted);
          notifyError(t("notifications.systemDnsSaveFailed"), err);
        }
      } finally {
        pendingSystemDnsSave.current = Math.max(0, pendingSystemDnsSave.current - 1);
      }
    });
  }

  async function handleApplySystemDns() {
    if (mutationLockRef.current) {
      return;
    }
    mutationLockRef.current = true;
    beginBusy(t("busy.applyingSystemDns"));
    try {
      await systemDnsSaveQueueRef.current;
      if (systemDnsSaveFailedRef.current) {
        return;
      }
      const next = await applySystemDns();
      systemDnsSaveFailedRef.current = false;
      systemDnsRef.current = next;
      persistedSystemDnsRef.current = next;
      setSystemDns(next);
      notify("success", t("notifications.systemDnsApplied"));
    } catch (err) {
      notifyError(t("notifications.systemDnsApplyFailed"), err);
    } finally {
      mutationLockRef.current = false;
      finishBusy();
    }
  }

  async function handleRestoreSystemDns() {
    if (mutationLockRef.current) {
      return;
    }
    mutationLockRef.current = true;
    beginBusy(t("busy.restoringSystemDns"));
    try {
      await systemDnsSaveQueueRef.current;
      const next = await restoreSystemDns();
      systemDnsRef.current = next;
      persistedSystemDnsRef.current = next;
      setSystemDns(next);
      notify("success", t("notifications.systemDnsRestored"));
    } catch (err) {
      notifyError(t("notifications.systemDnsRestoreFailed"), err);
    } finally {
      mutationLockRef.current = false;
      finishBusy();
    }
  }

  async function handleClearDnsCache() {
    if (!running) {
      return;
    }
    beginBusy(t("busy.clearingCache"));
    try {
      const cleared = await clearDnsCache();
      notify("success", t("notifications.cacheCleared"), cleared ? t("notifications.cacheClearedCount", { count: cleared }) : t("notifications.cacheClearedEmpty"));
    } catch (err) {
      notifyError(t("notifications.cacheClearFailed"), err);
    } finally {
      finishBusy();
    }
  }

  async function handleCheckUpstreamHealth(upstreamName: string) {
    const name = upstreamName.trim();
    if (!name) {
      return;
    }
    if (!running) {
      notify("warning", t("notifications.upstreamCheckUnavailable"), t("status.serviceNotRunning"));
      return;
    }
    setCheckingUpstreams((current) => new Set(current).add(name));
    try {
      const result = await checkUpstreamHealth(name);
      setStatus(result.status);
      const latency = result.upstream.latencyMs;
      if (result.success) {
        notify(
          "success",
          t("notifications.upstreamCheckPassed"),
          latency !== undefined
            ? t("notifications.upstreamCheckPassedDescription", { name, latency })
            : t("notifications.upstreamCheckPassedDescriptionNoLatency", { name })
        );
      } else {
        const error = result.upstream.lastErrorMessage
          ? localizedMessageText(result.upstream.lastErrorMessage, (key, values) => t(key, values))
          : result.upstream.lastError || t("common.unknown");
        notify("warning", t("notifications.upstreamCheckFailed"), t("notifications.upstreamCheckFailedDescription", { name, error }));
      }
    } catch (err) {
      notifyError(t("notifications.upstreamCheckFailed"), err);
    } finally {
      setCheckingUpstreams((current) => {
        const next = new Set(current);
        next.delete(name);
        return next;
      });
    }
  }

  async function handleHideToTray() {
    setClosePromptOpen(false);
    try {
      await hideWindow();
      notify("info", t("notifications.windowHidden"), t("notifications.windowHiddenDescription"));
    } catch (err) {
      notifyError(t("notifications.hideWindowFailed"), err);
    }
  }

  async function handleQuitApp() {
    if (mutationLockRef.current) {
      setClosePromptOpen(true);
      return;
    }
    mutationLockRef.current = true;
    setClosePromptOpen(false);
    setQuitting(true);
    beginBusy(t("common.processing"));
    try {
      const configSaved = await flushPendingConfigSaves();
      await preferencesSaveQueueRef.current;
      await systemDnsSaveQueueRef.current;
      if (!configSaved) {
        setClosePromptOpen(true);
        return;
      }
      await quitApp();
    } catch (err) {
      setClosePromptOpen(true);
      notifyError(t("notifications.quitFailed"), err);
    } finally {
      mutationLockRef.current = false;
      setQuitting(false);
      finishBusy();
    }
  }

  function handleNavigate(page: string) {
    if (page === "system-dns") {
      setSettingsSection("system-dns");
      setActiveTab("settings");
      return;
    }
    setActiveTab(page);
  }

  function renderActivePage() {
    if (activeTab === "rules") {
      return <RulesPage doc={configDoc} onChange={handleConfigDocChange} validation={validation.resolver} />;
    }
    if (activeTab === "upstreams") {
      return (
        <UpstreamsPage
          doc={configDoc}
          onChange={handleConfigDocChange}
          validation={validation.resolver}
          running={running}
          checkingUpstreams={checkingUpstreams}
          onCheckHealth={handleCheckUpstreamHealth}
        />
      );
    }
    if (activeTab === "lookup") {
      return <LookupPage running={running} />;
    }
    if (activeTab === "history") {
      return <HistoryPage language={resolvedLanguage} />;
    }
    if (activeTab === "settings") {
      return (
        <SettingsPage
          doc={configDoc}
          onChange={handleConfigDocChange}
          validation={validation}
          language={language}
          languageOptions={languageOptions}
          theme={theme}
          themeOptions={themeOptions}
          preferences={preferences}
          running={running}
          busy={busy}
          section={settingsSection}
          systemDns={systemDns}
          systemDnsLoading={systemDnsLoading}
          onClearDnsCache={handleClearDnsCache}
          onLanguageChange={(value) => setLanguage(normalizeLanguage(value))}
          onThemeChange={(value) => setTheme(normalizeTheme(value))}
          onPreferencesChange={handlePreferencesChange}
          onSectionChange={setSettingsSection}
          onSystemDnsSettingsChange={handleSystemDnsSettingsChange}
          onApplySystemDns={handleApplySystemDns}
          onRestoreSystemDns={handleRestoreSystemDns}
        />
      );
    }
    return (
      <OverviewPage
        active={activeTab === "overview"}
        status={status}
        lastStarted={lastStarted}
        systemDns={systemDns}
        systemDnsLoading={systemDnsLoading}
        onNavigate={handleNavigate}
        onApplySystemDns={handleApplySystemDns}
        onRestoreSystemDns={handleRestoreSystemDns}
        language={resolvedLanguage}
      />
    );
  }

  return (
    <ConfigProvider
      locale={antdLocale}
      theme={{
        algorithm: effectiveDark ? antdTheme.darkAlgorithm : antdTheme.defaultAlgorithm,
        cssVar: { key: "autodns" }
      }}
    >
      <AntdApp>
        {notificationContextHolder}
        <Layout className={`shell desktopShell ${effectiveDark ? "themeDark" : "themeLight"}`}>
          <Header className="desktopToolbar">
            <div className="desktopBrand">
              <img src="/appicon.svg" alt="" />
              <div>
                <Typography.Title level={1}>autodns</Typography.Title>
              </div>
            </div>
            <div className="appHeaderActions">
              <Space className="headerActionCluster" size={8}>
                <Button
                  type={running ? "default" : "primary"}
                  danger={running}
                  icon={running ? <StopOutlined /> : <PlayCircleOutlined />}
                  onClick={running ? handleStop : handleStart}
                  disabled={busy}
                >
                  {busy ? t("common.processing") : running ? t("actions.stop") : t("actions.start")}
                </Button>
                <Button icon={<ReloadOutlined />} onClick={handleRestart} disabled={busy || !running}>
                  {t("actions.restart")}
                </Button>
              </Space>
            </div>
          </Header>
          <Layout className="desktopBody">
            <Sider width={220} className="desktopSidebar" theme={effectiveDark ? "dark" : "light"}>
              <div className="desktopSidebarLabel">{t("app.sidebarLabel")}</div>
              <Menu
                className="desktopNavMenu"
                mode="inline"
                selectedKeys={[activeTab]}
                items={menuItems}
                onClick={({ key }) => setActiveTab(key)}
              />
              <div className="desktopSidebarStatus" aria-label={t("status.service")}>
                <div className="desktopSidebarStatusLine">
                  <Tag color={running ? "success" : "default"} className="runtimeTag">
                    {running ? t("status.running") : t("status.stopped")}
                  </Tag>
                </div>
                <Button
                  type="link"
                  className="desktopSidebarStatusLink"
                  title={listenLine}
                  onClick={() => {
                    setActiveTab("settings");
                    setSettingsSection("service");
                  }}
                >
                  {listenLine}
                </Button>
              </div>
            </Sider>
            <Content className="appContent">
              <section className="workspace loadingOverlayHost" aria-busy={Boolean(workspaceLoadingText)}>
                <Suspense fallback={<LoadingOverlay text={t("app.loadingPage")} compact />}>
                  {renderActivePage()}
                </Suspense>
                {workspaceLoadingText ? <LoadingOverlay text={workspaceLoadingText} /> : null}
              </section>
              {manualDirty ? (
                <div className="configSaveShelf" role="status" aria-live="polite">
                  <div className="configSaveShelfText">
                    <strong>{t("config.dirtyTitle")}</strong>
                    <span>{validationErrorCount ? t("config.dirtyValidation", { count: validationErrorCount }) : dirtyHint}</span>
                  </div>
                  <Space.Compact className="configSaveShelfActions">
                    <Button size="small" icon={<CheckCircleOutlined />} onClick={handleValidateConfig} disabled={busy || !configDoc}>
                      {t("actions.validate")}
                    </Button>
                    <Button size="small" icon={<RollbackOutlined />} onClick={handleDiscardConfig} disabled={busy || !manualDirty}>
                      {t("actions.discard")}
                    </Button>
                    <Button size="small" type="primary" icon={<SaveOutlined />} onClick={handleSaveConfig} disabled={busy || !configDoc || !manualDirty || validationErrorCount > 0}>
                      {t("actions.save")}
                    </Button>
                  </Space.Compact>
                </div>
              ) : null}
            </Content>
          </Layout>
          <Footer className="desktopStatusBar">
            <Typography.Text type={configStatusType} aria-live="polite">{t("status.config")}：{configStatusText}</Typography.Text>
            <Typography.Text type="secondary">{t("status.systemDns")}：{systemDnsState}</Typography.Text>
            <Typography.Text type="secondary">{t("status.cache")}：{configDoc?.config.cache.enabled ? t("common.enabled") : t("common.disabled")}</Typography.Text>
            <Typography.Text type="secondary">{t("status.upstreams")}：{t("status.upstreamHealth", { healthy: healthyUpstreams, unhealthy: unhealthyUpstreams })}</Typography.Text>
            {runtimeStatusError ? <Typography.Text type="danger">{t("status.lastError")}：{runtimeStatusError}</Typography.Text> : <Typography.Text type="secondary">{t("status.lastError")}：{t("status.noError")}</Typography.Text>}
            <Typography.Text type="secondary" className="statusBarEnd">
              autodns{appVersion ? ` v${appVersion}` : ""}
            </Typography.Text>
          </Footer>
        </Layout>
        <Modal
          open={closePromptOpen}
          title={t("app.closeTitle")}
          footer={[
            <Button key="cancel" onClick={() => setClosePromptOpen(false)} disabled={quitting}>
              {t("actions.cancel")}
            </Button>,
            <Button key="hide" onClick={handleHideToTray} disabled={busy || quitting}>
              {t("actions.hideWindow")}
            </Button>,
            <Button key="quit" type="primary" danger onClick={handleQuitApp} disabled={busy && !quitting} loading={quitting}>
              {t("actions.quitApp")}
            </Button>
          ]}
          onCancel={() => setClosePromptOpen(false)}
        />
      </AntdApp>
    </ConfigProvider>
  );
}
