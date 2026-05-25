import { getVersion } from "@tauri-apps/api/app";
import { open } from "@tauri-apps/plugin-dialog";
import { FolderOpen } from "lucide-react";
import { useEffect, useState } from "react";

import { logLevelOptions, serverModeOptions } from "../features/config/options";
import type { ConfigPageProps } from "../features/config/doc";
import type { DesktopConfig, DesktopPreferences } from "../shared/types";
import { NumberField, SelectField, SwitchField, type SelectOption } from "../shared/ui";

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
      <section className="panel configPanel">
        <header>
          <div>
            <h2>设置</h2>
            <p>正在加载本地配置。</p>
          </div>
        </header>
      </section>
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
    <section className="pageStack">
      <section className="panel configPanel">
        <header>
          <div>
            <h2>系统设置</h2>
          </div>
        </header>
        <div className="settingsGrid">
          <label className="compactField">
            <span>主题</span>
            <SelectField value={theme} onChange={onThemeChange} options={themeOptions} />
          </label>
          <label className="compactField">
            <span>关闭窗口</span>
            <SelectField
              value={preferences.closeBehavior}
              onChange={(value) => onPreferencesChange({ closeBehavior: value as DesktopPreferences["closeBehavior"] })}
              options={closeBehaviorOptions}
            />
          </label>
          <label className="compactField">
            <span>开机自启</span>
            <SelectField
              value={preferences.startAtLogin ? "enabled" : "disabled"}
              onChange={(value) => onPreferencesChange({ startAtLogin: value === "enabled" })}
              options={startAtLoginOptions}
            />
          </label>
        </div>
      </section>

      <section className="panel configPanel">
        <header>
          <div>
            <h2>服务</h2>
            <p>修改监听地址或协议后，应用配置会重启运行时。</p>
          </div>
        </header>
        <div className="settingsGrid">
          <label className="compactField">
            <span>协议模式</span>
            <SelectField value={cfg.server.mode} onChange={(value) => updateServer({ mode: value })} options={serverModeOptions} />
          </label>
          <label className="compactField">
            <span>监听地址</span>
            <input value={cfg.server.listen} onChange={(event) => updateServer({ listen: event.target.value })} placeholder="127.0.0.1:15353" />
          </label>
          <label className="compactField">
            <span>DoH 路径</span>
            <input value={cfg.server.path} onChange={(event) => updateServer({ path: event.target.value })} placeholder="/dns-query" disabled={!dohPathEnabled} />
          </label>
          <div className="compactField">
            <span id="server-cert-file-label">证书文件</span>
            <div className="fileInputGroup">
              <input aria-labelledby="server-cert-file-label" value={cfg.server.certFile} onChange={(event) => updateServer({ certFile: event.target.value })} disabled={!tlsFileEnabled} />
              <button type="button" className="fileInputButton" onClick={() => chooseServerFile("certFile", "选择证书文件")} aria-label="选择证书文件" disabled={!tlsFileEnabled}>
                <FolderOpen size={15} />
              </button>
            </div>
          </div>
          <div className="compactField">
            <span id="server-key-file-label">私钥文件</span>
            <div className="fileInputGroup">
              <input aria-labelledby="server-key-file-label" value={cfg.server.keyFile} onChange={(event) => updateServer({ keyFile: event.target.value })} disabled={!tlsFileEnabled} />
              <button type="button" className="fileInputButton" onClick={() => chooseServerFile("keyFile", "选择私钥文件")} aria-label="选择私钥文件" disabled={!tlsFileEnabled}>
                <FolderOpen size={15} />
              </button>
            </div>
          </div>
          <label className="compactField">
            <span>日志级别</span>
            <SelectField value={cfg.log.level} onChange={(value) => updateConfig({ ...cfg, log: { ...cfg.log, level: value } })} options={logLevelOptions} />
          </label>
        </div>
      </section>

      <section className="panel configPanel">
        <header>
          <div>
            <h2>缓存</h2>
            <p>缓存可减少重复查询；TTL 控制保留时间。</p>
          </div>
          <div className="panelHeaderActions">
            <button className="compactActionButton" onClick={onClearDnsCache} disabled={busy || !running}>立即清理</button>
            <SwitchField checked={cfg.cache.enabled} onChange={(checked) => updateCache("enabled", checked)}>启用缓存</SwitchField>
          </div>
        </header>
        <div className="cacheGrid">
          <NumberField label="最大条目" value={cfg.cache.maxEntries} onChange={(value) => updateCache("maxEntries", value)} />
          <NumberField label="单条字节" value={cfg.cache.maxEntrySize} onChange={(value) => updateCache("maxEntrySize", value)} />
          <NumberField label="最小 TTL" value={cfg.cache.minTTL} onChange={(value) => updateCache("minTTL", value)} />
          <NumberField label="最大 TTL" value={cfg.cache.maxTTL} onChange={(value) => updateCache("maxTTL", value)} />
          <NumberField label="失败 TTL" value={cfg.cache.negativeTTL} onChange={(value) => updateCache("negativeTTL", value)} />
        </div>
      </section>

      <section className="panel configPanel">
        <header>
          <div>
            <h2>健康检查</h2>
            <p>异常上游会被标记，恢复达标后重新参与解析。</p>
          </div>
          <SwitchField checked={cfg.healthcheck.enabled} onChange={(checked) => updateHealthcheck("enabled", checked)}>启用检查</SwitchField>
        </header>
        <div className="settingsGrid">
          <label className="compactField">
            <span>检查间隔</span>
            <input value={cfg.healthcheck.interval} onChange={(event) => updateHealthcheck("interval", event.target.value)} placeholder="30s" />
          </label>
          <label className="compactField">
            <span>检查超时</span>
            <input value={cfg.healthcheck.timeout} onChange={(event) => updateHealthcheck("timeout", event.target.value)} placeholder="2s" />
          </label>
          <label className="compactField">
            <span>探测域名</span>
            <input value={cfg.healthcheck.domain} onChange={(event) => updateHealthcheck("domain", event.target.value)} placeholder="." />
          </label>
          <NumberField label="失败阈值" value={cfg.healthcheck.failureThreshold} onChange={(value) => updateHealthcheck("failureThreshold", value)} />
          <NumberField label="恢复阈值" value={cfg.healthcheck.recoveryThreshold} onChange={(value) => updateHealthcheck("recoveryThreshold", value)} />
        </div>
      </section>

      <section className="appVersionLine" aria-label="应用版本">
        <span>autodns</span>
        <strong>{appVersion ? `v${appVersion}` : "版本读取中"}</strong>
      </section>
    </section>
  );
}
