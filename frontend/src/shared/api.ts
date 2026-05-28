import { invoke } from "@tauri-apps/api/core";

import type {
  ApplyConfigResult,
  ConfigDocument,
  DesktopConfig,
  DesktopPreferences,
  DesktopStatus,
  DnsHistoryList,
  DnsHistoryOverview,
  DnsHistoryStatusFilter,
  DnsHistoryTopDomain,
  DnsHistoryWindow,
  DnsLookupResult,
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

export async function startAutodns(configPath: string): Promise<DesktopStatus> {
  return normalizeStatus(await invoke<DesktopStatus>("start_autodns", { configPath }));
}

export async function stopAutodns(): Promise<DesktopStatus> {
  return normalizeStatus(await invoke<DesktopStatus>("stop_autodns"));
}

export async function loadStatus(): Promise<DesktopStatus> {
  const status = await invoke<DesktopStatus>("status").catch(() => emptyStatus);
  return normalizeStatus(status);
}

export function normalizeStatus(status: DesktopStatus): DesktopStatus {
  return {
    ...emptyStatus,
    ...status,
    upstreamHealth: Array.isArray(status.upstreamHealth) ? status.upstreamHealth : [],
    proxyHealth: Array.isArray(status.proxyHealth) ? status.proxyHealth : []
  };
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

export async function listDnsHistory(
  domain: string,
  limit = 100,
  offset = 0,
  statusFilter: DnsHistoryStatusFilter = "all",
  window: DnsHistoryWindow = "all"
): Promise<DnsHistoryList> {
  const result = await invoke<DnsHistoryList>("list_dns_history", { domain, statusFilter, window, limit, offset });
  return {
    items: Array.isArray(result.items) ? result.items.map(normalizeDnsHistoryEntry) : [],
    total: Number.isFinite(result.total) ? result.total : 0
  };
}

export async function dnsHistoryTopDomains(
  limit = 20,
  domain = "",
  statusFilter: DnsHistoryStatusFilter = "all",
  window: DnsHistoryWindow = "all"
): Promise<DnsHistoryTopDomain[]> {
  const result = await invoke<DnsHistoryTopDomain[]>("dns_history_top_domains", { limit, domain, statusFilter, window });
  return Array.isArray(result)
    ? result.map((item) => ({
        domain: item.domain ?? "",
        count: Number.isFinite(item.count) ? item.count : 0,
        lastSeenAt: item.lastSeenAt ?? "",
        averageDurationMs: Number.isFinite(item.averageDurationMs) ? item.averageDurationMs : 0
      }))
    : [];
}

export async function dnsHistoryOverview(): Promise<DnsHistoryOverview> {
  return normalizeDnsHistoryOverview(await invoke<DnsHistoryOverview>("dns_history_overview"));
}

export async function clearDnsHistory(): Promise<number> {
  return invoke<number>("clear_dns_history");
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
    action: result?.action ?? "saved",
    status: normalizeStatus(result?.status ?? emptyStatus)
  };
}

export async function loadPreferences(): Promise<DesktopPreferences> {
  const prefs = await invoke<DesktopPreferences>("load_preferences").catch(() => emptyPreferences);
  return normalizePreferences(prefs);
}

export async function savePreferences(prefs: DesktopPreferences): Promise<DesktopPreferences> {
  return normalizePreferences(await invoke<DesktopPreferences>("save_preferences", { prefs }));
}

export async function loadSystemDnsStatus(force = false): Promise<SystemDnsStatus> {
  const status = await invoke<SystemDnsStatus>("system_dns_status", { force }).catch(() => emptySystemDnsStatus);
  return normalizeSystemDnsStatus(status);
}

export async function saveSystemDnsSettings(settings: SystemDnsSettings): Promise<SystemDnsStatus> {
  return normalizeSystemDnsStatus(await invoke<SystemDnsStatus>("save_system_dns_settings", { settings }));
}

export async function applySystemDns(): Promise<SystemDnsStatus> {
  return normalizeSystemDnsStatus(await invoke<SystemDnsStatus>("apply_system_dns"));
}

export async function restoreSystemDns(): Promise<SystemDnsStatus> {
  return normalizeSystemDnsStatus(await invoke<SystemDnsStatus>("restore_system_dns"));
}

export async function hideWindow(): Promise<void> {
  await invoke("hide_window");
}

export async function showMainWindow(): Promise<void> {
  await invoke("show_main_window");
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
        bootstrapDns: Array.isArray(config.resolver.bootstrapDns) ? config.resolver.bootstrapDns : [],
        hosts: Array.isArray(config.resolver.hosts) ? config.resolver.hosts : [],
        hostStatuses: Array.isArray(config.resolver.hostStatuses) ? config.resolver.hostStatuses : [],
        routes: Array.isArray(config.resolver.routes) ? config.resolver.routes : [],
        routeStatuses: Array.isArray(config.resolver.routeStatuses) ? config.resolver.routeStatuses : [],
        ipv6Enabled: typeof config.resolver.ipv6Enabled === "boolean" ? config.resolver.ipv6Enabled : true
      }
    }
  };
}

function normalizeDnsHistoryEntry(item: Partial<DnsHistoryList["items"][number]>): DnsHistoryList["items"][number] {
  return {
    id: Number.isFinite(item.id) ? item.id! : 0,
    startedAt: item.startedAt ?? "",
    domain: item.domain ?? "",
    recordType: item.recordType ?? "",
    source: item.source ?? "",
    routeId: Number.isFinite(item.routeId) ? item.routeId! : -1,
    upstreamName: item.upstreamName ?? "",
    upstreamProtocol: item.upstreamProtocol ?? "",
    durationMs: Number.isFinite(item.durationMs) ? item.durationMs! : 0,
    attemptCount: Number.isFinite(item.attemptCount) ? item.attemptCount! : 0,
    responseCode: item.responseCode ?? "",
    minTtl: Number.isFinite(item.minTtl) ? item.minTtl : undefined,
    error: item.error ?? ""
  };
}

function normalizeDnsHistoryOverview(item: Partial<DnsHistoryOverview>): DnsHistoryOverview {
  return {
    windowStartedAt: item.windowStartedAt ?? "",
    generatedAt: item.generatedAt ?? "",
    total: Number.isFinite(item.total) ? item.total! : 0,
    cacheHits: Number.isFinite(item.cacheHits) ? item.cacheHits! : 0,
    failures: Number.isFinite(item.failures) ? item.failures! : 0,
    averageDurationMs: Number.isFinite(item.averageDurationMs) ? item.averageDurationMs! : 0,
    topDomains: Array.isArray(item.topDomains)
      ? item.topDomains.map((domain) => ({
          domain: domain.domain ?? "",
          count: Number.isFinite(domain.count) ? domain.count : 0,
          lastSeenAt: domain.lastSeenAt ?? "",
          averageDurationMs: Number.isFinite(domain.averageDurationMs) ? domain.averageDurationMs : 0
        }))
      : [],
    recentErrors: Array.isArray(item.recentErrors) ? item.recentErrors.map(normalizeDnsHistoryEntry) : []
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
