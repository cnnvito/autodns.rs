import { Button, Card, Input, InputNumber, Select, Space, Switch, Tabs, Typography } from "antd";
import { open } from "@tauri-apps/plugin-dialog";
import { FolderOpenOutlined } from "@ant-design/icons";
import { useEffect, useState } from "react";

import { logLevelOptions, serverModeOptions } from "../features/config/options";
import type { ConfigPageProps } from "../features/config/doc";
import type { ConfigValidation } from "../features/config/validation";
import type { DesktopConfig, DesktopPreferences, SystemDnsSettings, SystemDnsStatus } from "../shared/types";
import { SystemDnsPage } from "./SystemDnsPage";

type SelectOption = {
  value: string;
  label: string;
};

export type SettingsSection = "general" | "service" | "system-dns" | "cache" | "health";

type SettingsPageProps = ConfigPageProps & {
  validation: ConfigValidation;
  theme: string;
  themeOptions: SelectOption[];
  preferences: DesktopPreferences;
  running: boolean;
  busy: boolean;
  section: SettingsSection;
  systemDns: SystemDnsStatus | null;
  systemDnsLoading: boolean;
  onClearDnsCache: () => void;
  onThemeChange: (value: string) => void;
  onPreferencesChange: (patch: Partial<DesktopPreferences>) => void;
  onSectionChange: (section: SettingsSection) => void;
  onSystemDnsSettingsChange: (settings: SystemDnsSettings) => void;
  onApplySystemDns: () => void;
  onRestoreSystemDns: () => void;
};

const numberHealthFields = new Set<keyof DesktopConfig["healthcheck"]>(["failureThreshold", "recoveryThreshold"]);

const closeBehaviorOptions: SelectOption[] = [
  { value: "ask", label: "每次询问" },
  { value: "hide", label: "关闭时隐藏窗口" },
  { value: "quit", label: "关闭时退出程序" }
];

const startAtLoginOptions: SelectOption[] = [
  { value: "enabled", label: "启用" },
  { value: "disabled", label: "禁用" }
];

const settingsTabItems: Array<{ key: SettingsSection; label: string }> = [
  { key: "general", label: "通用" },
  { key: "service", label: "服务" },
  { key: "system-dns", label: "系统 DNS" },
  { key: "cache", label: "缓存" },
  { key: "health", label: "健康检查" }
];

export function SettingsPage({
  doc,
  onChange,
  validation,
  theme,
  themeOptions,
  preferences,
  running,
  busy,
  section,
  systemDns,
  systemDnsLoading,
  onClearDnsCache,
  onThemeChange,
  onPreferencesChange,
  onSectionChange,
  onSystemDnsSettingsChange,
  onApplySystemDns,
  onRestoreSystemDns
}: SettingsPageProps) {
  if (!doc) {
    return (
      <Card title="设置">
        <Typography.Text type="secondary">正在加载本地配置。</Typography.Text>
      </Card>
    );
  }

  const currentDoc = doc;
  const cfg = currentDoc.config;
  const tlsFileEnabled = cfg.server.mode === "dot" || cfg.server.mode === "doh";
  const dohPathEnabled = cfg.server.mode === "doh";

  function updateConfig(next: DesktopConfig) {
    onChange({ path: currentDoc.path, config: next });
  }

  function updateServer(patch: Partial<DesktopConfig["server"]>) {
    updateConfig({ ...cfg, server: { ...cfg.server, ...patch } });
  }

  async function chooseServerFile(field: "certFile" | "keyFile", title: string) {
    if (!tlsFileEnabled) {
      return;
    }
    const selected = await open({
      title,
      multiple: false,
      directory: false
    });
    const path = Array.isArray(selected) ? selected[0] : selected;
    if (typeof path === "string" && path.length > 0) {
      updateServer({ [field]: path });
    }
  }

  function updateCache(name: keyof DesktopConfig["cache"], value: string | boolean) {
    updateConfig({
      ...cfg,
      cache: {
        ...cfg.cache,
        [name]: typeof value === "boolean" ? value : parseOptionalNumber(value)
      }
    });
  }

  function updateHealthcheck(name: keyof DesktopConfig["healthcheck"], value: string | boolean) {
    updateConfig({
      ...cfg,
      healthcheck: {
        ...cfg.healthcheck,
        [name]: typeof value === "boolean" ? value : numberHealthFields.has(name) ? parseOptionalNumber(value) : value
      }
    });
  }

  return (
    <section className="pageWorkbench">
      <div className="workbenchSettingsShell">
        <Tabs
          className="workbenchSettingsTabs"
          activeKey={section}
          onChange={(value) => onSectionChange(value as SettingsSection)}
          items={settingsTabItems}
        />
        <main className="workbenchSettingsContent">
          {section === "general" ? (
            <div className="settingRows">
              <SettingRow title="主题">
                <Select className="workbenchInlineSelect" value={theme} onChange={onThemeChange} options={themeOptions} />
              </SettingRow>
              <SettingRow title="关闭窗口">
                <Select
                  className="workbenchInlineSelect"
                  value={preferences.closeBehavior}
                  onChange={(value) => onPreferencesChange({ closeBehavior: value as DesktopPreferences["closeBehavior"] })}
                  options={closeBehaviorOptions}
                />
              </SettingRow>
              <SettingRow title="开机自启">
                <Select
                  className="workbenchInlineSelect"
                  value={preferences.startAtLogin ? "enabled" : "disabled"}
                  onChange={(value) => onPreferencesChange({ startAtLogin: value === "enabled" })}
                  options={startAtLoginOptions}
                />
              </SettingRow>
            </div>
          ) : null}

          {section === "service" ? (
            <div className="settingRows">
              <SettingRow title="协议模式">
                <Select className="workbenchInlineSelect" value={cfg.server.mode} onChange={(value) => updateServer({ mode: value })} options={serverModeOptions} />
              </SettingRow>
              <SettingRow title="监听地址" description="保存后会重启服务。">
                <ValidatedInput error={validation.server.listen}>
                  <Input status={validation.server.listen ? "error" : undefined} value={cfg.server.listen} onChange={(event) => updateServer({ listen: event.target.value })} placeholder="127.0.0.1:53" />
                </ValidatedInput>
              </SettingRow>
              <SettingRow title="DoH 路径">
                <ValidatedInput error={validation.server.path}>
                  <Input status={validation.server.path ? "error" : undefined} value={cfg.server.path} onChange={(event) => updateServer({ path: event.target.value })} placeholder="/dns-query" disabled={!dohPathEnabled} />
                </ValidatedInput>
              </SettingRow>
              <SettingRow title="证书文件">
                <Space.Compact style={{ width: "100%" }}>
                  <Input status={validation.server.certFile ? "error" : undefined} value={cfg.server.certFile} onChange={(event) => updateServer({ certFile: event.target.value })} placeholder="/path/to/cert.pem" disabled={!tlsFileEnabled} />
                  <Button type="primary" icon={<FolderOpenOutlined />} onClick={() => chooseServerFile("certFile", "选择证书文件")} aria-label="选择证书文件" disabled={!tlsFileEnabled} />
                </Space.Compact>
                <FieldError message={validation.server.certFile} />
              </SettingRow>
              <SettingRow title="私钥文件">
                <Space.Compact style={{ width: "100%" }}>
                  <Input status={validation.server.keyFile ? "error" : undefined} value={cfg.server.keyFile} onChange={(event) => updateServer({ keyFile: event.target.value })} placeholder="/path/to/key.pem" disabled={!tlsFileEnabled} />
                  <Button type="primary" icon={<FolderOpenOutlined />} onClick={() => chooseServerFile("keyFile", "选择私钥文件")} aria-label="选择私钥文件" disabled={!tlsFileEnabled} />
                </Space.Compact>
                <FieldError message={validation.server.keyFile} />
              </SettingRow>
              <SettingRow title="日志级别">
                <Select className="workbenchInlineSelect" value={cfg.log.level} onChange={(value) => updateConfig({ ...cfg, log: { ...cfg.log, level: value } })} options={logLevelOptions} />
              </SettingRow>
            </div>
          ) : null}

          {section === "system-dns" ? (
            <SystemDnsPage
              embedded
              systemDns={systemDns}
              loading={systemDnsLoading}
              running={running}
              onSystemDnsSettingsChange={onSystemDnsSettingsChange}
              onApplySystemDns={onApplySystemDns}
              onRestoreSystemDns={onRestoreSystemDns}
            />
          ) : null}

          {section === "cache" ? (
            <div className="settingRows">
              <SettingRow title="缓存开关">
                <Space>
                  <Switch checkedChildren="启用" unCheckedChildren="关闭" checked={cfg.cache.enabled} onChange={(checked) => updateCache("enabled", checked)} />
                  <Button onClick={onClearDnsCache} disabled={busy || !running}>立即清理</Button>
                </Space>
              </SettingRow>
              <SettingRow title="最大条目">
                <InlineNumberSetting value={cfg.cache.maxEntries} error={validation.cache.maxEntries} onChange={(value) => updateCache("maxEntries", value)} />
              </SettingRow>
              <SettingRow title="单条字节">
                <InlineNumberSetting value={cfg.cache.maxEntrySize} error={validation.cache.maxEntrySize} onChange={(value) => updateCache("maxEntrySize", value)} />
              </SettingRow>
              <SettingRow title="最小 TTL">
                <InlineNumberSetting value={cfg.cache.minTTL} error={validation.cache.minTTL} onChange={(value) => updateCache("minTTL", value)} />
              </SettingRow>
              <SettingRow title="最大 TTL">
                <InlineNumberSetting value={cfg.cache.maxTTL} error={validation.cache.maxTTL} onChange={(value) => updateCache("maxTTL", value)} />
              </SettingRow>
              <SettingRow title="失败 TTL">
                <InlineNumberSetting value={cfg.cache.negativeTTL} error={validation.cache.negativeTTL} onChange={(value) => updateCache("negativeTTL", value)} />
              </SettingRow>
            </div>
          ) : null}

          {section === "health" ? (
            <div className="settingRows">
              <SettingRow title="健康检查">
                <Switch checkedChildren="启用" unCheckedChildren="关闭" checked={cfg.healthcheck.enabled} onChange={(checked) => updateHealthcheck("enabled", checked)} />
              </SettingRow>
              <SettingRow title="检查间隔">
                <ValidatedInput error={validation.healthcheck.interval}>
                  <Input status={validation.healthcheck.interval ? "error" : undefined} value={cfg.healthcheck.interval} onChange={(event) => updateHealthcheck("interval", event.target.value)} placeholder="30s" />
                </ValidatedInput>
              </SettingRow>
              <SettingRow title="检查超时">
                <ValidatedInput error={validation.healthcheck.timeout}>
                  <Input status={validation.healthcheck.timeout ? "error" : undefined} value={cfg.healthcheck.timeout} onChange={(event) => updateHealthcheck("timeout", event.target.value)} placeholder="2s" />
                </ValidatedInput>
              </SettingRow>
              <SettingRow title="探测域名">
                <ValidatedInput error={validation.healthcheck.domain}>
                  <Input status={validation.healthcheck.domain ? "error" : undefined} value={cfg.healthcheck.domain} onChange={(event) => updateHealthcheck("domain", event.target.value)} placeholder="." />
                </ValidatedInput>
              </SettingRow>
              <SettingRow title="失败阈值">
                <InlineNumberSetting value={cfg.healthcheck.failureThreshold} error={validation.healthcheck.failureThreshold} onChange={(value) => updateHealthcheck("failureThreshold", value)} />
              </SettingRow>
              <SettingRow title="恢复阈值">
                <InlineNumberSetting value={cfg.healthcheck.recoveryThreshold} error={validation.healthcheck.recoveryThreshold} onChange={(value) => updateHealthcheck("recoveryThreshold", value)} />
              </SettingRow>
            </div>
          ) : null}
        </main>
      </div>
    </section>
  );
}

function InlineNumberSetting({ value, error, onChange }: { value: number; error?: string; onChange: (value: string) => void }) {
  const [draft, setDraft] = useState<string | null>(null);
  const displayValue = draft === null ? value : draft;

  useEffect(() => {
    setDraft(null);
  }, [value]);

  function commit() {
    if (draft === null) {
      return;
    }
    onChange(draft.trim() === "" ? "0" : draft);
    setDraft(null);
  }

  return (
    <ValidatedInput error={error}>
      <InputNumber
        className="workbenchInlineNumber"
        status={error ? "error" : undefined}
        min={0}
        value={displayValue}
        placeholder="默认"
        onChange={(next) => {
          if (next === null) {
            setDraft("");
            return;
          }
          setDraft(String(next));
          onChange(String(next));
        }}
        onBlur={commit}
        onPressEnter={commit}
      />
    </ValidatedInput>
  );
}

function parseOptionalNumber(value: string): number {
  const parsed = Number(value.trim());
  return Number.isFinite(parsed) && parsed >= 0 ? parsed : 0;
}

function SettingRow({ title, description, children }: { title: string; description?: string; children: React.ReactNode }) {
  return (
    <div className="settingRow">
      <div className="settingRowLabel">
        <strong>{title}</strong>
        {description ? <span>{description}</span> : null}
      </div>
      <div>{children}</div>
    </div>
  );
}

function ValidatedInput({ error, children }: { error?: string; children: React.ReactNode }) {
  return (
    <Space direction="vertical" size={4} className="pageFill">
      {children}
      <FieldError message={error} />
    </Space>
  );
}

function FieldError({ message }: { message?: string }) {
  return message ? <Typography.Text type="danger">{message}</Typography.Text> : null;
}
