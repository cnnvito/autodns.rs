import { listen } from "@tauri-apps/api/event";
import { getVersion } from "@tauri-apps/api/app";
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
import { errorMessage, formatDate, localizedMessageText } from "../shared/format";
import type { ConfigDocument, DesktopPreferences, DesktopStatus, SystemDnsSettings, SystemDnsStatus } from "../shared/types";
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

const notificationConfig = {
  placement: "bottomRight" as const,
  bottom: 18,
  duration: 4,
  maxCount: 4,
  pauseOnHover: true
};

const defaultPreferences: DesktopPreferences = {
  closeBehavior: "ask",
  language: "system",
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
    || currentServer.certFile !== savedServer.certFile
    || currentServer.keyFile !== savedServer.keyFile
    || currentServer.path !== savedServer.path;
}

function hasConfigChanges(current: ConfigDocument | null, saved: ConfigDocument | null): boolean {
  if (!current || !saved) {
    return false;
  }
  return stableConfigString(current) !== stableConfigString(saved);
}

function stableConfigString(doc: ConfigDocument): string {
  const { resolver, ...config } = doc.config;
  const { hostStatuses: _hostStatuses, routeStatuses: _routeStatuses, ...resolverConfig } = resolver;
  return JSON.stringify({ ...config, resolver: resolverConfig });
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
  const lastRuntimeError = useRef("");
  const pendingSystemDnsSave = useRef(0);
  const lastSystemDnsSaveId = useRef(0);
  const systemDnsLoadingRef = useRef(false);
  const systemDnsAdaptersRequested = useRef(false);
  const [notificationApi, notificationContextHolder] = notification.useNotification(notificationConfig);
  const notificationApiRef = useRef(notificationApi);
  notificationApiRef.current = notificationApi;

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

  async function refreshSystemDns(force = false) {
    if (systemDnsLoadingRef.current) {
      return;
    }
    systemDnsLoadingRef.current = true;
    setSystemDnsLoading(true);
    try {
      const nextSystemDns = await loadSystemDnsStatus(force);
      if (pendingSystemDnsSave.current === 0) {
        setSystemDns(nextSystemDns);
      }
    } finally {
      systemDnsLoadingRef.current = false;
      setSystemDnsLoading(false);
    }
  }

  useEffect(() => {
    bootstrap()
      .catch((err: unknown) => notifyError(t("notifications.bootstrapFailed"), err))
      .finally(() => setInitializing(false));
  }, [notifyError, t]);

  async function bootstrap() {
    const [doc, prefs, nextStatus, nextSystemDns] = await Promise.all([
      loadManagedConfig(),
      loadPreferences(),
      loadStatus(),
      loadSystemDnsStatus(false)
    ]);
    setConfigDoc(doc);
    setSavedConfigDoc(doc);
    setPreferences(prefs);
    setLanguage(normalizeLanguage(prefs.language));
    setStatus(nextStatus);
    setSystemDns(nextSystemDns);
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
    let unlisten: (() => void) | undefined;
    listen("desktop:close-requested", () => {
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
  const dirty = useMemo(() => hasConfigChanges(configDoc, savedConfigDoc), [configDoc, savedConfigDoc]);
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
  const workspaceLoadingText = initializing ? t("busy.loadingConfig") : busy ? busyText || t("busy.processing") : "";

  const handleConfigDocChange = useCallback((doc: ConfigDocument) => {
    setConfigDoc(doc);
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
    beginBusy(t("busy.startingService"));
    try {
      const nextStatus = await startAutodns("");
      setStatus(nextStatus);
      notify("success", t("notifications.serviceStarted"), nextStatus.listen || t("notifications.serviceStartedDescription"));
    } catch (err) {
      notifyError(t("notifications.serviceStartFailed"), err);
    } finally {
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
    if (!configDoc) {
      return;
    }
    if (hasValidationErrors(validation)) {
      notify("warning", t("notifications.validateRequired"), validationMessages.slice(0, 3).join("\n"));
      return;
    }
    beginBusy(restartRequired ? t("busy.savingConfigAndRestarting") : t("busy.savingConfig"));
    try {
      const result = await saveConfig(configDoc);
      setSavedConfigDoc(configDoc);
      setStatus(result.status);
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
      finishBusy();
    }
  }

  async function handleStop() {
    beginBusy(t("busy.stoppingService"));
    try {
      const nextStatus = await stopAutodns();
      setStatus(nextStatus);
      notify("info", t("notifications.serviceStopped"));
    } catch (err) {
      notifyError(t("notifications.serviceStopFailed"), err);
    } finally {
      finishBusy();
    }
  }

  async function handleRestart() {
    if (!running) {
      return;
    }
    beginBusy(t("busy.restartingService"));
    try {
      await stopAutodns();
      const nextStatus = await startAutodns("");
      setStatus(nextStatus);
      notify("success", t("notifications.serviceRestarted"), t("notifications.serviceRestartedDescription"));
    } catch (err) {
      notifyError(t("notifications.restartFailed"), err);
    } finally {
      finishBusy();
    }
  }

  function handleDiscardConfig() {
    if (!savedConfigDoc) {
      return;
    }
    setConfigDoc(savedConfigDoc);
    notify("info", t("notifications.configDiscarded"), t("notifications.configDiscardedDescription"));
  }

  async function handlePreferencesChange(patch: Partial<DesktopPreferences>) {
    const next = { ...preferences, ...patch };
    setPreferences(next);
    try {
      const saved = await savePreferences(next);
      setPreferences(saved);
      notify("success", t("notifications.desktopBehaviorSaved"));
    } catch (err) {
      setPreferences(preferences);
      notifyError(t("notifications.desktopBehaviorSaveFailed"), err);
    }
  }

  async function handleSystemDnsSettingsChange(settings: SystemDnsSettings) {
    const previous = systemDns;
    const saveId = lastSystemDnsSaveId.current + 1;
    lastSystemDnsSaveId.current = saveId;
    pendingSystemDnsSave.current += 1;
    setSystemDns((current) => (current ? applyOptimisticSystemDnsSettings(current, settings) : current));
    try {
      const saved = await saveSystemDnsSettings(settings);
      if (saveId === lastSystemDnsSaveId.current) {
        setSystemDns(saved);
      }
    } catch (err) {
      if (previous && saveId === lastSystemDnsSaveId.current) {
        setSystemDns(previous);
        notifyError(t("notifications.systemDnsSaveFailed"), err);
      }
    } finally {
      pendingSystemDnsSave.current = Math.max(0, pendingSystemDnsSave.current - 1);
    }
  }

  async function handleApplySystemDns() {
    beginBusy(t("busy.applyingSystemDns"));
    try {
      setSystemDns(await applySystemDns());
      notify("success", t("notifications.systemDnsApplied"));
    } catch (err) {
      notifyError(t("notifications.systemDnsApplyFailed"), err);
    } finally {
      finishBusy();
    }
  }

  async function handleRestoreSystemDns() {
    beginBusy(t("busy.restoringSystemDns"));
    try {
      setSystemDns(await restoreSystemDns());
      notify("success", t("notifications.systemDnsRestored"));
    } catch (err) {
      notifyError(t("notifications.systemDnsRestoreFailed"), err);
    } finally {
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
    setClosePromptOpen(false);
    try {
      await quitApp();
    } catch (err) {
      notifyError(t("notifications.quitFailed"), err);
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
      return <UpstreamsPage doc={configDoc} onChange={handleConfigDocChange} validation={validation.resolver} />;
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
              {dirty ? (
                <div className="configSaveShelf" role="status" aria-live="polite">
                  <div className="configSaveShelfText">
                    <strong>{t("config.dirtyTitle")}</strong>
                    <span>{validationErrorCount ? t("config.dirtyValidation", { count: validationErrorCount }) : dirtyHint}</span>
                  </div>
                  <Space.Compact className="configSaveShelfActions">
                    <Button size="small" icon={<CheckCircleOutlined />} onClick={handleValidateConfig} disabled={busy || !configDoc}>
                      {t("actions.validate")}
                    </Button>
                    <Button size="small" icon={<RollbackOutlined />} onClick={handleDiscardConfig} disabled={busy || !dirty}>
                      {t("actions.discard")}
                    </Button>
                    <Button size="small" type="primary" icon={<SaveOutlined />} onClick={handleSaveConfig} disabled={busy || !configDoc || !dirty || validationErrorCount > 0}>
                      {t("actions.save")}
                    </Button>
                  </Space.Compact>
                </div>
              ) : null}
            </Content>
          </Layout>
          <Footer className="desktopStatusBar">
            <Typography.Text type={dirty ? "warning" : "secondary"}>{t("status.config")}：{dirty ? t("config.unsaved") : t("config.saved")}</Typography.Text>
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
            <Button key="cancel" onClick={() => setClosePromptOpen(false)}>
              {t("actions.cancel")}
            </Button>,
            <Button key="hide" onClick={handleHideToTray}>
              {t("actions.hideWindow")}
            </Button>,
            <Button key="quit" type="primary" danger onClick={handleQuitApp}>
              {t("actions.quitApp")}
            </Button>
          ]}
          onCancel={() => setClosePromptOpen(false)}
        />
      </AntdApp>
    </ConfigProvider>
  );
}
