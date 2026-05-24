use crate::config::{
    core_config_from_desktop, database_path, default_local_config, desktop_config_from_core,
    validate_desktop_config, CoreConfig,
};
use crate::desktop::{
    ConfigDocument, DesktopConfig, DesktopHostStatus, DesktopRouteStatus, SystemDnsSettings,
};
use anyhow::{anyhow, Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use std::path::{Path, PathBuf};
use url::Url;

const CONFIG_DOCUMENT_PATH: &str = "local://autodns/config";

#[derive(Clone)]
pub struct ConfigStore {
    path: PathBuf,
}

#[derive(Clone)]
struct ParsedRoute {
    match_type: String,
    domain: String,
    upstreams: Vec<String>,
}

#[derive(Default)]
struct EndpointParts {
    protocol: String,
    host: String,
    port: String,
    path: String,
}

impl ConfigStore {
    pub fn open_default() -> Result<Self> {
        let path = database_path()?;
        Self::open(path)
    }

    pub fn open(path: PathBuf) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).context("create config directory")?;
        }
        let store = Self { path };
        store.with_conn(|conn| {
            migrate(conn)?;
            if is_empty(conn)? {
                replace_config(conn, desktop_config_from_core(&default_local_config()))?;
            }
            Ok(())
        })?;
        Ok(store)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn load_document(&self) -> Result<ConfigDocument> {
        let config = self.with_conn(|conn| load_desktop_config(conn))?;
        Ok(ConfigDocument {
            path: CONFIG_DOCUMENT_PATH.to_string(),
            config,
        })
    }

    pub fn save_document(&self, doc: ConfigDocument) -> Result<()> {
        self.save_config(doc.config)
    }

    pub fn save_config(&self, config: DesktopConfig) -> Result<()> {
        validate_store_config(&config)?;
        self.with_conn(|conn| replace_config(conn, config))
    }

    pub fn validate_config(&self, config: DesktopConfig) -> Result<()> {
        validate_store_config(&config)
    }

    pub fn load_system_dns_settings(&self) -> Result<SystemDnsSettings> {
        self.with_conn(load_system_dns_settings)
    }

    pub fn save_system_dns_settings(&self, settings: SystemDnsSettings) -> Result<()> {
        self.with_conn(|conn| save_system_dns_settings(conn, settings))
    }

    pub fn load_system_dns_adapter_state(&self, adapter_id: &str) -> Result<SystemDnsAdapterState> {
        self.with_conn(|conn| load_system_dns_adapter_state(conn, adapter_id))
    }

    pub fn mark_system_dns_applied(&self, adapter_id: &str, original_dns: &[String]) -> Result<()> {
        self.with_conn(|conn| mark_system_dns_applied(conn, adapter_id, original_dns))
    }

    pub fn mark_system_dns_restored(&self, adapter_id: &str) -> Result<()> {
        self.with_conn(|conn| mark_system_dns_restored(conn, adapter_id))
    }

    pub fn mark_system_dns_error(&self, adapter_id: &str, err: &str) -> Result<()> {
        self.with_conn(|conn| mark_system_dns_error(conn, adapter_id, err))
    }

    pub fn runtime_config(&self) -> Result<CoreConfig> {
        let mut cfg = self.with_conn(|conn| load_runtime_config(conn))?;
        cfg.apply_defaults();
        cfg.validate()?;
        Ok(cfg)
    }

    fn with_conn<T>(&self, f: impl FnOnce(&mut Connection) -> Result<T>) -> Result<T> {
        let mut conn = Connection::open(&self.path)
            .with_context(|| format!("open database: {}", self.path.display()))?;
        f(&mut conn)
    }
}

#[derive(Clone, Debug, Default)]
pub struct SystemDnsAdapterState {
    pub managed: bool,
    pub original_dns: Vec<String>,
    pub last_applied_at: Option<String>,
    pub last_restored_at: Option<String>,
    pub last_error: Option<String>,
}

fn migrate(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        PRAGMA foreign_keys = ON;

        CREATE TABLE IF NOT EXISTS app_settings (
            id INTEGER PRIMARY KEY CHECK (id = 1),
            server_mode TEXT NOT NULL,
            server_listen TEXT NOT NULL,
            server_cert_file TEXT NOT NULL DEFAULT '',
            server_key_file TEXT NOT NULL DEFAULT '',
            server_path TEXT NOT NULL DEFAULT '',
            default_proxy TEXT NOT NULL DEFAULT '',
            resolver_timeout TEXT NOT NULL DEFAULT '',
            fallback_system_dns INTEGER NOT NULL DEFAULT 0,
            ipv6_enabled INTEGER NOT NULL DEFAULT 1,
            cache_enabled INTEGER NOT NULL DEFAULT 1,
            cache_max_entries INTEGER NOT NULL DEFAULT 10000,
            cache_max_entry_size INTEGER NOT NULL DEFAULT 4096,
            cache_min_ttl INTEGER NOT NULL DEFAULT 10,
            cache_max_ttl INTEGER NOT NULL DEFAULT 600,
            cache_negative_ttl INTEGER NOT NULL DEFAULT 30,
            cache_eviction_policy TEXT NOT NULL DEFAULT 'lru',
            healthcheck_enabled INTEGER NOT NULL DEFAULT 1,
            healthcheck_interval TEXT NOT NULL DEFAULT '30s',
            healthcheck_timeout TEXT NOT NULL DEFAULT '2s',
            healthcheck_domain TEXT NOT NULL DEFAULT '.',
            healthcheck_failure_threshold INTEGER NOT NULL DEFAULT 3,
            healthcheck_recovery_threshold INTEGER NOT NULL DEFAULT 2,
            log_level TEXT NOT NULL DEFAULT 'info',
            updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        );

        CREATE TABLE IF NOT EXISTS proxies (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL UNIQUE,
            endpoint TEXT NOT NULL,
            protocol TEXT NOT NULL DEFAULT '',
            host TEXT NOT NULL DEFAULT '',
            port INTEGER,
            username TEXT NOT NULL DEFAULT '',
            password TEXT NOT NULL DEFAULT '',
            sort_order INTEGER NOT NULL,
            enabled INTEGER NOT NULL DEFAULT 1,
            deleted_at TEXT
        );

        CREATE TABLE IF NOT EXISTS upstreams (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL UNIQUE,
            endpoint TEXT NOT NULL,
            protocol TEXT NOT NULL DEFAULT '',
            host TEXT NOT NULL DEFAULT '',
            port INTEGER,
            path TEXT NOT NULL DEFAULT '',
            server_name TEXT NOT NULL DEFAULT '',
            proxy_name TEXT NOT NULL DEFAULT '',
            sort_order INTEGER NOT NULL,
            enabled INTEGER NOT NULL DEFAULT 1,
            deleted_at TEXT
        );

        CREATE TABLE IF NOT EXISTS host_rules (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            domain TEXT NOT NULL,
            ips TEXT NOT NULL,
            sort_order INTEGER NOT NULL,
            enabled INTEGER NOT NULL DEFAULT 1,
            note TEXT NOT NULL DEFAULT ''
        );

        CREATE TABLE IF NOT EXISTS route_rules (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            match_type TEXT NOT NULL,
            domain TEXT NOT NULL,
            sort_order INTEGER NOT NULL,
            enabled INTEGER NOT NULL DEFAULT 1,
            invalid_reason TEXT NOT NULL DEFAULT '',
            note TEXT NOT NULL DEFAULT ''
        );

        CREATE TABLE IF NOT EXISTS route_rule_upstreams (
            route_rule_id INTEGER NOT NULL,
            upstream_name TEXT NOT NULL,
            sort_order INTEGER NOT NULL,
            enabled INTEGER NOT NULL DEFAULT 1,
            PRIMARY KEY (route_rule_id, sort_order),
            FOREIGN KEY (route_rule_id) REFERENCES route_rules(id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS system_dns_settings (
            id INTEGER PRIMARY KEY CHECK (id = 1),
            enabled INTEGER NOT NULL DEFAULT 0,
            target_servers TEXT NOT NULL DEFAULT '[]',
            selected_adapter_ids TEXT NOT NULL DEFAULT '[]',
            last_error TEXT,
            updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        );

        CREATE TABLE IF NOT EXISTS system_dns_adapter_state (
            adapter_id TEXT PRIMARY KEY,
            managed INTEGER NOT NULL DEFAULT 0,
            original_dns TEXT NOT NULL DEFAULT '[]',
            last_applied_at TEXT,
            last_restored_at TEXT,
            last_error TEXT,
            updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        );
        "#,
    )
    .context("migrate database")?;
    add_column_if_missing(
        conn,
        "app_settings",
        "ipv6_enabled",
        "INTEGER NOT NULL DEFAULT 1",
    )?;
    add_column_if_missing(conn, "proxies", "protocol", "TEXT NOT NULL DEFAULT ''")?;
    add_column_if_missing(conn, "proxies", "host", "TEXT NOT NULL DEFAULT ''")?;
    add_column_if_missing(conn, "proxies", "port", "INTEGER")?;
    add_column_if_missing(conn, "upstreams", "protocol", "TEXT NOT NULL DEFAULT ''")?;
    add_column_if_missing(conn, "upstreams", "host", "TEXT NOT NULL DEFAULT ''")?;
    add_column_if_missing(conn, "upstreams", "port", "INTEGER")?;
    add_column_if_missing(conn, "upstreams", "path", "TEXT NOT NULL DEFAULT ''")?;
    conn.execute(
        r#"
        INSERT OR IGNORE INTO system_dns_settings (id, enabled, target_servers, selected_adapter_ids)
        VALUES (1, 0, '[]', '[]')
        "#,
        [],
    )
    .context("seed system dns settings")?;
    Ok(())
}

fn is_empty(conn: &Connection) -> Result<bool> {
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM app_settings", [], |row| row.get(0))?;
    Ok(count == 0)
}

fn add_column_if_missing(
    conn: &Connection,
    table: &str,
    column: &str,
    definition: &str,
) -> Result<()> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
    for name in rows {
        if name? == column {
            return Ok(());
        }
    }
    conn.execute(
        &format!("ALTER TABLE {table} ADD COLUMN {column} {definition}"),
        [],
    )
    .with_context(|| format!("add column {table}.{column}"))?;
    Ok(())
}

fn replace_config(conn: &mut Connection, config: DesktopConfig) -> Result<()> {
    let tx = conn.transaction().context("begin config transaction")?;
    tx.execute_batch(
        r#"
        DELETE FROM route_rule_upstreams;
        DELETE FROM route_rules;
        DELETE FROM host_rules;
        DELETE FROM upstreams;
        DELETE FROM proxies;
        DELETE FROM app_settings;
        "#,
    )
    .context("clear config tables")?;

    tx.execute(
        r#"
        INSERT INTO app_settings (
            id, server_mode, server_listen, server_cert_file, server_key_file, server_path,
            default_proxy, resolver_timeout, fallback_system_dns, ipv6_enabled,
            cache_enabled, cache_max_entries, cache_max_entry_size, cache_min_ttl, cache_max_ttl,
            cache_negative_ttl, cache_eviction_policy,
            healthcheck_enabled, healthcheck_interval, healthcheck_timeout, healthcheck_domain,
            healthcheck_failure_threshold, healthcheck_recovery_threshold, log_level
        )
        VALUES (1, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
        params![
            config.server.mode,
            config.server.listen,
            config.server.cert_file,
            config.server.key_file,
            config.server.path,
            config.resolver.default_proxy,
            config.resolver.timeout,
            bool_to_i64(config.resolver.fallback_system_dns),
            bool_to_i64(config.resolver.ipv6_enabled),
            bool_to_i64(config.cache.enabled),
            usize_to_i64(config.cache.max_entries)?,
            usize_to_i64(config.cache.max_entry_size)?,
            u32_to_i64(config.cache.min_ttl),
            u32_to_i64(config.cache.max_ttl),
            u32_to_i64(config.cache.negative_ttl),
            config.cache.eviction_policy,
            bool_to_i64(config.healthcheck.enabled),
            config.healthcheck.interval,
            config.healthcheck.timeout,
            config.healthcheck.domain,
            u32_to_i64(config.healthcheck.failure_threshold),
            u32_to_i64(config.healthcheck.recovery_threshold),
            config.log.level,
        ],
    )
    .context("save app settings")?;

    for (index, proxy) in config.resolver.proxies.into_iter().enumerate() {
        let endpoint = proxy_endpoint_from_fields(&proxy);
        tx.execute(
            r#"
            INSERT INTO proxies (name, endpoint, protocol, host, port, username, password, sort_order, enabled)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, 1)
            "#,
            params![
                proxy.name,
                endpoint,
                proxy.protocol,
                proxy.host,
                port_to_i64(&proxy.port)?,
                proxy.username,
                proxy.password,
                usize_to_i64(index)?
            ],
        )
        .with_context(|| format!("save proxy {}", index + 1))?;
    }

    for (index, upstream) in config.resolver.upstreams.into_iter().enumerate() {
        let endpoint = upstream_endpoint_from_fields(&upstream);
        tx.execute(
            r#"
            INSERT INTO upstreams (name, endpoint, protocol, host, port, path, server_name, proxy_name, sort_order, enabled)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 1)
            "#,
            params![
                upstream.name,
                endpoint,
                upstream.protocol,
                upstream.host,
                port_to_i64(&upstream.port)?,
                upstream.path,
                upstream.server_name,
                upstream.proxy,
                usize_to_i64(index)?
            ],
        )
        .with_context(|| format!("save upstream {}", index + 1))?;
    }

    for (index, raw) in config.resolver.hosts.into_iter().enumerate() {
        let status =
            config
                .resolver
                .host_statuses
                .get(index)
                .cloned()
                .unwrap_or(DesktopHostStatus {
                    index,
                    enabled: true,
                    note: String::new(),
                });
        let (domain, ips) = parse_host_rule(&raw)?;
        tx.execute(
            "INSERT INTO host_rules (domain, ips, sort_order, enabled, note) VALUES (?, ?, ?, ?, ?)",
            params![
                domain,
                ips,
                usize_to_i64(index)?,
                bool_to_i64(status.enabled),
                status.note
            ],
        )
        .with_context(|| format!("save host rule {}", index + 1))?;
    }

    for (index, raw) in config.resolver.routes.into_iter().enumerate() {
        let status = config
            .resolver
            .route_statuses
            .get(index)
            .cloned()
            .unwrap_or(DesktopRouteStatus {
                index,
                enabled: true,
                invalid_reason: String::new(),
                note: String::new(),
            });
        let route = parse_route_rule(&raw)?;
        let invalid_reason = route_invalid_reason(&tx, &route)?;
        tx.execute(
            r#"
            INSERT INTO route_rules (match_type, domain, sort_order, enabled, invalid_reason, note)
            VALUES (?, ?, ?, ?, ?, ?)
            "#,
            params![
                &route.match_type,
                &route.domain,
                usize_to_i64(index)?,
                bool_to_i64(status.enabled),
                &invalid_reason,
                &status.note,
            ],
        )
        .with_context(|| format!("save route rule {}", index + 1))?;
        let route_id = tx.last_insert_rowid();
        for (upstream_index, upstream) in route.upstreams.into_iter().enumerate() {
            let enabled = upstream_exists(&tx, &upstream)?;
            tx.execute(
                r#"
                INSERT INTO route_rule_upstreams (route_rule_id, upstream_name, sort_order, enabled)
                VALUES (?, ?, ?, ?)
                "#,
                params![
                    route_id,
                    upstream,
                    usize_to_i64(upstream_index)?,
                    bool_to_i64(enabled)
                ],
            )
            .with_context(|| format!("save route upstream {}", upstream_index + 1))?;
        }
    }

    tx.commit().context("commit config transaction")?;
    Ok(())
}

fn load_desktop_config(conn: &Connection) -> Result<DesktopConfig> {
    let mut cfg = load_settings(conn)?;
    cfg.resolver.proxies = load_desktop_proxies(conn)?;
    cfg.resolver.upstreams = load_desktop_upstreams(conn)?;
    cfg.resolver.hosts = load_desktop_hosts(conn, false)?;
    cfg.resolver.host_statuses = load_desktop_host_statuses(conn)?;
    cfg.resolver.routes = load_desktop_routes(conn, false)?;
    cfg.resolver.route_statuses = load_desktop_route_statuses(conn)?;
    Ok(cfg)
}

fn load_runtime_config(conn: &Connection) -> Result<CoreConfig> {
    let mut desktop = load_desktop_config(conn)?;
    desktop.resolver.hosts = load_desktop_hosts(conn, true)?;
    desktop.resolver.routes = load_desktop_routes(conn, true)?;
    let mut core = core_config_from_desktop(desktop);
    core.apply_defaults();
    Ok(core)
}

fn load_settings(conn: &Connection) -> Result<DesktopConfig> {
    Ok(conn
        .query_row(
            r#"
        SELECT
            server_mode, server_listen, server_cert_file, server_key_file, server_path,
            default_proxy, resolver_timeout, fallback_system_dns, ipv6_enabled,
            cache_enabled, cache_max_entries, cache_max_entry_size, cache_min_ttl, cache_max_ttl,
            cache_negative_ttl, cache_eviction_policy,
            healthcheck_enabled, healthcheck_interval, healthcheck_timeout, healthcheck_domain,
            healthcheck_failure_threshold, healthcheck_recovery_threshold, log_level
        FROM app_settings WHERE id = 1
        "#,
            [],
            |row| {
                Ok(DesktopConfig {
                    server: crate::desktop::DesktopServerConfig {
                        mode: row.get(0)?,
                        listen: row.get(1)?,
                        cert_file: row.get(2)?,
                        key_file: row.get(3)?,
                        path: row.get(4)?,
                    },
                    resolver: crate::desktop::DesktopResolverConfig {
                        upstreams: Vec::new(),
                        proxies: Vec::new(),
                        default_proxy: row.get(5)?,
                        hosts: Vec::new(),
                        host_statuses: Vec::new(),
                        routes: Vec::new(),
                        route_statuses: Vec::new(),
                        timeout: row.get(6)?,
                        fallback_system_dns: int_to_bool(row.get(7)?),
                        ipv6_enabled: int_to_bool(row.get(8)?),
                    },
                    cache: crate::desktop::DesktopCacheConfig {
                        enabled: int_to_bool(row.get(9)?),
                        max_entries: i64_to_usize(row.get(10)?)?,
                        max_entry_size: i64_to_usize(row.get(11)?)?,
                        min_ttl: i64_to_u32(row.get(12)?)?,
                        max_ttl: i64_to_u32(row.get(13)?)?,
                        negative_ttl: i64_to_u32(row.get(14)?)?,
                        eviction_policy: row.get(15)?,
                    },
                    healthcheck: crate::desktop::DesktopHealthcheckConfig {
                        enabled: int_to_bool(row.get(16)?),
                        interval: row.get(17)?,
                        timeout: row.get(18)?,
                        domain: row.get(19)?,
                        failure_threshold: i64_to_u32(row.get(20)?)?,
                        recovery_threshold: i64_to_u32(row.get(21)?)?,
                    },
                    log: crate::desktop::DesktopLogConfig {
                        level: row.get(22)?,
                    },
                })
            },
        )
        .context("load app settings")?)
}

fn load_desktop_proxies(conn: &Connection) -> Result<Vec<crate::desktop::DesktopProxyConfig>> {
    let mut stmt = conn.prepare(
        r#"
        SELECT name, endpoint, protocol, host, port, username, password
        FROM proxies
        WHERE deleted_at IS NULL
        ORDER BY sort_order, id
        "#,
    )?;
    let rows = stmt.query_map([], |row| {
        let endpoint: String = row.get(1)?;
        let mut parts = parse_endpoint_parts_lossy(&endpoint);
        let protocol: String = row.get(2)?;
        let host: String = row.get(3)?;
        let port: Option<i64> = row.get(4)?;
        if !protocol.is_empty() {
            parts.protocol = protocol;
        }
        if !host.is_empty() {
            parts.host = host;
        }
        if let Some(port) = port {
            parts.port = port.to_string();
        }
        Ok(crate::desktop::DesktopProxyConfig {
            name: row.get(0)?,
            protocol: parts.protocol,
            host: parts.host,
            port: parts.port,
            username: row.get(5)?,
            password: row.get(6)?,
        })
    })?;
    collect_rows(rows)
}

fn load_desktop_upstreams(conn: &Connection) -> Result<Vec<crate::desktop::DesktopUpstreamConfig>> {
    let mut stmt = conn.prepare(
        r#"
        SELECT name, endpoint, protocol, host, port, path, server_name, proxy_name
        FROM upstreams
        WHERE deleted_at IS NULL
        ORDER BY sort_order, id
        "#,
    )?;
    let rows = stmt.query_map([], |row| {
        let endpoint: String = row.get(1)?;
        let mut parts = parse_endpoint_parts_lossy(&endpoint);
        let protocol: String = row.get(2)?;
        let host: String = row.get(3)?;
        let port: Option<i64> = row.get(4)?;
        let path: String = row.get(5)?;
        if !protocol.is_empty() {
            parts.protocol = protocol;
        }
        if !host.is_empty() {
            parts.host = host;
        }
        if let Some(port) = port {
            parts.port = port.to_string();
        }
        if !path.is_empty() {
            parts.path = path;
        }
        Ok(crate::desktop::DesktopUpstreamConfig {
            name: row.get(0)?,
            protocol: parts.protocol,
            host: parts.host,
            port: parts.port,
            path: parts.path,
            server_name: row.get(6)?,
            proxy: row.get(7)?,
        })
    })?;
    collect_rows(rows)
}

fn load_desktop_hosts(conn: &Connection, runtime_only: bool) -> Result<Vec<String>> {
    let sql = if runtime_only {
        r#"
        SELECT domain, ips
        FROM host_rules
        WHERE enabled = 1
        ORDER BY sort_order, id
        "#
    } else {
        r#"
        SELECT domain, ips
        FROM host_rules
        ORDER BY sort_order, id
        "#
    };
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map([], |row| {
        Ok(format!(
            "{}={}",
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?
        ))
    })?;
    collect_rows(rows)
}

fn load_desktop_host_statuses(conn: &Connection) -> Result<Vec<DesktopHostStatus>> {
    let mut stmt = conn.prepare(
        r#"
        SELECT enabled, note
        FROM host_rules
        ORDER BY sort_order, id
        "#,
    )?;
    let rows = stmt.query_map([], |row| {
        let index = 0;
        Ok(DesktopHostStatus {
            index,
            enabled: int_to_bool(row.get(0)?),
            note: row.get(1)?,
        })
    })?;
    let mut statuses = collect_rows(rows)?;
    for (index, status) in statuses.iter_mut().enumerate() {
        status.index = index;
    }
    Ok(statuses)
}

fn load_desktop_routes(conn: &Connection, runtime_only: bool) -> Result<Vec<String>> {
    let sql = if runtime_only {
        r#"
        SELECT id, match_type, domain
        FROM route_rules
        WHERE enabled = 1 AND invalid_reason = ''
        ORDER BY sort_order, id
        "#
    } else {
        r#"
        SELECT id, match_type, domain
        FROM route_rules
        ORDER BY sort_order, id
        "#
    };
    let mut stmt = conn.prepare(sql)?;
    let mut routes = Vec::new();
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let id: i64 = row.get(0)?;
        let match_type: String = row.get(1)?;
        let domain: String = row.get(2)?;
        let upstreams = route_upstreams(conn, id, runtime_only)?;
        if runtime_only && upstreams.is_empty() {
            continue;
        }
        routes.push(format!("{match_type}:{domain}={}", upstreams.join(",")));
    }
    Ok(routes)
}

fn load_desktop_route_statuses(conn: &Connection) -> Result<Vec<DesktopRouteStatus>> {
    let mut stmt = conn.prepare(
        r#"
        SELECT enabled, invalid_reason, note
        FROM route_rules
        ORDER BY sort_order, id
        "#,
    )?;
    let rows = stmt.query_map([], |row| {
        let index = 0;
        Ok(DesktopRouteStatus {
            index,
            enabled: int_to_bool(row.get(0)?),
            invalid_reason: row.get(1)?,
            note: row.get(2)?,
        })
    })?;
    let mut statuses = collect_rows(rows)?;
    for (index, status) in statuses.iter_mut().enumerate() {
        status.index = index;
    }
    Ok(statuses)
}

fn route_upstreams(conn: &Connection, route_id: i64, runtime_only: bool) -> Result<Vec<String>> {
    let sql = if runtime_only {
        r#"
        SELECT ru.upstream_name
        FROM route_rule_upstreams ru
        JOIN upstreams u ON u.name = ru.upstream_name AND u.enabled = 1 AND u.deleted_at IS NULL
        WHERE ru.route_rule_id = ? AND ru.enabled = 1
        ORDER BY ru.sort_order
        "#
    } else {
        r#"
        SELECT upstream_name
        FROM route_rule_upstreams
        WHERE route_rule_id = ?
        ORDER BY sort_order
        "#
    };
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(params![route_id], |row| row.get(0))?;
    collect_rows(rows)
}

fn validate_store_config(config: &DesktopConfig) -> Result<()> {
    let mut runtime = config.clone();
    let upstream_names: Vec<String> = runtime
        .resolver
        .upstreams
        .iter()
        .map(|upstream| upstream.name.clone())
        .collect();
    let routes = runtime.resolver.routes.clone();
    runtime.resolver.routes = routes
        .iter()
        .filter_map(|raw| {
            let route = parse_route_rule(raw).ok()?;
            let upstreams: Vec<String> = route
                .upstreams
                .into_iter()
                .filter(|name| upstream_names.iter().any(|upstream| upstream == name))
                .collect();
            (!upstreams.is_empty()).then(|| {
                format!(
                    "{}:{}={}",
                    route.match_type,
                    route.domain,
                    upstreams.join(",")
                )
            })
        })
        .collect();
    validate_desktop_config(runtime)
}

fn parse_host_rule(raw: &str) -> Result<(String, String)> {
    let Some((domain, ips)) = raw.split_once('=') else {
        return Err(anyhow!("host rule must contain '='"));
    };
    Ok((domain.trim().to_string(), ips.trim().to_string()))
}

fn parse_endpoint_parts_lossy(raw: &str) -> EndpointParts {
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

fn upstream_endpoint_from_fields(upstream: &crate::desktop::DesktopUpstreamConfig) -> String {
    let protocol = if upstream.protocol.trim().is_empty() {
        "udp"
    } else {
        upstream.protocol.trim()
    };
    let address = endpoint_address(upstream.host.trim(), upstream.port.trim());
    if protocol == "http" || protocol == "https" {
        format!(
            "{protocol}://{address}{}",
            normalize_doh_path(&upstream.path)
        )
    } else {
        format!("{protocol}://{address}")
    }
}

fn proxy_endpoint_from_fields(proxy: &crate::desktop::DesktopProxyConfig) -> String {
    let protocol = if proxy.protocol.trim().is_empty() {
        "socks5"
    } else {
        proxy.protocol.trim()
    };
    format!(
        "{protocol}://{}",
        endpoint_address(proxy.host.trim(), proxy.port.trim())
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

fn port_to_i64(port: &str) -> Result<Option<i64>> {
    let value = port.trim();
    if value.is_empty() {
        return Ok(None);
    }
    let parsed = value
        .parse::<u16>()
        .with_context(|| format!("parse port {value:?}"))?;
    Ok(Some(i64::from(parsed)))
}

fn parse_route_rule(raw: &str) -> Result<ParsedRoute> {
    let Some((match_type, rest)) = raw.split_once(':') else {
        return Err(anyhow!("route rule must contain ':'"));
    };
    let Some((domain, upstreams)) = rest.split_once('=') else {
        return Err(anyhow!("route rule must contain '='"));
    };
    let upstreams = upstreams
        .split(',')
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(ToString::to_string)
        .collect();
    Ok(ParsedRoute {
        match_type: match_type.trim().to_string(),
        domain: domain.trim().to_string(),
        upstreams,
    })
}

fn route_invalid_reason(conn: &Connection, route: &ParsedRoute) -> Result<String> {
    if route.upstreams.is_empty() {
        return Ok("no upstream selected".to_string());
    }
    let mut missing = Vec::new();
    for upstream in &route.upstreams {
        if !upstream_exists(conn, upstream)? {
            missing.push(upstream.clone());
        }
    }
    if missing.is_empty() {
        Ok(String::new())
    } else {
        Ok(format!("missing upstream: {}", missing.join(",")))
    }
}

fn upstream_exists(conn: &Connection, name: &str) -> Result<bool> {
    let exists = conn
        .query_row(
            "SELECT 1 FROM upstreams WHERE name = ? AND enabled = 1 AND deleted_at IS NULL LIMIT 1",
            params![name],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    Ok(exists)
}

fn load_system_dns_settings(conn: &mut Connection) -> Result<SystemDnsSettings> {
    conn.query_row(
        "SELECT enabled, target_servers, selected_adapter_ids FROM system_dns_settings WHERE id = 1",
        [],
        |row| {
            let target_servers: String = row.get(1)?;
            let selected_adapter_ids: String = row.get(2)?;
            Ok(SystemDnsSettings {
                enabled: int_to_bool(row.get(0)?),
                target_servers: json_vec_from_sql(&target_servers)?,
                selected_adapter_ids: json_vec_from_sql(&selected_adapter_ids)?,
            })
        },
    )
    .optional()?
    .map(Ok)
    .unwrap_or_else(|| Ok(SystemDnsSettings::default()))
}

fn save_system_dns_settings(conn: &mut Connection, settings: SystemDnsSettings) -> Result<()> {
    conn.execute(
        r#"
        INSERT INTO system_dns_settings (id, enabled, target_servers, selected_adapter_ids, updated_at)
        VALUES (1, ?, ?, ?, CURRENT_TIMESTAMP)
        ON CONFLICT(id) DO UPDATE SET
            enabled = excluded.enabled,
            target_servers = excluded.target_servers,
            selected_adapter_ids = excluded.selected_adapter_ids,
            updated_at = excluded.updated_at
        "#,
        params![
            bool_to_i64(settings.enabled),
            serde_json::to_string(&settings.target_servers).context("serialize target servers")?,
            serde_json::to_string(&settings.selected_adapter_ids).context("serialize selected adapters")?,
        ],
    )
    .context("save system dns settings")?;
    Ok(())
}

fn load_system_dns_adapter_state(
    conn: &mut Connection,
    adapter_id: &str,
) -> Result<SystemDnsAdapterState> {
    conn.query_row(
        r#"
        SELECT managed, original_dns, last_applied_at, last_restored_at, last_error
        FROM system_dns_adapter_state
        WHERE adapter_id = ?
        "#,
        params![adapter_id],
        |row| {
            let original_dns: String = row.get(1)?;
            Ok(SystemDnsAdapterState {
                managed: int_to_bool(row.get(0)?),
                original_dns: json_vec_from_sql(&original_dns)?,
                last_applied_at: row.get(2)?,
                last_restored_at: row.get(3)?,
                last_error: row.get(4)?,
            })
        },
    )
    .optional()?
    .map(Ok)
    .unwrap_or_else(|| Ok(SystemDnsAdapterState::default()))
}

fn mark_system_dns_applied(
    conn: &mut Connection,
    adapter_id: &str,
    original_dns: &[String],
) -> Result<()> {
    conn.execute(
        r#"
        INSERT INTO system_dns_adapter_state (
            adapter_id, managed, original_dns, last_applied_at, last_restored_at, last_error, updated_at
        )
        VALUES (?, 1, ?, CURRENT_TIMESTAMP, NULL, NULL, CURRENT_TIMESTAMP)
        ON CONFLICT(adapter_id) DO UPDATE SET
            managed = 1,
            original_dns = CASE
                WHEN system_dns_adapter_state.managed = 1 THEN system_dns_adapter_state.original_dns
                ELSE excluded.original_dns
            END,
            last_applied_at = CURRENT_TIMESTAMP,
            last_error = NULL,
            updated_at = CURRENT_TIMESTAMP
        "#,
        params![
            adapter_id,
            serde_json::to_string(original_dns).context("serialize original dns")?,
        ],
    )
    .with_context(|| format!("mark system dns applied: {adapter_id}"))?;
    Ok(())
}

fn mark_system_dns_restored(conn: &mut Connection, adapter_id: &str) -> Result<()> {
    conn.execute(
        r#"
        INSERT INTO system_dns_adapter_state (
            adapter_id, managed, original_dns, last_restored_at, last_error, updated_at
        )
        VALUES (?, 0, '[]', CURRENT_TIMESTAMP, NULL, CURRENT_TIMESTAMP)
        ON CONFLICT(adapter_id) DO UPDATE SET
            managed = 0,
            original_dns = '[]',
            last_restored_at = CURRENT_TIMESTAMP,
            last_error = NULL,
            updated_at = CURRENT_TIMESTAMP
        "#,
        params![adapter_id],
    )
    .with_context(|| format!("mark system dns restored: {adapter_id}"))?;
    Ok(())
}

fn mark_system_dns_error(conn: &mut Connection, adapter_id: &str, err: &str) -> Result<()> {
    conn.execute(
        r#"
        INSERT INTO system_dns_adapter_state (
            adapter_id, managed, original_dns, last_error, updated_at
        )
        VALUES (?, 0, '[]', ?, CURRENT_TIMESTAMP)
        ON CONFLICT(adapter_id) DO UPDATE SET
            last_error = excluded.last_error,
            updated_at = CURRENT_TIMESTAMP
        "#,
        params![adapter_id, err],
    )
    .with_context(|| format!("mark system dns error: {adapter_id}"))?;
    Ok(())
}

fn json_vec_from_sql(raw: &str) -> rusqlite::Result<Vec<String>> {
    serde_json::from_str(raw).map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(err))
    })
}

fn collect_rows<T, F>(rows: rusqlite::MappedRows<'_, F>) -> Result<Vec<T>>
where
    F: FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<T>,
{
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

fn bool_to_i64(value: bool) -> i64 {
    if value {
        1
    } else {
        0
    }
}

fn int_to_bool(value: i64) -> bool {
    value != 0
}

fn usize_to_i64(value: usize) -> Result<i64> {
    i64::try_from(value).context("integer is too large")
}

fn u32_to_i64(value: u32) -> i64 {
    i64::from(value)
}

fn i64_to_usize(value: i64) -> rusqlite::Result<usize> {
    usize::try_from(value).map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Integer, Box::new(err))
    })
}

fn i64_to_u32(value: i64) -> rusqlite::Result<u32> {
    u32::try_from(value).map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Integer, Box::new(err))
    })
}
