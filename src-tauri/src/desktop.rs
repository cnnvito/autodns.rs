use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopConfig {
    pub server: DesktopServerConfig,
    pub resolver: DesktopResolverConfig,
    pub cache: DesktopCacheConfig,
    pub healthcheck: DesktopHealthcheckConfig,
    pub log: DesktopLogConfig,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopServerConfig {
    pub mode: String,
    pub listen: String,
    pub cert_file: String,
    pub key_file: String,
    pub path: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopResolverConfig {
    pub upstreams: Vec<DesktopUpstreamConfig>,
    pub proxies: Vec<DesktopProxyConfig>,
    pub bootstrap_dns: Vec<String>,
    pub default_proxy: String,
    pub hosts: Vec<String>,
    pub host_statuses: Vec<DesktopHostStatus>,
    pub routes: Vec<String>,
    pub route_statuses: Vec<DesktopRouteStatus>,
    pub timeout: String,
    pub ipv6_enabled: bool,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopHostStatus {
    pub index: usize,
    pub enabled: bool,
    pub note: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopRouteStatus {
    pub index: usize,
    pub enabled: bool,
    pub invalid_reason: String,
    pub note: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopUpstreamConfig {
    pub name: String,
    pub protocol: String,
    pub host: String,
    pub port: String,
    pub path: String,
    pub server_name: String,
    pub proxy: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopProxyConfig {
    pub name: String,
    pub protocol: String,
    pub host: String,
    pub port: String,
    pub username: String,
    pub password: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopCacheConfig {
    pub enabled: bool,
    pub max_entries: usize,
    pub max_entry_size: usize,
    pub min_ttl: u32,
    pub max_ttl: u32,
    pub negative_ttl: u32,
    pub eviction_policy: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopHealthcheckConfig {
    pub enabled: bool,
    pub interval: String,
    pub timeout: String,
    pub domain: String,
    pub failure_threshold: u32,
    pub recovery_threshold: u32,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopLogConfig {
    pub level: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigDocument {
    pub path: String,
    pub config: DesktopConfig,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyConfigResult {
    pub action: ApplyConfigAction,
    pub status: DesktopStatus,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DnsLookupResult {
    pub domain: String,
    pub record_type: String,
    pub response_code: String,
    pub answer_count: usize,
    pub duration_ms: u128,
    pub records: Vec<DnsLookupRecord>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DnsLookupRecord {
    pub name: String,
    pub record_type: String,
    pub ttl: u32,
    pub value: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DnsHistoryList {
    pub items: Vec<DnsHistoryEntry>,
    pub total: usize,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DnsHistoryEntry {
    pub id: i64,
    pub started_at: String,
    pub domain: String,
    pub record_type: String,
    pub source: String,
    pub route_id: i32,
    pub upstream_name: String,
    pub upstream_protocol: String,
    pub duration_ms: u128,
    pub attempt_count: usize,
    pub response_code: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_ttl: Option<u32>,
    pub error: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DnsHistoryTopDomain {
    pub domain: String,
    pub count: usize,
    pub last_seen_at: String,
    pub average_duration_ms: f64,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ApplyConfigAction {
    Saved,
    HotReloaded,
    Restarted,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopStatus {
    pub running: bool,
    pub config_path: String,
    pub mode: String,
    pub listen: String,
    pub upstreams: usize,
    pub routes: usize,
    pub default_upstreams: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    pub upstream_health: Vec<UpstreamHealth>,
    pub proxy_health: Vec<ProxyHealth>,
}

impl Default for DesktopStatus {
    fn default() -> Self {
        Self {
            running: false,
            config_path: String::new(),
            mode: String::new(),
            listen: String::new(),
            upstreams: 0,
            routes: 0,
            default_upstreams: 0,
            started_at: None,
            last_error: None,
            upstream_health: Vec::new(),
            proxy_health: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpstreamHealth {
    pub name: String,
    pub endpoint: String,
    pub protocol: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub proxy: String,
    pub order: usize,
    pub health: HealthState,
    pub failure_count: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_success_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latency_ms: Option<u128>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyHealth {
    pub name: String,
    pub endpoint: String,
    pub health: HealthState,
    pub upstreams: Vec<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum HealthState {
    Unknown,
    Healthy,
    Unhealthy,
    Unused,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopPreferences {
    pub close_behavior: String,
    pub start_at_login: bool,
    pub start_at_login_supported: bool,
    pub tray_supported: bool,
    pub tray_message: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemDnsSettings {
    pub enabled: bool,
    pub target_servers: Vec<String>,
    pub selected_adapter_ids: Vec<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemDnsStatus {
    pub platform: String,
    pub supported: bool,
    pub can_apply: bool,
    pub settings: SystemDnsSettings,
    pub local_servers: Vec<String>,
    pub adapters: Vec<SystemDnsAdapter>,
    pub warnings: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemDnsAdapter {
    pub id: String,
    pub name: String,
    pub description: String,
    pub status: String,
    pub kind: String,
    pub dns_servers: Vec<String>,
    pub selected: bool,
    pub managed: bool,
    pub virtual_adapter: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interface_index: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original_dns: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_applied_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_restored_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}
