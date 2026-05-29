import { Button, Card, Input, InputNumber, Segmented, Select, Space, Switch, Tag, Typography } from "antd";
import { getVersion } from "@tauri-apps/api/app";
import { open } from "@tauri-apps/plugin-dialog";
import { FolderOpenOutlined } from "@ant-design/icons";
import { useEffect, useState } from "react";

import { logLevelOptions, serverModeOptions } from "../features/config/options";
import type { ConfigPageProps } from "../features/config/doc";
import type { DesktopConfig, DesktopPreferences } from "../shared/types";

type SelectOption = {
  value: string;
  label: string;
};

type SettingsSection = "general" | "service" | "cache" | "health";

type SettingsPageProps = ConfigPageProps & {
  theme: string;
  themeOptions: SelectOption[];
  preferences: DesktopPreferences;
  running: boolean;
  busy: boolean;
  onClearDnsCache: () => void;
  onThemeChange: (value: string) => void;
  onPreferencesChange: (patch: Partial<DesktopPreferences>) => void;
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

export function SettingsPage({
  doc,
  onChange,
  theme,
  themeOptions,
  preferences,
  running,
  busy,
  onClearDnsCache,
  onThemeChange,
  onPreferencesChange
}: SettingsPageProps) {
  const [appVersion, setAppVersion] = useState("");
  const [section, setSection] = useState<SettingsSection>("general");

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
        [name]: typeof value === "boolean" ? value : Number(value)
      }
    });
  }

  function updateHealthcheck(name: keyof DesktopConfig["healthcheck"], value: string | boolean) {
    updateConfig({
      ...cfg,
      healthcheck: {
        ...cfg.healthcheck,
        [name]: typeof value === "boolean" ? value : numberHealthFields.has(name) ? Number(value) : value
      }
    });
  }

  return (
    <section className="pageWorkbench">
      <div className="workbenchToolbar">
        <div className="workbenchToolbarMain">
          <span className="workbenchTitle">设置</span>
          <Tag>{running ? "服务运行中" : "服务未运行"}</Tag>
          <Typography.Text type="secondary">autodns {appVersion ? `v${appVersion}` : "版本读取中"}</Typography.Text>
        </div>
      </div>

      <div className="workbenchBody workbenchSettingsBody">
        <aside className="workbenchSideNav">
          <Segmented
            vertical
            block
            value={section}
            onChange={(value) => setSection(value as SettingsSection)}
            options={[
              { value: "general", label: "通用" },
              { value: "service", label: "服务" },
              { value: "cache", label: "缓存" },
              { value: "health", label: "健康检查" }
            ]}
          />
        </aside>

        <main className="workbenchSettingsContent">
          {section === "general" ? (
            <div className="settingRows">
              <SettingRow title="主题" description="跟随系统或手动选择界面主题。">
                <Select className="workbenchInlineSelect" value={theme} onChange={onThemeChange} options={themeOptions} />
              </SettingRow>
              <SettingRow title="关闭窗口" description="控制窗口关闭按钮的默认行为。">
                <Select
                  className="workbenchInlineSelect"
                  value={preferences.closeBehavior}
                  onChange={(value) => onPreferencesChange({ closeBehavior: value as DesktopPreferences["closeBehavior"] })}
                  options={closeBehaviorOptions}
                />
              </SettingRow>
              <SettingRow title="开机自启" description="登录系统后自动启动桌面端。">
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
              <SettingRow title="协议模式" description="本地服务监听协议。">
                <Select className="workbenchInlineSelect" value={cfg.server.mode} onChange={(value) => updateServer({ mode: value })} options={serverModeOptions} />
              </SettingRow>
              <SettingRow title="监听地址" description="保存监听或协议变更时会重启服务。">
                <Input value={cfg.server.listen} onChange={(event) => updateServer({ listen: event.target.value })} placeholder="127.0.0.1:15353" />
              </SettingRow>
              <SettingRow title="DoH 路径" description="仅 DoH 模式生效。">
                <Input value={cfg.server.path} onChange={(event) => updateServer({ path: event.target.value })} placeholder="/dns-query" disabled={!dohPathEnabled} />
              </SettingRow>
              <SettingRow title="证书文件" description="DoT 或 DoH 模式使用。">
                <Input
                  value={cfg.server.certFile}
                  onChange={(event) => updateServer({ certFile: event.target.value })}
                  disabled={!tlsFileEnabled}
                  addonAfter={<Button type="text" icon={<FolderOpenOutlined />} onClick={() => chooseServerFile("certFile", "选择证书文件")} aria-label="选择证书文件" disabled={!tlsFileEnabled} />}
                />
              </SettingRow>
              <SettingRow title="私钥文件" description="DoT 或 DoH 模式使用。">
                <Input
                  value={cfg.server.keyFile}
                  onChange={(event) => updateServer({ keyFile: event.target.value })}
                  disabled={!tlsFileEnabled}
                  addonAfter={<Button type="text" icon={<FolderOpenOutlined />} onClick={() => chooseServerFile("keyFile", "选择私钥文件")} aria-label="选择私钥文件" disabled={!tlsFileEnabled} />}
                />
              </SettingRow>
              <SettingRow title="日志级别" description="控制桌面端和本地服务日志详细程度。">
                <Select className="workbenchInlineSelect" value={cfg.log.level} onChange={(value) => updateConfig({ ...cfg, log: { ...cfg.log, level: value } })} options={logLevelOptions} />
              </SettingRow>
            </div>
          ) : null}

          {section === "cache" ? (
            <div className="settingRows">
              <SettingRow title="缓存开关" description="缓存可减少重复查询。">
                <Space>
                  <Switch checkedChildren="启用" unCheckedChildren="关闭" checked={cfg.cache.enabled} onChange={(checked) => updateCache("enabled", checked)} />
                  <Button onClick={onClearDnsCache} disabled={busy || !running}>立即清理</Button>
                </Space>
              </SettingRow>
              <SettingRow title="最大条目" description="缓存中最多保留的记录数量。">
                <InlineNumberSetting value={cfg.cache.maxEntries} onChange={(value) => updateCache("maxEntries", value)} />
              </SettingRow>
              <SettingRow title="单条字节" description="单条缓存记录大小上限。">
                <InlineNumberSetting value={cfg.cache.maxEntrySize} onChange={(value) => updateCache("maxEntrySize", value)} />
              </SettingRow>
              <SettingRow title="最小 TTL" description="低于该值的结果会被提升到此 TTL。">
                <InlineNumberSetting value={cfg.cache.minTTL} onChange={(value) => updateCache("minTTL", value)} />
              </SettingRow>
              <SettingRow title="最大 TTL" description="高于该值的结果会被截断。">
                <InlineNumberSetting value={cfg.cache.maxTTL} onChange={(value) => updateCache("maxTTL", value)} />
              </SettingRow>
              <SettingRow title="失败 TTL" description="无记录或失败结果的缓存时间。">
                <InlineNumberSetting value={cfg.cache.negativeTTL} onChange={(value) => updateCache("negativeTTL", value)} />
              </SettingRow>
            </div>
          ) : null}

          {section === "health" ? (
            <div className="settingRows">
              <SettingRow title="健康检查" description="异常上游会被标记，恢复达标后重新参与解析。">
                <Switch checkedChildren="启用" unCheckedChildren="关闭" checked={cfg.healthcheck.enabled} onChange={(checked) => updateHealthcheck("enabled", checked)} />
              </SettingRow>
              <SettingRow title="检查间隔" description="每次探测之间的间隔。">
                <Input value={cfg.healthcheck.interval} onChange={(event) => updateHealthcheck("interval", event.target.value)} placeholder="30s" />
              </SettingRow>
              <SettingRow title="检查超时" description="单次健康检查等待时长。">
                <Input value={cfg.healthcheck.timeout} onChange={(event) => updateHealthcheck("timeout", event.target.value)} placeholder="2s" />
              </SettingRow>
              <SettingRow title="探测域名" description="用于检查上游可用性的域名。">
                <Input value={cfg.healthcheck.domain} onChange={(event) => updateHealthcheck("domain", event.target.value)} placeholder="." />
              </SettingRow>
              <SettingRow title="失败阈值" description="连续失败多少次后标记异常。">
                <InlineNumberSetting value={cfg.healthcheck.failureThreshold} onChange={(value) => updateHealthcheck("failureThreshold", value)} />
              </SettingRow>
              <SettingRow title="恢复阈值" description="连续成功多少次后恢复使用。">
                <InlineNumberSetting value={cfg.healthcheck.recoveryThreshold} onChange={(value) => updateHealthcheck("recoveryThreshold", value)} />
              </SettingRow>
            </div>
          ) : null}
        </main>
      </div>
    </section>
  );
}

function InlineNumberSetting({ value, onChange }: { value: number; onChange: (value: string) => void }) {
  return <InputNumber className="workbenchInlineNumber" min={0} value={value} onChange={(next) => onChange(next === null ? "" : String(next))} />;
}

function SettingRow({ title, description, children }: { title: string; description: string; children: React.ReactNode }) {
  return (
    <div className="settingRow">
      <div className="settingRowLabel">
        <strong>{title}</strong>
        <span>{description}</span>
      </div>
      <div>{children}</div>
    </div>
  );
}
