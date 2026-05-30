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
import { errorMessage, formatDate } from "../shared/format";
import type { ConfigDocument, DesktopPreferences, DesktopStatus, SystemDnsSettings, SystemDnsStatus } from "../shared/types";
import type { SettingsSection } from "../pages/SettingsPage";
import { LoadingOverlay } from "../shared/LoadingOverlay";
import { applyThemePreference, loadThemePreference, normalizeTheme, themeOptions, type ThemePreference } from "./theme";

type NavigationItem = {
  key: string;
  label: string;
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
  { key: "overview", label: "状态", icon: <DashboardOutlined /> },
  { key: "rules", label: "规则", icon: <DatabaseOutlined /> },
  { key: "upstreams", label: "上游", icon: <ApiOutlined /> },
  { key: "lookup", label: "查询", icon: <SearchOutlined /> },
  { key: "history", label: "历史", icon: <HistoryOutlined /> },
  { key: "settings", label: "偏好设置", icon: <SettingOutlined /> }
];

const menuItems = navigationItems.map((item) => ({
  key: item.key,
  label: (
    <span className="desktopNavLabel">
      {item.icon}
      <span>{item.label}</span>
    </span>
  )
}));

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
  const [status, setStatus] = useState<DesktopStatus | null>(null);
  const [configDoc, setConfigDoc] = useState<ConfigDocument | null>(null);
  const [savedConfigDoc, setSavedConfigDoc] = useState<ConfigDocument | null>(null);
  const [busy, setBusy] = useState(false);
  const [busyText, setBusyText] = useState("");
  const [initializing, setInitializing] = useState(true);
  const [theme, setTheme] = useState<ThemePreference>(() => loadThemePreference());
  const [systemDark, setSystemDark] = useState(() => getSystemDarkPreference());
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

  const notifyError = useCallback((title: string, err: unknown) => {
    notify("error", title, errorMessage(err));
  }, [notify]);

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
      .catch((err: unknown) => notifyError("初始化失败", err))
      .finally(() => setInitializing(false));
  }, [notifyError]);

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
      refreshSystemDns(true).catch((err: unknown) => notifyError("系统 DNS 状态读取失败", err));
    }
  }, [activeTab, notifyError, settingsSection]);

  useEffect(() => {
    applyThemePreference(theme);
  }, [theme]);

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
    const runtimeError = status?.lastError || "";
    if (runtimeError && runtimeError !== lastRuntimeError.current) {
      notify("error", "运行时异常", runtimeError);
    }
    lastRuntimeError.current = runtimeError;
  }, [notify, status?.lastError]);

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
  const validation = useMemo(() => configDoc ? validateDesktopConfig(configDoc.config) : emptyConfigValidation(), [configDoc]);
  const validationMessages = useMemo(() => flattenValidationMessages(validation), [validation]);
  const validationErrorCount = validationMessages.length;
  const lastStarted = useMemo(() => formatDate(status?.startedAt), [status?.startedAt]);
  const restartRequired = useMemo(() => needsRuntimeRestart(configDoc, savedConfigDoc), [configDoc, savedConfigDoc]);
  const dirty = useMemo(() => hasConfigChanges(configDoc, savedConfigDoc), [configDoc, savedConfigDoc]);
  const effectiveDark = theme === "system" ? systemDark : theme === "dark";
  const listenLine = running ? `${status?.listen || "本地监听中"} · ${(status?.mode || "udp").toUpperCase()}` : "服务未运行";
  const healthyUpstreams = status?.upstreamHealth.filter((item) => item.health === "healthy").length ?? 0;
  const unhealthyUpstreams = status?.upstreamHealth.filter((item) => item.health === "unhealthy").length ?? 0;
  const systemDnsState = systemDnsLoading
    ? "读取中"
    : systemDns?.settings.enabled
      ? "允许接管"
      : systemDns?.supported
        ? "未接管"
        : "不可用";
  const dirtyHint = running
    ? restartRequired
      ? "监听入口已变更，保存时会自动重启服务。"
      : "保存后会立即替换运行中的上游、分流、缓存等配置。"
    : "服务未启动；保存后会在下次启动时使用新配置。";
  const workspaceLoadingText = initializing ? "正在加载本地配置" : busy ? busyText || "正在处理操作" : "";

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
    beginBusy("正在启动 DNS 服务");
    try {
      const nextStatus = await startAutodns("");
      setStatus(nextStatus);
      notify("success", "服务已启动", nextStatus.listen || "本地 DNS 已开始监听");
    } catch (err) {
      notifyError("启动失败", err);
    } finally {
      finishBusy();
    }
  }

  async function handleValidateConfig() {
    if (!configDoc) {
      return;
    }
    if (hasValidationErrors(validation)) {
      notify("warning", "配置校验未通过", validationMessages.slice(0, 3).join("\n"));
      return;
    }
    beginBusy("正在校验配置");
    try {
      await validateConfig(configDoc.config);
      notify("success", "配置校验通过", "当前配置可以保存。");
    } catch (err) {
      notifyError("配置校验失败", err);
    } finally {
      finishBusy();
    }
  }

  async function handleSaveConfig() {
    if (!configDoc) {
      return;
    }
    if (hasValidationErrors(validation)) {
      notify("warning", "请先修正配置", validationMessages.slice(0, 3).join("\n"));
      return;
    }
    beginBusy(restartRequired ? "正在保存配置并重启服务" : "正在保存配置");
    try {
      const result = await saveConfig(configDoc);
      setSavedConfigDoc(configDoc);
      setStatus(result.status);
      if (result.action === "restarted") {
        notify("success", "配置已保存并重启", "监听入口发生变化，服务已自动重启。");
      } else if (result.action === "hotReloaded") {
        notify("success", "配置已保存并生效", "运行中的 DNS 服务已使用新配置。");
      } else {
        notify("success", "配置已保存", "本地配置库已更新。");
      }
    } catch (err) {
      notifyError("保存失败", err);
    } finally {
      finishBusy();
    }
  }

  async function handleStop() {
    beginBusy("正在停止 DNS 服务");
    try {
      const nextStatus = await stopAutodns();
      setStatus(nextStatus);
      notify("info", "服务已停止");
    } catch (err) {
      notifyError("停止失败", err);
    } finally {
      finishBusy();
    }
  }

  async function handleRestart() {
    if (!running) {
      return;
    }
    beginBusy("正在重启 DNS 服务");
    try {
      await stopAutodns();
      const nextStatus = await startAutodns("");
      setStatus(nextStatus);
      notify("success", "服务已重启", "本地 DNS 已重新开始监听。");
    } catch (err) {
      notifyError("重启失败", err);
    } finally {
      finishBusy();
    }
  }

  function handleDiscardConfig() {
    if (!savedConfigDoc) {
      return;
    }
    setConfigDoc(savedConfigDoc);
    notify("info", "已放弃修改", "配置已恢复到上次保存状态。");
  }

  async function handlePreferencesChange(patch: Partial<DesktopPreferences>) {
    const next = { ...preferences, ...patch };
    setPreferences(next);
    try {
      const saved = await savePreferences(next);
      setPreferences(saved);
      notify("success", "桌面行为已更新");
    } catch (err) {
      setPreferences(preferences);
      notifyError("桌面行为保存失败", err);
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
        notifyError("系统 DNS 设置保存失败", err);
      }
    } finally {
      pendingSystemDnsSave.current = Math.max(0, pendingSystemDnsSave.current - 1);
    }
  }

  async function handleApplySystemDns() {
    beginBusy("正在接管系统 DNS");
    try {
      setSystemDns(await applySystemDns());
      notify("success", "系统 DNS 已接管");
    } catch (err) {
      notifyError("系统 DNS 接管失败", err);
    } finally {
      finishBusy();
    }
  }

  async function handleRestoreSystemDns() {
    beginBusy("正在恢复系统 DNS");
    try {
      setSystemDns(await restoreSystemDns());
      notify("success", "系统 DNS 已恢复");
    } catch (err) {
      notifyError("系统 DNS 恢复失败", err);
    } finally {
      finishBusy();
    }
  }

  async function handleClearDnsCache() {
    if (!running) {
      return;
    }
    beginBusy("正在清理 DNS 缓存");
    try {
      const cleared = await clearDnsCache();
      notify("success", "缓存已清理", cleared ? `已移除 ${cleared} 条缓存记录。` : "当前没有可清理的缓存记录。");
    } catch (err) {
      notifyError("清理缓存失败", err);
    } finally {
      finishBusy();
    }
  }

  async function handleHideToTray() {
    setClosePromptOpen(false);
    try {
      await hideWindow();
      notify("info", "窗口已隐藏", "可从托盘重新打开。");
    } catch (err) {
      notifyError("隐藏窗口失败", err);
    }
  }

  async function handleQuitApp() {
    setClosePromptOpen(false);
    try {
      await quitApp();
    } catch (err) {
      notifyError("退出失败", err);
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
      return <HistoryPage />;
    }
    if (activeTab === "settings") {
      return (
        <SettingsPage
          doc={configDoc}
          onChange={handleConfigDocChange}
          validation={validation}
          theme={theme}
          themeOptions={themeOptions}
          preferences={preferences}
          running={running}
          busy={busy}
          section={settingsSection}
          systemDns={systemDns}
          systemDnsLoading={systemDnsLoading}
          onClearDnsCache={handleClearDnsCache}
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
      />
    );
  }

  return (
    <ConfigProvider
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
                  {busy ? "处理中" : running ? "停止" : "启动"}
                </Button>
                <Button icon={<ReloadOutlined />} onClick={handleRestart} disabled={busy || !running}>
                  重启
                </Button>
              </Space>
            </div>
          </Header>
          <Layout className="desktopBody">
            <Sider width={220} className="desktopSidebar" theme={effectiveDark ? "dark" : "light"}>
              <div className="desktopSidebarLabel">AUTODNS</div>
              <Menu
                className="desktopNavMenu"
                mode="inline"
                selectedKeys={[activeTab]}
                items={menuItems}
                onClick={({ key }) => setActiveTab(key)}
              />
              <div className="desktopSidebarStatus" aria-label="服务状态">
                <div className="desktopSidebarStatusLine">
                  <Tag color={running ? "success" : "default"} className="runtimeTag">
                    {running ? "运行中" : "已停止"}
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
                <Suspense fallback={<LoadingOverlay text="正在加载页面" compact />}>
                  {renderActivePage()}
                </Suspense>
                {workspaceLoadingText ? <LoadingOverlay text={workspaceLoadingText} /> : null}
              </section>
              {dirty ? (
                <div className="configSaveShelf" role="status" aria-live="polite">
                  <div className="configSaveShelfText">
                    <strong>未保存修改</strong>
                    <span>{validationErrorCount ? `发现 ${validationErrorCount} 个配置问题，请修正后再保存。` : dirtyHint}</span>
                  </div>
                  <Space.Compact className="configSaveShelfActions">
                    <Button size="small" icon={<CheckCircleOutlined />} onClick={handleValidateConfig} disabled={busy || !configDoc}>
                      校验
                    </Button>
                    <Button size="small" icon={<RollbackOutlined />} onClick={handleDiscardConfig} disabled={busy || !dirty}>
                      放弃
                    </Button>
                    <Button size="small" type="primary" icon={<SaveOutlined />} onClick={handleSaveConfig} disabled={busy || !configDoc || !dirty || validationErrorCount > 0}>
                      保存
                    </Button>
                  </Space.Compact>
                </div>
              ) : null}
            </Content>
          </Layout>
          <Footer className="desktopStatusBar">
            <Typography.Text type={dirty ? "warning" : "secondary"}>配置：{dirty ? "未保存" : "已保存"}</Typography.Text>
            <Typography.Text type="secondary">系统 DNS：{systemDnsState}</Typography.Text>
            <Typography.Text type="secondary">缓存：{configDoc?.config.cache.enabled ? "启用" : "关闭"}</Typography.Text>
            <Typography.Text type="secondary">上游：{healthyUpstreams} 健康 / {unhealthyUpstreams} 异常</Typography.Text>
            {status?.lastError ? <Typography.Text type="danger">最近错误：{status.lastError}</Typography.Text> : <Typography.Text type="secondary">最近错误：无</Typography.Text>}
            <Typography.Text type="secondary" className="statusBarEnd">
              autodns{appVersion ? ` v${appVersion}` : ""}
            </Typography.Text>
          </Footer>
        </Layout>
        <Modal
          open={closePromptOpen}
          title="关闭 autodns"
          footer={[
            <Button key="cancel" onClick={() => setClosePromptOpen(false)}>
              取消
            </Button>,
            <Button key="hide" onClick={handleHideToTray}>
              隐藏窗口
            </Button>,
            <Button key="quit" type="primary" danger onClick={handleQuitApp}>
              退出程序
            </Button>
          ]}
          onCancel={() => setClosePromptOpen(false)}
        />
      </AntdApp>
    </ConfigProvider>
  );
}
