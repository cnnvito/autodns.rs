import { invoke } from "@tauri-apps/api/core";

import type {
  ApplyConfigResult,
  ConfigDocument,
  DesktopConfig,
  DesktopPreferences,
  DesktopStatus,
  DnsLookupResult,
  LogEntry,
  ProxyConfig,
  SystemDnsSettings,
  SystemDnsStatus,
  UpstreamConfig
} from "./types";

const emptyStatus: DesktopStatus = {
  running: false,
  configPath: "",
  mode: "",
  listen: "",
  upstreams: 0,
  routes: 0,
  defaultUpstreams: 0,
  upstreamHealth: [],
  proxyHealth: []
};

const emptyPreferences: DesktopPreferences = {
  closeBehavior: "ask",
  startAtLogin: false,
  startAtLoginSupported: false,
  traySupported: false,
  trayMessage: ""
};

const emptySystemDnsStatus: SystemDnsStatus = {
  platform: "",
  supported: false,
  canApply: false,
  settings: {
    enabled: false,
    targetServers: [],
    selectedAdapterIds: []
  },
  localServers: [],
  adapters: [],
  warnings: []
};

export async function startAutodns(configPath: string): Promise<void> {
  await invoke("start_autodns", { configPath });
}

export async function stopAutodns(): Promise<void> {
  await invoke("stop_autodns");
}

export async function loadStatus(): Promise<DesktopStatus> {
  const status = await invoke<DesktopStatus>("status").catch(() => emptyStatus);
  return {
    ...emptyStatus,
    ...status,
    upstreamHealth: Array.isArray(status.upstreamHealth) ? status.upstreamHealth : [],
    proxyHealth: Array.isArray(status.proxyHealth) ? status.proxyHealth : []
  };
}

export async function loadLogs(): Promise<LogEntry[]> {
  const logs = await invoke<LogEntry[]>("recent_logs").catch(() => []);
  return Array.isArray(logs) ? logs : [];
}

export async function clearDnsCache(): Promise<number> {
  return invoke<number>("clear_dns_cache");
}

export async function lookupDomain(domain: string, recordType: string): Promise<DnsLookupResult> {
  const result = await invoke<DnsLookupResult>("lookup_domain", { domain, recordType });
  return {
    domain: result.domain,
    recordType: result.recordType,
    responseCode: result.responseCode,
    answerCount: result.answerCount,
    durationMs: result.durationMs,
    records: Array.isArray(result.records) ? result.records : []
  };
}

export async function loadManagedConfig(): Promise<ConfigDocument> {
  return normalizeConfigDocument(await invoke<ConfigDocument>("managed_config"));
}

export async function validateConfig(config: DesktopConfig): Promise<void> {
  await invoke("validate_config", { config });
}

export async function saveConfig(doc: ConfigDocument): Promise<ApplyConfigResult> {
  const result = await invoke<ApplyConfigResult>("apply_config", { doc });
  return {
    action: result?.action ?? "saved"
  };
}

export async function loadPreferences(): Promise<DesktopPreferences> {
  const prefs = await invoke<DesktopPreferences>("load_preferences").catch(() => emptyPreferences);
  return normalizePreferences(prefs);
}

export async function savePreferences(prefs: DesktopPreferences): Promise<DesktopPreferences> {
  return normalizePreferences(await invoke<DesktopPreferences>("save_preferences", { prefs }));
}

export async function loadSystemDnsStatus(): Promise<SystemDnsStatus> {
  const status = await invoke<SystemDnsStatus>("system_dns_status").catch(() => emptySystemDnsStatus);
  return normalizeSystemDnsStatus(status);
}

export async function saveSystemDnsSettings(settings: SystemDnsSettings): Promise<SystemDnsStatus> {
  return normalizeSystemDnsStatus(await invoke<SystemDnsStatus>("save_system_dns_settings", { settings }));
}

export async function applySystemDns(): Promise<void> {
  await invoke("apply_system_dns");
}

export async function restoreSystemDns(): Promise<void> {
  await invoke("restore_system_dns");
}

export async function hideWindow(): Promise<void> {
  await invoke("hide_window");
}

export async function quitApp(): Promise<void> {
  await invoke("quit_app");
}

function normalizeConfigDocument(doc: ConfigDocument): ConfigDocument {
  const config = doc.config;
  return {
    ...doc,
    config: {
      ...config,
      resolver: {
        ...config.resolver,
        upstreams: normalizeUpstreams(config.resolver.upstreams),
        proxies: normalizeProxies(config.resolver.proxies),
        hosts: Array.isArray(config.resolver.hosts) ? config.resolver.hosts : [],
        hostStatuses: Array.isArray(config.resolver.hostStatuses) ? config.resolver.hostStatuses : [],
        routes: Array.isArray(config.resolver.routes) ? config.resolver.routes : [],
        routeStatuses: Array.isArray(config.resolver.routeStatuses) ? config.resolver.routeStatuses : [],
        ipv6Enabled: typeof config.resolver.ipv6Enabled === "boolean" ? config.resolver.ipv6Enabled : true
      }
    }
  };
}

function normalizeUpstreams(upstreams: unknown): UpstreamConfig[] {
  if (!Array.isArray(upstreams)) {
    return [];
  }
  return upstreams.map((item) => {
    const upstream = item as Partial<UpstreamConfig>;
    return {
      name: upstream.name ?? "",
      protocol: upstream.protocol ?? "udp",
      host: upstream.host ?? "",
      port: upstream.port ?? "",
      path: upstream.path ?? "",
      serverName: upstream.serverName ?? "",
      proxy: upstream.proxy ?? ""
    };
  });
}

function normalizeProxies(proxies: unknown): ProxyConfig[] {
  if (!Array.isArray(proxies)) {
    return [];
  }
  return proxies.map((item) => {
    const proxy = item as Partial<ProxyConfig>;
    return {
      name: proxy.name ?? "",
      protocol: proxy.protocol ?? "socks5",
      host: proxy.host ?? "",
      port: proxy.port ?? "",
      username: proxy.username ?? "",
      password: proxy.password ?? ""
    };
  });
}

function normalizePreferences(prefs: DesktopPreferences): DesktopPreferences {
  return {
    ...emptyPreferences,
    ...prefs,
    closeBehavior: prefs.closeBehavior === "hide" || prefs.closeBehavior === "quit" ? prefs.closeBehavior : "ask"
  };
}

function normalizeSystemDnsStatus(status: SystemDnsStatus): SystemDnsStatus {
  return {
    ...emptySystemDnsStatus,
    ...status,
    settings: {
      ...emptySystemDnsStatus.settings,
      ...status.settings,
      targetServers: Array.isArray(status.settings?.targetServers) ? status.settings.targetServers : [],
      selectedAdapterIds: Array.isArray(status.settings?.selectedAdapterIds) ? status.settings.selectedAdapterIds : []
    },
    localServers: Array.isArray(status.localServers) ? status.localServers : [],
    adapters: Array.isArray(status.adapters) ? status.adapters : [],
    warnings: Array.isArray(status.warnings) ? status.warnings : []
  };
}
