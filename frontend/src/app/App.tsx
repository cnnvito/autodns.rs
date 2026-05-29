import { listen } from "@tauri-apps/api/event";
import {
  ApiOutlined,
  CheckCircleOutlined,
  DashboardOutlined,
  DatabaseOutlined,
  HistoryOutlined,
  PlayCircleOutlined,
  PoweroffOutlined,
  ReloadOutlined,
  RollbackOutlined,
  SaveOutlined,
  SearchOutlined,
  SettingOutlined,
  StopOutlined
} from "@ant-design/icons";
import { Alert, App as AntdApp, Button, ConfigProvider, Layout, Modal, Space, Tabs, Tag, Typography, theme as antdTheme } from "antd";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";

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
import { errorMessage, formatDate } from "../shared/format";
import { NotificationCenter, type AppNotification, type NotificationKind } from "../shared/notifications";
import type { ConfigDocument, DesktopPreferences, DesktopStatus, SystemDnsSettings, SystemDnsStatus } from "../shared/types";
import { HistoryPage } from "../pages/HistoryPage";
import { LookupPage } from "../pages/LookupPage";
import { OverviewPage } from "../pages/OverviewPage";
import { RulesPage } from "../pages/RulesPage";
import { SettingsPage } from "../pages/SettingsPage";
import { SystemDnsPage } from "../pages/SystemDnsPage";
import { UpstreamsPage } from "../pages/UpstreamsPage";
import { applyThemePreference, loadThemePreference, normalizeTheme, themeOptions, type ThemePreference } from "./theme";

type NavigationItem = {
  key: string;
  label: string;
  icon: React.ReactNode;
};

const defaultPreferences: DesktopPreferences = {
  closeBehavior: "ask",
  startAtLogin: false,
  startAtLoginSupported: false,
  traySupported: false,
  trayMessage: ""
};

const { Header, Content, Footer } = Layout;

const navigationItems: NavigationItem[] = [
  { key: "overview", label: "首页", icon: <DashboardOutlined /> },
  { key: "rules", label: "规则", icon: <DatabaseOutlined /> },
  { key: "upstreams", label: "上游", icon: <ApiOutlined /> },
  { key: "lookup", label: "解析查询", icon: <SearchOutlined /> },
  { key: "history", label: "历史记录", icon: <HistoryOutlined /> },
  { key: "system-dns", label: "系统 DNS", icon: <ApiOutlined /> },
  { key: "settings", label: "设置", icon: <SettingOutlined /> }
];

const sectionItems = navigationItems.map((item) => ({
  key: item.key,
  label: (
    <span className="moduleTabLabel">
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

export function App() {
  const [status, setStatus] = useState<DesktopStatus | null>(null);
  const [configDoc, setConfigDoc] = useState<ConfigDocument | null>(null);
  const [savedConfigDoc, setSavedConfigDoc] = useState<ConfigDocument | null>(null);
  const [dirty, setDirty] = useState(false);
  const [notifications, setNotifications] = useState<AppNotification[]>([]);
  const [busy, setBusy] = useState(false);
  const [theme, setTheme] = useState<ThemePreference>(() => loadThemePreference());
  const [preferences, setPreferences] = useState<DesktopPreferences>(defaultPreferences);
  const [systemDns, setSystemDns] = useState<SystemDnsStatus | null>(null);
  const [systemDnsLoading, setSystemDnsLoading] = useState(false);
  const [activeTab, setActiveTab] = useState("overview");
  const [closePromptOpen, setClosePromptOpen] = useState(false);
  const nextNotificationId = useRef(1);
  const lastRuntimeError = useRef("");
  const pendingSystemDnsSave = useRef(0);
  const lastSystemDnsSaveId = useRef(0);
  const systemDnsLoadingRef = useRef(false);
  const systemDnsAdaptersRequested = useRef(false);

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

  const dismissNotification = useCallback((id: number) => {
    setNotifications((items) => items.filter((item) => item.id !== id));
  }, []);

  const notify = useCallback((kind: NotificationKind, title: string, description?: string) => {
    const id = nextNotificationId.current;
    nextNotificationId.current += 1;
    setNotifications((items) => [...items.slice(-3), { id, kind, title, description }]);
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

  async function refresh(forceSystemDns = false) {
    const nextStatus = await loadStatus();
    setStatus(nextStatus);
    if (activeTab === "system-dns") {
      await refreshSystemDns(forceSystemDns);
    }
  }

  useEffect(() => {
    bootstrap().catch((err: unknown) => notifyError("初始化失败", err));
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
    setDirty(false);
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
    if (activeTab === "system-dns" && !systemDnsAdaptersRequested.current) {
      systemDnsAdaptersRequested.current = true;
      refreshSystemDns(true).catch((err: unknown) => notifyError("系统 DNS 状态读取失败", err));
    }
  }, [activeTab, notifyError]);

  useEffect(() => {
    applyThemePreference(theme);
  }, [theme]);

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
  const lastStarted = useMemo(() => formatDate(status?.startedAt), [status?.startedAt]);
  const restartRequired = useMemo(() => needsRuntimeRestart(configDoc, savedConfigDoc), [configDoc, savedConfigDoc]);
  const effectiveDark = theme === "dark";
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

  const handleConfigDocChange = useCallback((doc: ConfigDocument) => {
    setConfigDoc(doc);
    setDirty(true);
  }, []);

  async function handleStart() {
    setBusy(true);
    try {
      const nextStatus = await startAutodns("");
      setStatus(nextStatus);
      notify("success", "服务已启动", nextStatus.listen || "本地 DNS 已开始监听");
    } catch (err) {
      notifyError("启动失败", err);
    } finally {
      setBusy(false);
    }
  }

  async function handleValidateConfig() {
    if (!configDoc) {
      return;
    }
    setBusy(true);
    try {
      await validateConfig(configDoc.config);
      notify("success", "配置校验通过", "当前配置可以保存。");
    } catch (err) {
      notifyError("配置校验失败", err);
    } finally {
      setBusy(false);
    }
  }

  async function handleSaveConfig() {
    if (!configDoc) {
      return;
    }
    setBusy(true);
    try {
      const result = await saveConfig(configDoc);
      setSavedConfigDoc(configDoc);
      setDirty(false);
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
      setBusy(false);
    }
  }

  async function handleStop() {
    setBusy(true);
    try {
      const nextStatus = await stopAutodns();
      setStatus(nextStatus);
      notify("info", "服务已停止");
    } catch (err) {
      notifyError("停止失败", err);
    } finally {
      setBusy(false);
    }
  }

  async function handleRestart() {
    if (!running) {
      return;
    }
    setBusy(true);
    try {
      await stopAutodns();
      const nextStatus = await startAutodns("");
      setStatus(nextStatus);
      notify("success", "服务已重启", "本地 DNS 已重新开始监听。");
    } catch (err) {
      notifyError("重启失败", err);
    } finally {
      setBusy(false);
    }
  }

  function handleDiscardConfig() {
    if (!savedConfigDoc) {
      return;
    }
    setConfigDoc(savedConfigDoc);
    setDirty(false);
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
    setBusy(true);
    try {
      setSystemDns(await applySystemDns());
      notify("success", "系统 DNS 已接管");
    } catch (err) {
      notifyError("系统 DNS 接管失败", err);
    } finally {
      setBusy(false);
    }
  }

  async function handleRestoreSystemDns() {
    setBusy(true);
    try {
      setSystemDns(await restoreSystemDns());
      notify("success", "系统 DNS 已恢复");
    } catch (err) {
      notifyError("系统 DNS 恢复失败", err);
    } finally {
      setBusy(false);
    }
  }

  async function handleClearDnsCache() {
    if (!running) {
      return;
    }
    setBusy(true);
    try {
      const cleared = await clearDnsCache();
      notify("success", "缓存已清理", cleared ? `已移除 ${cleared} 条缓存记录。` : "当前没有可清理的缓存记录。");
    } catch (err) {
      notifyError("清理缓存失败", err);
    } finally {
      setBusy(false);
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

  function renderActivePage() {
    if (activeTab === "rules") {
      return <RulesPage doc={configDoc} onChange={handleConfigDocChange} />;
    }
    if (activeTab === "upstreams") {
      return <UpstreamsPage doc={configDoc} onChange={handleConfigDocChange} />;
    }
    if (activeTab === "lookup") {
      return <LookupPage running={running} />;
    }
    if (activeTab === "history") {
      return <HistoryPage />;
    }
    if (activeTab === "system-dns") {
      return (
        <SystemDnsPage
          systemDns={systemDns}
          loading={systemDnsLoading}
          running={running}
          onSystemDnsSettingsChange={handleSystemDnsSettingsChange}
          onApplySystemDns={handleApplySystemDns}
          onRestoreSystemDns={handleRestoreSystemDns}
        />
      );
    }
    if (activeTab === "settings") {
      return (
        <SettingsPage
          doc={configDoc}
          onChange={handleConfigDocChange}
          theme={theme}
          themeOptions={themeOptions}
          preferences={preferences}
          running={running}
          busy={busy}
          onClearDnsCache={handleClearDnsCache}
          onThemeChange={(value) => setTheme(normalizeTheme(value))}
          onPreferencesChange={handlePreferencesChange}
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
        onNavigate={setActiveTab}
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
        <Layout className="shell desktopShell">
          <Header className="desktopToolbar">
            <div className="desktopBrand">
              <img src="/appicon.svg" alt="" />
              <div>
                <Typography.Title level={1}>autodns</Typography.Title>
                <Typography.Text type="secondary">本地 DNS 控制台</Typography.Text>
              </div>
            </div>
            <div className="desktopRuntime">
              <Tag color={running ? "success" : "default"} className="runtimeTag">
                {running ? "运行中" : "已停止"}
              </Tag>
              <Typography.Text type="secondary" ellipsis title={listenLine}>
                {listenLine}
              </Typography.Text>
            </div>
            <div className="appHeaderActions">
              <Space.Compact className="headerActionCluster">
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
              </Space.Compact>
              <Space.Compact className="headerActionCluster">
                <Button icon={<ReloadOutlined />} onClick={() => refresh(activeTab === "system-dns")} disabled={busy}>
                  刷新
                </Button>
                <Button type="primary" icon={<SaveOutlined />} onClick={handleSaveConfig} disabled={busy || !configDoc || !dirty}>
                  保存
                </Button>
                <Button icon={<PoweroffOutlined />} onClick={handleQuitApp} disabled={busy}>
                  退出
                </Button>
              </Space.Compact>
            </div>
          </Header>
          <nav className="moduleNav" aria-label="主导航">
            <Tabs activeKey={activeTab} onChange={setActiveTab} items={sectionItems} />
          </nav>
          <Content className="appContent">
            {dirty ? (
              <Alert
                className="configActionBar"
                type="warning"
                showIcon
                title="有未保存修改"
                description={
                  running
                    ? restartRequired
                      ? "监听入口已变更，保存时会自动重启服务。"
                      : "保存后会立即替换运行中的上游、路由、缓存等配置。"
                    : "服务未启动；保存后会在下次启动时使用新配置。"
                }
                action={(
                  <Space>
                    <Button icon={<CheckCircleOutlined />} onClick={handleValidateConfig} disabled={busy || !configDoc}>
                      校验
                    </Button>
                    <Button icon={<RollbackOutlined />} onClick={handleDiscardConfig} disabled={busy || !dirty}>
                      放弃
                    </Button>
                    <Button type="primary" icon={<SaveOutlined />} onClick={handleSaveConfig} disabled={busy || !configDoc || !dirty}>
                      保存
                    </Button>
                  </Space>
                )}
              />
            ) : null}

            <section className="workspace">
              {renderActivePage()}
            </section>
          </Content>
          <Footer className="desktopStatusBar">
            <Typography.Text type="secondary">系统 DNS：{systemDnsState}</Typography.Text>
            <Typography.Text type="secondary">缓存：{configDoc?.config.cache.enabled ? "启用" : "关闭"}</Typography.Text>
            <Typography.Text type="secondary">上游：{healthyUpstreams} 健康 / {unhealthyUpstreams} 异常</Typography.Text>
            {status?.lastError ? <Typography.Text type="danger">最近错误：{status.lastError}</Typography.Text> : <Typography.Text type="secondary">最近错误：无</Typography.Text>}
            <Typography.Text type="secondary" className="statusBarEnd">
              {configDoc?.path || ""}
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
        <NotificationCenter notifications={notifications} onDismiss={dismissNotification} />
      </AntdApp>
    </ConfigProvider>
  );
}
