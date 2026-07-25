import { invoke } from "@tauri-apps/api/core";

import type {
  ApplyConfigResult,
  CertificateDefaults,
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
  GenerateCertificateRequest,
  GeneratedCertificate,
  SystemDnsSettings,
  SystemDnsStatus,
  UpstreamHealthCheckResult
} from "./types";

// The IPC payload shapes are guaranteed by the backend: every type in
// src-tauri/src/desktop.rs mirrors types.ts and serializes all fields
// (camelCase, no missing keys). Responses are therefore used as-is; only
// whole-command fallbacks are handled here.

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
  language: "system",
  historyEnabled: true,
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
  warnings: [],
  warningMessages: []
};

export async function startAutodns(configPath: string): Promise<DesktopStatus> {
  return invoke<DesktopStatus>("start_autodns", { configPath });
}

export async function stopAutodns(): Promise<DesktopStatus> {
  return invoke<DesktopStatus>("stop_autodns");
}

export async function loadStatus(): Promise<DesktopStatus> {
  return invoke<DesktopStatus>("status").catch(() => emptyStatus);
}

export async function clearDnsCache(): Promise<number> {
  return invoke<number>("clear_dns_cache");
}

export async function checkUpstreamHealth(upstreamName: string): Promise<UpstreamHealthCheckResult> {
  return invoke<UpstreamHealthCheckResult>("check_upstream_health", { upstreamName });
}

export async function lookupDomain(domain: string, recordType: string): Promise<DnsLookupResult> {
  return invoke<DnsLookupResult>("lookup_domain", { domain, recordType });
}

export async function listDnsHistory(
  domain: string,
  limit = 100,
  offset = 0,
  statusFilter: DnsHistoryStatusFilter = "all",
  window: DnsHistoryWindow = "all",
  upstreamName = ""
): Promise<DnsHistoryList> {
  return invoke<DnsHistoryList>("list_dns_history", { domain, statusFilter, window, upstreamName, limit, offset });
}

export async function dnsHistoryTopDomains(
  limit = 20,
  domain = "",
  statusFilter: DnsHistoryStatusFilter = "all",
  window: DnsHistoryWindow = "all",
  upstreamName = ""
): Promise<DnsHistoryTopDomain[]> {
  return invoke<DnsHistoryTopDomain[]>("dns_history_top_domains", { limit, domain, statusFilter, window, upstreamName });
}

export async function dnsHistoryUpstreamNames(limit = 200): Promise<string[]> {
  return invoke<string[]>("dns_history_upstream_names", { limit });
}

export async function dnsHistoryOverview(): Promise<DnsHistoryOverview> {
  return invoke<DnsHistoryOverview>("dns_history_overview");
}

export async function clearDnsHistory(): Promise<number> {
  return invoke<number>("clear_dns_history");
}

export async function loadManagedConfig(): Promise<ConfigDocument> {
  return invoke<ConfigDocument>("managed_config");
}

export async function validateConfig(config: DesktopConfig): Promise<void> {
  await invoke("validate_config", { config });
}

export async function validateServerCertificate(config: DesktopConfig): Promise<void> {
  await invoke("validate_server_certificate", { config });
}

export async function saveConfig(doc: ConfigDocument): Promise<ApplyConfigResult> {
  return invoke<ApplyConfigResult>("apply_config", { doc });
}

export async function loadCertificateDefaults(): Promise<CertificateDefaults> {
  return invoke<CertificateDefaults>("certificate_defaults");
}

export async function generateServerCertificate(request: GenerateCertificateRequest): Promise<GeneratedCertificate> {
  return invoke<GeneratedCertificate>("generate_server_certificate", { request });
}

export async function loadPreferences(): Promise<DesktopPreferences> {
  const prefs = await invoke<DesktopPreferences>("load_preferences").catch(() => emptyPreferences);
  return normalizePreferences(prefs);
}

export async function savePreferences(prefs: DesktopPreferences): Promise<DesktopPreferences> {
  return normalizePreferences(await invoke<DesktopPreferences>("save_preferences", { prefs }));
}

export async function loadSystemDnsStatus(force = false): Promise<SystemDnsStatus> {
  return invoke<SystemDnsStatus>("system_dns_status", { force }).catch(() => emptySystemDnsStatus);
}

export async function saveSystemDnsSettings(settings: SystemDnsSettings): Promise<SystemDnsStatus> {
  return invoke<SystemDnsStatus>("save_system_dns_settings", { settings });
}

export async function applySystemDns(): Promise<SystemDnsStatus> {
  return invoke<SystemDnsStatus>("apply_system_dns");
}

export async function restoreSystemDns(): Promise<SystemDnsStatus> {
  return invoke<SystemDnsStatus>("restore_system_dns");
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

// Preference values may come from a hand-edited file on disk, so clamp the
// enum-like fields to values the UI understands.
function normalizePreferences(prefs: DesktopPreferences): DesktopPreferences {
  return {
    ...prefs,
    closeBehavior: prefs.closeBehavior === "hide" || prefs.closeBehavior === "quit" ? prefs.closeBehavior : "ask",
    language: prefs.language === "zh-CN" || prefs.language === "en-US" ? prefs.language : "system"
  };
}
