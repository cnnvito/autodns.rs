import { Button, Card, Input, InputNumber, Select, Space, Switch, Tabs, Typography } from "antd";
import { open } from "@tauri-apps/plugin-dialog";
import { FolderOpenOutlined } from "@ant-design/icons";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { getLogLevelOptions, serverModeOptions } from "../features/config/options";
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
  language: string;
  languageOptions: SelectOption[];
  theme: string;
  themeOptions: SelectOption[];
  preferences: DesktopPreferences;
  running: boolean;
  busy: boolean;
  section: SettingsSection;
  systemDns: SystemDnsStatus | null;
  systemDnsLoading: boolean;
  onClearDnsCache: () => void;
  onLanguageChange: (value: string) => void;
  onThemeChange: (value: string) => void;
  onPreferencesChange: (patch: Partial<DesktopPreferences>) => void;
  onSectionChange: (section: SettingsSection) => void;
  onSystemDnsSettingsChange: (settings: SystemDnsSettings) => void;
  onApplySystemDns: () => void;
  onRestoreSystemDns: () => void;
};

const numberHealthFields = new Set<keyof DesktopConfig["healthcheck"]>(["failureThreshold", "recoveryThreshold"]);

export function SettingsPage({
  doc,
  onChange,
  validation,
  language,
  languageOptions,
  theme,
  themeOptions,
  preferences,
  running,
  busy,
  section,
  systemDns,
  systemDnsLoading,
  onClearDnsCache,
  onLanguageChange,
  onThemeChange,
  onPreferencesChange,
  onSectionChange,
  onSystemDnsSettingsChange,
  onApplySystemDns,
  onRestoreSystemDns
}: SettingsPageProps) {
  const { t } = useTranslation();
  const closeBehaviorOptions: SelectOption[] = [
    { value: "ask", label: t("settings.closeAsk") },
    { value: "hide", label: t("settings.closeHide") },
    { value: "quit", label: t("settings.closeQuit") }
  ];
  const startAtLoginOptions: SelectOption[] = [
    { value: "enabled", label: t("common.enabled") },
    { value: "disabled", label: t("common.disabled") }
  ];
  const settingsTabItems: Array<{ key: SettingsSection; label: string }> = [
    { key: "general", label: t("settings.tabGeneral") },
    { key: "service", label: t("settings.tabService") },
    { key: "system-dns", label: t("settings.tabSystemDns") },
    { key: "cache", label: t("settings.tabCache") },
    { key: "health", label: t("settings.tabHealth") }
  ];
  const logLevelOptions = getLogLevelOptions(t);

  if (!doc) {
    return (
      <Card title={t("settings.title")}>
        <Typography.Text type="secondary">{t("settings.loading")}</Typography.Text>
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
              <SettingRow title={t("settings.language")}>
                <Select className="workbenchInlineSelect" value={language} onChange={onLanguageChange} options={languageOptions} />
              </SettingRow>
              <SettingRow title={t("settings.theme")}>
                <Select className="workbenchInlineSelect" value={theme} onChange={onThemeChange} options={themeOptions} />
              </SettingRow>
              <SettingRow title={t("settings.closeWindow")}>
                <Select
                  className="workbenchInlineSelect"
                  value={preferences.closeBehavior}
                  onChange={(value) => onPreferencesChange({ closeBehavior: value as DesktopPreferences["closeBehavior"] })}
                  options={closeBehaviorOptions}
                />
              </SettingRow>
              <SettingRow title={t("settings.startAtLogin")}>
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
              <SettingRow title={t("settings.serviceMode")}>
                <Select className="workbenchInlineSelect" value={cfg.server.mode} onChange={(value) => updateServer({ mode: value })} options={serverModeOptions} />
              </SettingRow>
              <SettingRow title={t("settings.listenAddress")} description={t("settings.restartAfterSave")}>
                <ValidatedInput error={validation.server.listen}>
                  <Input status={validation.server.listen ? "error" : undefined} value={cfg.server.listen} onChange={(event) => updateServer({ listen: event.target.value })} placeholder="127.0.0.1:53" />
                </ValidatedInput>
              </SettingRow>
              <SettingRow title={t("settings.dohPath")}>
                <ValidatedInput error={validation.server.path}>
                  <Input status={validation.server.path ? "error" : undefined} value={cfg.server.path} onChange={(event) => updateServer({ path: event.target.value })} placeholder="/dns-query" disabled={!dohPathEnabled} />
                </ValidatedInput>
              </SettingRow>
              <SettingRow title={t("settings.certFile")}>
                <Space.Compact style={{ width: "100%" }}>
                  <Input status={validation.server.certFile ? "error" : undefined} value={cfg.server.certFile} onChange={(event) => updateServer({ certFile: event.target.value })} placeholder="/path/to/cert.pem" disabled={!tlsFileEnabled} />
                  <Button type="primary" icon={<FolderOpenOutlined />} onClick={() => chooseServerFile("certFile", t("settings.chooseCertFile"))} aria-label={t("settings.chooseCertFile")} disabled={!tlsFileEnabled} />
                </Space.Compact>
                <FieldError message={validation.server.certFile} />
              </SettingRow>
              <SettingRow title={t("settings.keyFile")}>
                <Space.Compact style={{ width: "100%" }}>
                  <Input status={validation.server.keyFile ? "error" : undefined} value={cfg.server.keyFile} onChange={(event) => updateServer({ keyFile: event.target.value })} placeholder="/path/to/key.pem" disabled={!tlsFileEnabled} />
                  <Button type="primary" icon={<FolderOpenOutlined />} onClick={() => chooseServerFile("keyFile", t("settings.chooseKeyFile"))} aria-label={t("settings.chooseKeyFile")} disabled={!tlsFileEnabled} />
                </Space.Compact>
                <FieldError message={validation.server.keyFile} />
              </SettingRow>
              <SettingRow title={t("settings.logLevel")}>
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
              <SettingRow title={t("settings.cacheEnabled")}>
                <Space>
                  <Switch checkedChildren={t("common.enabled")} unCheckedChildren={t("common.disabled")} checked={cfg.cache.enabled} onChange={(checked) => updateCache("enabled", checked)} />
                  <Button onClick={onClearDnsCache} disabled={busy || !running}>{t("settings.clearNow")}</Button>
                </Space>
              </SettingRow>
              <SettingRow title={t("settings.maxEntries")}>
                <InlineNumberSetting value={cfg.cache.maxEntries} error={validation.cache.maxEntries} onChange={(value) => updateCache("maxEntries", value)} />
              </SettingRow>
              <SettingRow title={t("settings.maxEntrySize")}>
                <InlineNumberSetting value={cfg.cache.maxEntrySize} error={validation.cache.maxEntrySize} onChange={(value) => updateCache("maxEntrySize", value)} />
              </SettingRow>
              <SettingRow title={t("settings.minTtl")}>
                <InlineNumberSetting value={cfg.cache.minTTL} error={validation.cache.minTTL} onChange={(value) => updateCache("minTTL", value)} />
              </SettingRow>
              <SettingRow title={t("settings.maxTtl")}>
                <InlineNumberSetting value={cfg.cache.maxTTL} error={validation.cache.maxTTL} onChange={(value) => updateCache("maxTTL", value)} />
              </SettingRow>
              <SettingRow title={t("settings.negativeTtl")}>
                <InlineNumberSetting value={cfg.cache.negativeTTL} error={validation.cache.negativeTTL} onChange={(value) => updateCache("negativeTTL", value)} />
              </SettingRow>
            </div>
          ) : null}

          {section === "health" ? (
            <div className="settingRows">
              <SettingRow title={t("settings.healthcheckEnabled")}>
                <Switch checkedChildren={t("common.enabled")} unCheckedChildren={t("common.disabled")} checked={cfg.healthcheck.enabled} onChange={(checked) => updateHealthcheck("enabled", checked)} />
              </SettingRow>
              <SettingRow title={t("settings.healthInterval")}>
                <ValidatedInput error={validation.healthcheck.interval}>
                  <Input status={validation.healthcheck.interval ? "error" : undefined} value={cfg.healthcheck.interval} onChange={(event) => updateHealthcheck("interval", event.target.value)} placeholder="30s" />
                </ValidatedInput>
              </SettingRow>
              <SettingRow title={t("settings.healthTimeout")}>
                <ValidatedInput error={validation.healthcheck.timeout}>
                  <Input status={validation.healthcheck.timeout ? "error" : undefined} value={cfg.healthcheck.timeout} onChange={(event) => updateHealthcheck("timeout", event.target.value)} placeholder="2s" />
                </ValidatedInput>
              </SettingRow>
              <SettingRow title={t("settings.healthDomain")}>
                <ValidatedInput error={validation.healthcheck.domain}>
                  <Input status={validation.healthcheck.domain ? "error" : undefined} value={cfg.healthcheck.domain} onChange={(event) => updateHealthcheck("domain", event.target.value)} placeholder="." />
                </ValidatedInput>
              </SettingRow>
              <SettingRow title={t("settings.failureThreshold")}>
                <InlineNumberSetting value={cfg.healthcheck.failureThreshold} error={validation.healthcheck.failureThreshold} onChange={(value) => updateHealthcheck("failureThreshold", value)} />
              </SettingRow>
              <SettingRow title={t("settings.recoveryThreshold")}>
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
  const { t } = useTranslation();
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
        placeholder={t("settings.defaultPlaceholder")}
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
