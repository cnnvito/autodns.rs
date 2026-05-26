use crate::desktop::*;
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::path::PathBuf;
use std::time::Duration;
use url::Url;

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct CoreConfig {
    pub server: CoreServerConfig,
    pub resolver: CoreResolverConfig,
    pub cache: CoreCacheConfig,
    pub healthcheck: CoreHealthcheckConfig,
    pub log: CoreLogConfig,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct CoreServerConfig {
    pub mode: String,
    pub listen: String,
    #[serde(default)]
    pub cert_file: String,
    #[serde(default)]
    pub key_file: String,
    #[serde(default)]
    pub path: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct CoreResolverConfig {
    #[serde(default)]
    pub upstreams: Vec<CoreUpstreamConfig>,
    #[serde(default)]
    pub proxies: Vec<CoreProxyConfig>,
    #[serde(default)]
    pub default_proxy: String,
    #[serde(default)]
    pub hosts: Vec<String>,
    #[serde(default)]
    pub routes: Vec<String>,
    #[serde(default)]
    pub timeout: String,
    #[serde(default = "default_true")]
    pub ipv6_enabled: bool,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct CoreUpstreamConfig {
    pub name: String,
    pub endpoint: String,
    #[serde(default)]
    pub server_name: String,
    #[serde(default)]
    pub proxy: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct CoreProxyConfig {
    pub name: String,
    pub endpoint: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct CoreCacheConfig {
    pub enabled: bool,
    #[serde(default)]
    pub max_entries: usize,
    #[serde(default)]
    pub max_entry_size: usize,
    #[serde(default)]
    pub min_ttl: u32,
    #[serde(default)]
    pub max_ttl: u32,
    #[serde(default)]
    pub negative_ttl: u32,
    #[serde(default)]
    pub eviction_policy: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct CoreHealthcheckConfig {
    pub enabled: bool,
    #[serde(default)]
    pub interval: String,
    #[serde(default)]
    pub timeout: String,
    #[serde(default)]
    pub domain: String,
    #[serde(default)]
    pub failure_threshold: u32,
    #[serde(default)]
    pub recovery_threshold: u32,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct CoreLogConfig {
    #[serde(default)]
    pub level: String,
}

pub fn desktop_config_dir() -> Result<PathBuf> {
    let dir = dirs::config_dir().ok_or_else(|| anyhow!("user config dir is not available"))?;
    Ok(dir.join("autodns"))
}

pub fn database_path() -> Result<PathBuf> {
    Ok(desktop_config_dir()?.join("autodns.sqlite3"))
}

pub fn validate_desktop_config(cfg: DesktopConfig) -> Result<()> {
    let mut core = core_config_from_desktop(cfg);
    core.apply_defaults();
    core.validate()
}

pub fn default_local_config() -> CoreConfig {
    let mut cfg = CoreConfig {
        server: CoreServerConfig {
            mode: "udp".into(),
            listen: "127.0.0.1:15353".into(),
            ..Default::default()
        },
        resolver: CoreResolverConfig {
            timeout: "5s".into(),
            upstreams: vec![
                CoreUpstreamConfig {
                    name: "cloudflare".into(),
                    endpoint: "udp://1.1.1.1".into(),
                    ..Default::default()
                },
                CoreUpstreamConfig {
                    name: "google".into(),
                    endpoint: "udp://8.8.8.8".into(),
                    ..Default::default()
                },
            ],
            ipv6_enabled: true,
            ..Default::default()
        },
        cache: CoreCacheConfig {
            enabled: true,
            ..Default::default()
        },
        healthcheck: CoreHealthcheckConfig {
            enabled: true,
            ..Default::default()
        },
        log: CoreLogConfig {
            level: "info".into(),
        },
    };
    cfg.apply_defaults();
    cfg
}

pub fn desktop_config_from_core(cfg: &CoreConfig) -> DesktopConfig {
    DesktopConfig {
        server: DesktopServerConfig {
            mode: cfg.server.mode.clone(),
            listen: cfg.server.listen.clone(),
            cert_file: cfg.server.cert_file.clone(),
            key_file: cfg.server.key_file.clone(),
            path: cfg.server.path.clone(),
        },
        resolver: DesktopResolverConfig {
            upstreams: cfg
                .resolver
                .upstreams
                .iter()
                .map(desktop_upstream_from_core)
                .collect(),
            proxies: cfg
                .resolver
                .proxies
                .iter()
                .map(desktop_proxy_from_core)
                .collect(),
            default_proxy: cfg.resolver.default_proxy.clone(),
            hosts: cfg.resolver.hosts.clone(),
            host_statuses: Vec::new(),
            routes: cfg.resolver.routes.clone(),
            route_statuses: Vec::new(),
            timeout: cfg.resolver.timeout.clone(),
            ipv6_enabled: cfg.resolver.ipv6_enabled,
        },
        cache: DesktopCacheConfig {
            enabled: cfg.cache.enabled,
            max_entries: cfg.cache.max_entries,
            max_entry_size: cfg.cache.max_entry_size,
            min_ttl: cfg.cache.min_ttl,
            max_ttl: cfg.cache.max_ttl,
            negative_ttl: cfg.cache.negative_ttl,
            eviction_policy: cfg.cache.eviction_policy.clone(),
        },
        healthcheck: DesktopHealthcheckConfig {
            enabled: cfg.healthcheck.enabled,
            interval: cfg.healthcheck.interval.clone(),
            timeout: cfg.healthcheck.timeout.clone(),
            domain: cfg.healthcheck.domain.clone(),
            failure_threshold: cfg.healthcheck.failure_threshold,
            recovery_threshold: cfg.healthcheck.recovery_threshold,
        },
        log: DesktopLogConfig {
            level: cfg.log.level.clone(),
        },
    }
}

pub fn core_config_from_desktop(cfg: DesktopConfig) -> CoreConfig {
    CoreConfig {
        server: CoreServerConfig {
            mode: cfg.server.mode,
            listen: cfg.server.listen,
            cert_file: cfg.server.cert_file,
            key_file: cfg.server.key_file,
            path: cfg.server.path,
        },
        resolver: CoreResolverConfig {
            upstreams: cfg
                .resolver
                .upstreams
                .into_iter()
                .map(|item| {
                    let endpoint = upstream_endpoint_from_desktop(&item);
                    CoreUpstreamConfig {
                        name: item.name,
                        endpoint,
                        server_name: item.server_name,
                        proxy: item.proxy,
                    }
                })
                .collect(),
            proxies: cfg
                .resolver
                .proxies
                .into_iter()
                .map(|item| {
                    let endpoint = proxy_endpoint_from_desktop(&item);
                    CoreProxyConfig {
                        name: item.name,
                        endpoint,
                    }
                })
                .collect(),
            default_proxy: cfg.resolver.default_proxy,
            hosts: cfg.resolver.hosts,
            routes: cfg.resolver.routes,
            timeout: cfg.resolver.timeout,
            ipv6_enabled: cfg.resolver.ipv6_enabled,
        },
        cache: CoreCacheConfig {
            enabled: cfg.cache.enabled,
            max_entries: cfg.cache.max_entries,
            max_entry_size: cfg.cache.max_entry_size,
            min_ttl: cfg.cache.min_ttl,
            max_ttl: cfg.cache.max_ttl,
            negative_ttl: cfg.cache.negative_ttl,
            eviction_policy: cfg.cache.eviction_policy,
        },
        healthcheck: CoreHealthcheckConfig {
            enabled: cfg.healthcheck.enabled,
            interval: cfg.healthcheck.interval,
            timeout: cfg.healthcheck.timeout,
            domain: cfg.healthcheck.domain,
            failure_threshold: cfg.healthcheck.failure_threshold,
            recovery_threshold: cfg.healthcheck.recovery_threshold,
        },
        log: CoreLogConfig {
            level: cfg.log.level,
        },
    }
}

fn default_true() -> bool {
    true
}

fn desktop_upstream_from_core(item: &CoreUpstreamConfig) -> DesktopUpstreamConfig {
    let parts = endpoint_parts_from_raw(&item.endpoint);
    DesktopUpstreamConfig {
        name: item.name.clone(),
        protocol: parts.protocol,
        host: parts.host,
        port: parts.port,
        path: parts.path,
        server_name: item.server_name.clone(),
        proxy: item.proxy.clone(),
    }
}

fn desktop_proxy_from_core(item: &CoreProxyConfig) -> DesktopProxyConfig {
    let (endpoint, username, password) = split_proxy_endpoint_auth(&item.endpoint);
    let parts = endpoint_parts_from_raw(&endpoint);
    DesktopProxyConfig {
        name: item.name.clone(),
        protocol: parts.protocol,
        host: parts.host,
        port: parts.port,
        username,
        password,
    }
}

pub(crate) fn upstream_endpoint_from_desktop(item: &DesktopUpstreamConfig) -> String {
    let protocol = if item.protocol.trim().is_empty() {
        "udp"
    } else {
        item.protocol.trim()
    };
    let host = item.host.trim();
    let port = item.port.trim();
    let address = endpoint_address(host, port);
    if protocol == "http" || protocol == "https" {
        let path = normalize_doh_path(&item.path);
        format!("{protocol}://{address}{path}")
    } else {
        format!("{protocol}://{address}")
    }
}

pub(crate) fn proxy_endpoint_from_desktop(item: &DesktopProxyConfig) -> String {
    let endpoint = proxy_endpoint_base_from_desktop(item);
    proxy_endpoint_with_auth(&endpoint, &item.username, &item.password)
}

fn proxy_endpoint_base_from_desktop(item: &DesktopProxyConfig) -> String {
    let protocol = if item.protocol.trim().is_empty() {
        "socks5"
    } else {
        item.protocol.trim()
    };
    format!(
        "{protocol}://{}",
        endpoint_address(item.host.trim(), item.port.trim())
    )
}

fn endpoint_address(host: &str, port: &str) -> String {
    let host = if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]")
    } else {
        host.to_string()
    };
    if port.is_empty() {
        host
    } else {
        format!("{host}:{port}")
    }
}

fn normalize_doh_path(path: &str) -> String {
    let value = path.trim();
    if value.is_empty() || value == "/" {
        "/dns-query".into()
    } else if value.starts_with('/') {
        value.into()
    } else {
        format!("/{value}")
    }
}

#[derive(Default)]
struct EndpointParts {
    protocol: String,
    host: String,
    port: String,
    path: String,
}

fn endpoint_parts_from_raw(raw: &str) -> EndpointParts {
    let Ok(url) = Url::parse(raw) else {
        return EndpointParts {
            protocol: String::new(),
            host: raw.to_string(),
            port: String::new(),
            path: String::new(),
        };
    };
    EndpointParts {
        protocol: url.scheme().to_string(),
        host: url.host_str().unwrap_or_default().to_string(),
        port: url.port().map(|port| port.to_string()).unwrap_or_default(),
        path: if url.path() == "/" {
            String::new()
        } else {
            url.path().to_string()
        },
    }
}

impl CoreConfig {
    pub fn apply_defaults(&mut self) {
        if self.server.mode == "doh" && self.server.path.is_empty() {
            self.server.path = "/dns-query".into();
        }
        self.apply_cache_defaults();
        if self.healthcheck.enabled {
            if self.healthcheck.interval.is_empty() {
                self.healthcheck.interval = "30s".into();
            }
            if self.healthcheck.timeout.is_empty() {
                self.healthcheck.timeout = "2s".into();
            }
            if self.healthcheck.domain.is_empty() {
                self.healthcheck.domain = ".".into();
            }
            if self.healthcheck.failure_threshold == 0 {
                self.healthcheck.failure_threshold = 3;
            }
            if self.healthcheck.recovery_threshold == 0 {
                self.healthcheck.recovery_threshold = 2;
            }
        }
        if self.log.level.is_empty() {
            self.log.level = "info".into();
        }
    }

    fn apply_cache_defaults(&mut self) {
        if self.cache.eviction_policy.is_empty() {
            self.cache.eviction_policy = "lru".into();
        }
        if !self.cache.enabled {
            return;
        }
        if self.cache.max_entries == 0 {
            self.cache.max_entries = 10000;
        }
        if self.cache.max_entry_size == 0 {
            self.cache.max_entry_size = 4096;
        }
        if self.cache.min_ttl == 0 {
            self.cache.min_ttl = 10;
        }
        if self.cache.max_ttl == 0 {
            self.cache.max_ttl = 600;
        }
        if self.cache.negative_ttl == 0 {
            self.cache.negative_ttl = 30;
        }
    }

    pub fn validate(&self) -> Result<()> {
        match self.server.mode.as_str() {
            "udp" | "tcp" | "doh" | "dot" => {}
            _ => return Err(anyhow!("server.mode must be one of udp,tcp,doh,dot")),
        }
        if self.server.listen.trim().is_empty() {
            return Err(anyhow!("server.listen is required"));
        }
        match self.server.mode.as_str() {
            "doh" => {
                if self.server.cert_file.is_empty() || self.server.key_file.is_empty() {
                    return Err(anyhow!(
                        "server.cert_file and server.key_file are required for doh mode"
                    ));
                }
                if self.server.path.is_empty() {
                    return Err(anyhow!("server.path is required for doh mode"));
                }
            }
            "dot" => {
                if self.server.cert_file.is_empty() || self.server.key_file.is_empty() {
                    return Err(anyhow!(
                        "server.cert_file and server.key_file are required for dot mode"
                    ));
                }
                if !self.server.path.is_empty() {
                    return Err(anyhow!("server.path is only valid for doh mode"));
                }
            }
            _ => {
                if !self.server.cert_file.is_empty() || !self.server.key_file.is_empty() {
                    return Err(anyhow!("TLS fields are only valid for doh or dot mode"));
                }
                if !self.server.path.is_empty() {
                    return Err(anyhow!("server.path is only valid for doh mode"));
                }
            }
        }
        if self.resolver.upstreams.is_empty() {
            return Err(anyhow!("resolver.upstreams must not be empty"));
        }

        let proxies = compile_proxy_definitions(&self.resolver.proxies)?;
        if !self.resolver.default_proxy.is_empty()
            && !proxies.contains_key(&self.resolver.default_proxy)
        {
            return Err(anyhow!(
                "resolver.default_proxy references unknown proxy {:?}",
                self.resolver.default_proxy
            ));
        }

        let mut names = HashSet::new();
        for (i, upstream) in self.resolver.upstreams.iter().enumerate() {
            if upstream.name.is_empty() {
                return Err(anyhow!("resolver.upstreams[{}].name is required", i));
            }
            if !names.insert(upstream.name.clone()) {
                return Err(anyhow!("duplicate upstream name {:?}", upstream.name));
            }
            if upstream.endpoint.is_empty() {
                return Err(anyhow!("resolver.upstreams[{}].endpoint is required", i));
            }
            let endpoint = parse_upstream_endpoint(&upstream.endpoint)?;
            if upstream_points_to_listener(&self.server.listen, &endpoint) {
                return Err(anyhow!(
                    "resolver.upstreams[{}].endpoint must not point to server.listen",
                    i
                ));
            }
            let proxy_name = if upstream.proxy.is_empty() {
                &self.resolver.default_proxy
            } else {
                &upstream.proxy
            };
            if !proxy_name.is_empty() && !proxies.contains_key(proxy_name) {
                return Err(anyhow!(
                    "resolver.upstreams[{}]: proxy references unknown proxy {:?}",
                    i,
                    proxy_name
                ));
            }
        }
        crate::dns::compile_routes(&self.resolver.routes, &names)?;
        crate::dns::compile_hosts(&self.resolver.hosts)?;

        if !self.resolver.timeout.is_empty() && parse_go_duration(&self.resolver.timeout)?.is_zero()
        {
            return Err(anyhow!("resolver.timeout must be > 0 when set"));
        }

        if self.cache.enabled {
            if self.cache.max_entries == 0 {
                return Err(anyhow!(
                    "cache.max_entries must be > 0 when cache is enabled"
                ));
            }
            if self.cache.max_entry_size == 0 {
                return Err(anyhow!(
                    "cache.max_entry_size must be > 0 when cache is enabled"
                ));
            }
            if self.cache.eviction_policy != "lru" {
                return Err(anyhow!("cache.eviction_policy must be one of: lru"));
            }
            if self.cache.max_ttl > 0 && self.cache.min_ttl > self.cache.max_ttl {
                return Err(anyhow!(
                    "cache.min_ttl must be <= cache.max_ttl when cache.max_ttl is set"
                ));
            }
        }

        if !self.healthcheck.interval.is_empty() {
            parse_go_duration(&self.healthcheck.interval).context("healthcheck.interval")?;
        }
        if !self.healthcheck.timeout.is_empty() {
            parse_go_duration(&self.healthcheck.timeout).context("healthcheck.timeout")?;
        }
        if self.healthcheck.enabled {
            if self.healthcheck.domain.is_empty() {
                return Err(anyhow!(
                    "healthcheck.domain must not be empty when healthcheck is enabled"
                ));
            }
            if self.healthcheck.failure_threshold == 0 {
                return Err(anyhow!(
                    "healthcheck.failure_threshold must be > 0 when healthcheck is enabled"
                ));
            }
            if self.healthcheck.recovery_threshold == 0 {
                return Err(anyhow!(
                    "healthcheck.recovery_threshold must be > 0 when healthcheck is enabled"
                ));
            }
        }
        match self.log.level.to_lowercase().as_str() {
            "debug" | "info" | "warn" | "error" => Ok(()),
            _ => Err(anyhow!("log.level must be one of debug,info,warn,error")),
        }
    }
}

fn upstream_points_to_listener(listen: &str, endpoint: &Url) -> bool {
    let Ok(listener) = listen.trim().parse::<SocketAddr>() else {
        return false;
    };
    let Some(upstream_port) = endpoint_port(endpoint) else {
        return false;
    };
    if upstream_port != listener.port() {
        return false;
    }
    let Some(upstream_ips) = endpoint_ips(endpoint) else {
        return false;
    };
    upstream_ips
        .into_iter()
        .any(|upstream_ip| addresses_overlap(listener.ip(), upstream_ip))
}

fn endpoint_port(endpoint: &Url) -> Option<u16> {
    endpoint.port().or_else(|| match endpoint.scheme() {
        "udp" | "tcp" => Some(53),
        "dot" | "doq" | "quic" => Some(853),
        "http" => Some(80),
        "https" => Some(443),
        _ => None,
    })
}

fn endpoint_ips(endpoint: &Url) -> Option<Vec<IpAddr>> {
    let host = endpoint.host_str()?;
    if host.eq_ignore_ascii_case("localhost") {
        return Some(vec![
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            IpAddr::V6(Ipv6Addr::LOCALHOST),
        ]);
    }
    host.parse::<IpAddr>().ok().map(|ip| vec![ip])
}

fn addresses_overlap(listener_ip: IpAddr, upstream_ip: IpAddr) -> bool {
    if listener_ip == upstream_ip {
        return true;
    }
    if listener_ip.is_unspecified() && same_ip_family(listener_ip, upstream_ip) {
        return upstream_ip.is_unspecified() || upstream_ip.is_loopback();
    }
    false
}

fn same_ip_family(a: IpAddr, b: IpAddr) -> bool {
    matches!(
        (a, b),
        (IpAddr::V4(_), IpAddr::V4(_)) | (IpAddr::V6(_), IpAddr::V6(_))
    )
}

pub fn parse_go_duration(raw: &str) -> Result<Duration> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err(anyhow!("duration is empty"));
    }
    let mut total = Duration::ZERO;
    let mut index = 0;
    let bytes = raw.as_bytes();
    while index < bytes.len() {
        let start = index;
        while index < bytes.len() && (bytes[index].is_ascii_digit() || bytes[index] == b'.') {
            index += 1;
        }
        if start == index {
            return Err(anyhow!("invalid duration {:?}", raw));
        }
        let number: f64 = raw[start..index].parse().context("parse duration number")?;
        let unit_start = index;
        while index < bytes.len() && !bytes[index].is_ascii_digit() && bytes[index] != b'.' {
            index += 1;
        }
        let unit = &raw[unit_start..index];
        let seconds = match unit {
            "ns" => number / 1_000_000_000.0,
            "us" => number / 1_000_000.0,
            "ms" => number / 1_000.0,
            "s" => number,
            "m" => number * 60.0,
            "h" => number * 3600.0,
            _ => return Err(anyhow!("unknown duration unit {:?}", unit)),
        };
        total += Duration::from_secs_f64(seconds);
    }
    Ok(total)
}

pub fn parse_upstream_endpoint(raw: &str) -> Result<Url> {
    let url = Url::parse(raw).with_context(|| format!("invalid endpoint {:?}", raw))?;
    match url.scheme() {
        "udp" | "tcp" | "dot" | "doq" | "quic" | "http" | "https" => {}
        scheme => return Err(anyhow!("unsupported endpoint scheme {:?}", scheme)),
    }
    if url.host_str().unwrap_or("").is_empty() {
        return Err(anyhow!("endpoint {:?} is missing host", raw));
    }
    if url.scheme() != "http" && url.scheme() != "https" {
        let path = url.path();
        if !path.is_empty() && path != "/" {
            return Err(anyhow!(
                "endpoint {:?} must not include path for {}",
                raw,
                url.scheme()
            ));
        }
        if url.query().is_some() {
            return Err(anyhow!(
                "endpoint {:?} must not include query for {}",
                raw,
                url.scheme()
            ));
        }
    }
    Ok(url)
}

fn compile_proxy_definitions(items: &[CoreProxyConfig]) -> Result<HashMap<String, Url>> {
    let mut proxies = HashMap::new();
    for (i, item) in items.iter().enumerate() {
        if item.name.is_empty() {
            return Err(anyhow!("resolver.proxies[{}].name is required", i));
        }
        if proxies.contains_key(&item.name) {
            return Err(anyhow!("duplicate proxy name {:?}", item.name));
        }
        if item.endpoint.is_empty() {
            return Err(anyhow!("resolver.proxies[{}].endpoint is required", i));
        }
        let url = Url::parse(&item.endpoint).with_context(|| format!("resolver.proxies[{}]", i))?;
        if url.scheme() != "socks5" {
            return Err(anyhow!(
                "resolver.proxies[{}]: unsupported proxy scheme {:?}; only socks5 is supported",
                i,
                url.scheme()
            ));
        }
        if url.host_str().unwrap_or("").is_empty() {
            return Err(anyhow!(
                "resolver.proxies[{}]: proxy endpoint is missing host",
                i
            ));
        }
        if !url.path().is_empty() && url.path() != "/" {
            return Err(anyhow!(
                "resolver.proxies[{}]: proxy endpoint must not include path",
                i
            ));
        }
        if url.query().is_some() {
            return Err(anyhow!(
                "resolver.proxies[{}]: proxy endpoint must not include query",
                i
            ));
        }
        proxies.insert(item.name.clone(), url);
    }
    Ok(proxies)
}

fn split_proxy_endpoint_auth(raw: &str) -> (String, String, String) {
    let Ok(mut url) = Url::parse(raw) else {
        return (raw.to_string(), String::new(), String::new());
    };
    let username = url.username().to_string();
    let password = url.password().unwrap_or("").to_string();
    if username.is_empty() {
        return (raw.to_string(), String::new(), String::new());
    }
    let _ = url.set_username("");
    let _ = url.set_password(None);
    (url.to_string(), username, password)
}

fn proxy_endpoint_with_auth(raw: &str, username: &str, password: &str) -> String {
    let username = username.trim();
    if username.is_empty() {
        return raw.to_string();
    }
    let Ok(mut url) = Url::parse(raw) else {
        return raw.to_string();
    };
    let _ = url.set_username(username);
    let _ = url.set_password(Some(password));
    url.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config_with_listener_and_upstream(listen: &str, endpoint: &str) -> CoreConfig {
        let mut cfg = default_local_config();
        cfg.server.listen = listen.into();
        cfg.resolver.upstreams = vec![CoreUpstreamConfig {
            name: "test".into(),
            endpoint: endpoint.into(),
            ..Default::default()
        }];
        cfg
    }

    #[test]
    fn validate_rejects_upstream_pointing_to_listener() {
        let cfg = config_with_listener_and_upstream("127.0.0.1:15353", "udp://127.0.0.1:15353");

        let err = cfg
            .validate()
            .expect_err("upstream points back to listener");

        assert!(err
            .to_string()
            .contains("endpoint must not point to server.listen"));
    }

    #[test]
    fn validate_rejects_upstream_with_implicit_dns_port_pointing_to_listener() {
        let cfg = config_with_listener_and_upstream("127.0.0.1:53", "udp://127.0.0.1");

        let err = cfg
            .validate()
            .expect_err("implicit port points back to listener");

        assert!(err
            .to_string()
            .contains("endpoint must not point to server.listen"));
    }

    #[test]
    fn validate_rejects_localhost_upstream_pointing_to_listener() {
        let cfg = config_with_listener_and_upstream("127.0.0.1:15353", "tcp://localhost:15353");

        let err = cfg
            .validate()
            .expect_err("localhost points back to listener");

        assert!(err
            .to_string()
            .contains("endpoint must not point to server.listen"));
    }

    #[test]
    fn validate_rejects_loopback_upstream_when_listener_is_unspecified() {
        let cfg = config_with_listener_and_upstream("0.0.0.0:53", "udp://127.0.0.1:53");

        let err = cfg
            .validate()
            .expect_err("loopback points back to unspecified listener");

        assert!(err
            .to_string()
            .contains("endpoint must not point to server.listen"));
    }

    #[test]
    fn validate_allows_public_upstream_on_same_port_when_listener_is_unspecified() {
        let cfg = config_with_listener_and_upstream("0.0.0.0:53", "udp://1.1.1.1:53");

        cfg.validate()
            .expect("public upstream is not the local listener");
    }
}
