export type DesktopStatus = {
  running: boolean;
  configPath: string;
  mode: string;
  listen: string;
  upstreams: number;
  routes: number;
  defaultUpstreams: number;
  startedAt?: string;
  lastError?: string;
  lastErrorMessage?: LocalizedMessage;
  upstreamHealth: UpstreamHealth[];
  proxyHealth: ProxyHealth[];
};

export type LocalizedMessage = {
  code: string;
  message: string;
  values?: Record<string, string | number | boolean>;
};

export type CommandError = LocalizedMessage;

export type ConfigDocument = {
  path: string;
  config: DesktopConfig;
};

export type CertificateDefaults = {
  commonName: string;
  organization: string;
  domains: string[];
  ipAddresses: string[];
  validDays: number;
  outputDir: string;
  filePrefix: string;
};

export type GenerateCertificateRequest = CertificateDefaults;

export type GeneratedCertificate = {
  caCertFile: string;
  caKeyFile: string;
  certFile: string;
  keyFile: string;
};

export type ApplyConfigAction = "saved" | "hotReloaded" | "restarted";

export type ApplyConfigResult = {
  action: ApplyConfigAction;
  status: DesktopStatus;
};

export type DnsLookupResult = {
  domain: string;
  recordType: string;
  responseCode: string;
  answerCount: number;
  durationMs: number;
  records: DnsLookupRecord[];
};

export type DnsLookupRecord = {
  name: string;
  recordType: string;
  ttl: number;
  value: string;
};

export type DnsHistoryList = {
  items: DnsHistoryEntry[];
  total: number;
};

export type DnsHistoryEntry = {
  id: number;
  startedAt: string;
  domain: string;
  recordType: string;
  source: string;
  routeId: number;
  upstreamName: string;
  upstreamProtocol: string;
  durationMs: number;
  attemptCount: number;
  responseCode: string;
  minTtl?: number;
  error: string;
  errorMessage?: LocalizedMessage;
};

export type DnsHistoryTopDomain = {
  domain: string;
  count: number;
  lastSeenAt: string;
  averageDurationMs: number;
};

export type DnsHistoryStatusFilter = "all" | "errors";

export type DnsHistoryWindow = "1h" | "24h" | "all";

export type DnsHistoryOverview = {
  windowStartedAt: string;
  generatedAt: string;
  total: number;
  cacheHits: number;
  failures: number;
  averageDurationMs: number;
  topDomains: DnsHistoryTopDomain[];
  recentErrors: DnsHistoryEntry[];
};

export type DesktopConfig = {
  server: ServerConfig;
  resolver: ResolverConfig;
  cache: CacheConfig;
  healthcheck: HealthcheckConfig;
  log: LogConfig;
};

export type ServerConfig = {
  mode: string;
  listen: string;
  tlsSource: string;
  certFile: string;
  keyFile: string;
  certPem: string;
  keyPem: string;
  path: string;
};

export type ResolverConfig = {
  upstreams: UpstreamConfig[];
  proxies: ProxyConfig[];
  bootstrapDns: string[];
  defaultProxy: string;
  hosts: string[];
  hostStatuses: HostStatus[];
  routes: string[];
  routeStatuses: RouteStatus[];
  timeout: string;
  ipv6Enabled: boolean;
};

export type HostStatus = {
  index: number;
  enabled: boolean;
  note: string;
};

export type RouteStatus = {
  index: number;
  enabled: boolean;
  invalidReason: string;
  note: string;
};

export type UpstreamConfig = {
  name: string;
  protocol: string;
  host: string;
  port: string;
  path: string;
  serverName: string;
  proxy: string;
};

export type ProxyConfig = {
  name: string;
  protocol: string;
  host: string;
  port: string;
  username: string;
  password: string;
};

export type CacheConfig = {
  enabled: boolean;
  maxEntries: number;
  maxEntrySize: number;
  minTTL: number;
  maxTTL: number;
  negativeTTL: number;
  evictionPolicy: string;
};

export type HealthcheckConfig = {
  enabled: boolean;
  interval: string;
  timeout: string;
  domain: string;
  failureThreshold: number;
  recoveryThreshold: number;
};

export type LogConfig = {
  level: string;
};

export type HealthState = "unknown" | "healthy" | "unhealthy" | "unused";

export type UpstreamHealth = {
  name: string;
  endpoint: string;
  protocol: string;
  proxy?: string;
  order: number;
  health: HealthState;
  failureCount: number;
  lastError?: string;
  lastErrorMessage?: LocalizedMessage;
  lastSuccessAt?: string;
  latencyMs?: number;
};

export type ProxyHealth = {
  name: string;
  endpoint: string;
  health: HealthState;
  upstreams: string[];
};

export type CloseBehavior = "ask" | "hide" | "quit";
export type LanguagePreference = "system" | "zh-CN" | "en-US";

export type DesktopPreferences = {
  closeBehavior: CloseBehavior;
  language: LanguagePreference;
  historyEnabled: boolean;
  startAtLogin: boolean;
  startAtLoginSupported: boolean;
  traySupported: boolean;
  trayMessage: string;
};

export type SystemDnsSettings = {
  enabled: boolean;
  targetServers: string[];
  selectedAdapterIds: string[];
};

export type SystemDnsStatus = {
  platform: string;
  supported: boolean;
  canApply: boolean;
  settings: SystemDnsSettings;
  localServers: string[];
  adapters: SystemDnsAdapter[];
  warnings: string[];
  warningMessages?: LocalizedMessage[];
  lastError?: string;
  lastErrorMessage?: LocalizedMessage;
};

export type SystemDnsAdapter = {
  id: string;
  name: string;
  description: string;
  status: string;
  kind: string;
  dnsServers: string[];
  selected: boolean;
  managed: boolean;
  virtualAdapter: boolean;
  interfaceIndex?: number;
  originalDns?: string[];
  lastAppliedAt?: string;
  lastRestoredAt?: string;
  lastError?: string;
  lastErrorMessage?: LocalizedMessage;
};
