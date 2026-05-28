use crate::config::{
    core_config_from_desktop, database_path, default_local_config, desktop_config_from_core,
    validate_desktop_config, CoreConfig,
};
use crate::desktop::{
    ConfigDocument, DesktopConfig, DesktopHostStatus, DesktopRouteStatus, DnsHistoryEntry,
    DnsHistoryList, DnsHistoryTopDomain, SystemDnsSettings,
};
use crate::history::DnsHistoryEvent;
use anyhow::{anyhow, Context, Result};
use chrono::{Duration as ChronoDuration, Utc};
use parking_lot::Mutex;
use rusqlite::{params, Connection, OptionalExtension};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

const CONFIG_DOCUMENT_PATH: &str = "local://autodns/config";

#[derive(Clone)]
pub struct ConfigStore {
    path: PathBuf,
    conn: Arc<Mutex<Connection>>,
}

#[derive(Clone)]
struct ParsedRoute {
    match_type: String,
    domain: String,
    upstreams: Vec<String>,
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
        let conn = Connection::open(&path)
            .with_context(|| format!("open database: {}", path.display()))?;
        configure_connection(&conn)?;
        let store = Self {
            path,
            conn: Arc::new(Mutex::new(conn)),
        };
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

    pub fn service_enabled(&self) -> Result<bool> {
        self.with_conn(load_service_enabled)
    }

    pub fn save_service_enabled(&self, enabled: bool) -> Result<()> {
        self.with_conn(|conn| save_service_enabled(conn, enabled))
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

    pub(crate) fn insert_dns_history(&self, events: &[DnsHistoryEvent]) -> Result<()> {
        self.with_conn(|conn| insert_dns_history(conn, events))
    }

    pub fn list_dns_history(
        &self,
        domain: &str,
        limit: usize,
        offset: usize,
    ) -> Result<DnsHistoryList> {
        self.with_conn(|conn| list_dns_history(conn, domain, limit, offset))
    }

    pub fn dns_history_top_domains(&self, limit: usize) -> Result<Vec<DnsHistoryTopDomain>> {
        self.with_conn(|conn| dns_history_top_domains(conn, limit))
    }

    pub fn clear_dns_history(&self) -> Result<usize> {
        self.with_conn(clear_dns_history)
    }

    pub(crate) fn cleanup_dns_history(
        &self,
        retention_days: i64,
        max_entries: usize,
    ) -> Result<usize> {
        self.with_conn(|conn| cleanup_dns_history(conn, retention_days, max_entries))
    }

    fn with_conn<T>(&self, f: impl FnOnce(&mut Connection) -> Result<T>) -> Result<T> {
        let mut conn = self.conn.lock();
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
            service_enabled INTEGER NOT NULL DEFAULT 0,
            bootstrap_dns TEXT NOT NULL DEFAULT '["1.1.1.1:53","8.8.8.8:53"]',
            default_proxy TEXT NOT NULL DEFAULT '',
            resolver_timeout TEXT NOT NULL DEFAULT '',
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

        CREATE TABLE IF NOT EXISTS dns_query_history (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            started_at TEXT NOT NULL,
            domain TEXT NOT NULL,
            record_type TEXT NOT NULL,
            qclass INTEGER NOT NULL DEFAULT 1,
            source TEXT NOT NULL,
            route_id INTEGER NOT NULL,
            upstream_name TEXT NOT NULL DEFAULT '',
            upstream_protocol TEXT NOT NULL DEFAULT '',
            duration_ms INTEGER NOT NULL,
            attempt_count INTEGER NOT NULL,
            response_code TEXT NOT NULL,
            min_ttl INTEGER,
            error TEXT NOT NULL DEFAULT '',
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        );

        CREATE INDEX IF NOT EXISTS idx_dns_query_history_started_at
            ON dns_query_history(started_at DESC, id DESC);
        CREATE INDEX IF NOT EXISTS idx_dns_query_history_domain_started_at
            ON dns_query_history(domain, started_at DESC);
        CREATE INDEX IF NOT EXISTS idx_dns_query_history_upstream
            ON dns_query_history(upstream_name);
        "#,
    )
    .context("migrate database")?;
    add_column_if_missing(
        conn,
        "app_settings",
        "ipv6_enabled",
        "INTEGER NOT NULL DEFAULT 1",
    )?;
    add_column_if_missing(
        conn,
        "app_settings",
        "bootstrap_dns",
        r#"TEXT NOT NULL DEFAULT '["1.1.1.1:53","8.8.8.8:53"]'"#,
    )?;
    add_column_if_missing(
        conn,
        "app_settings",
        "service_enabled",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    add_column_if_missing(conn, "proxies", "protocol", "TEXT NOT NULL DEFAULT ''")?;
    add_column_if_missing(conn, "proxies", "host", "TEXT NOT NULL DEFAULT ''")?;
    add_column_if_missing(conn, "proxies", "port", "INTEGER")?;
    add_column_if_missing(conn, "upstreams", "protocol", "TEXT NOT NULL DEFAULT ''")?;
    add_column_if_missing(conn, "upstreams", "host", "TEXT NOT NULL DEFAULT ''")?;
    add_column_if_missing(conn, "upstreams", "port", "INTEGER")?;
    add_column_if_missing(conn, "upstreams", "path", "TEXT NOT NULL DEFAULT ''")?;
    drop_column_if_exists(conn, "dns_query_history", "results_json")?;
    drop_column_if_exists(conn, "dns_query_history", "answer_count")?;
    add_column_if_missing(conn, "dns_query_history", "min_ttl", "INTEGER")?;
    drop_column_if_exists(conn, "app_settings", "fallback_system_dns")?;
    drop_column_if_exists(conn, "proxies", "endpoint")?;
    drop_column_if_exists(conn, "upstreams", "endpoint")?;
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

fn configure_connection(conn: &Connection) -> Result<()> {
    conn.busy_timeout(Duration::from_secs(5))
        .context("set sqlite busy timeout")?;
    conn.execute_batch(
        r#"
        PRAGMA foreign_keys = ON;
        PRAGMA journal_mode = WAL;
        PRAGMA synchronous = NORMAL;
        "#,
    )
    .context("configure sqlite connection")?;
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

fn drop_column_if_exists(conn: &Connection, table: &str, column: &str) -> Result<()> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
    for name in rows {
        if name? == column {
            conn.execute(&format!("ALTER TABLE {table} DROP COLUMN {column}"), [])
                .with_context(|| format!("drop column {table}.{column}"))?;
            return Ok(());
        }
    }
    Ok(())
}

fn replace_config(conn: &mut Connection, config: DesktopConfig) -> Result<()> {
    let service_enabled = load_service_enabled(conn).unwrap_or(false);
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
            service_enabled, bootstrap_dns, default_proxy, resolver_timeout, ipv6_enabled,
            cache_enabled, cache_max_entries, cache_max_entry_size, cache_min_ttl, cache_max_ttl,
            cache_negative_ttl, cache_eviction_policy,
            healthcheck_enabled, healthcheck_interval, healthcheck_timeout, healthcheck_domain,
            healthcheck_failure_threshold, healthcheck_recovery_threshold, log_level
        )
        VALUES (1, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
        params![
            config.server.mode,
            config.server.listen,
            config.server.cert_file,
            config.server.key_file,
            config.server.path,
            bool_to_i64(service_enabled),
            serde_json::to_string(&config.resolver.bootstrap_dns)
                .context("serialize bootstrap dns")?,
            config.resolver.default_proxy,
            config.resolver.timeout,
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
        tx.execute(
            r#"
            INSERT INTO proxies (name, protocol, host, port, username, password, sort_order, enabled)
            VALUES (?, ?, ?, ?, ?, ?, ?, 1)
            "#,
            params![
                proxy.name,
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
        tx.execute(
            r#"
            INSERT INTO upstreams (name, protocol, host, port, path, server_name, proxy_name, sort_order, enabled)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, 1)
            "#,
            params![
                upstream.name,
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

fn load_service_enabled(conn: &mut Connection) -> Result<bool> {
    conn.query_row(
        "SELECT service_enabled FROM app_settings WHERE id = 1",
        [],
        |row| row.get::<_, i64>(0),
    )
    .map(int_to_bool)
    .context("load service enabled state")
}

fn save_service_enabled(conn: &mut Connection, enabled: bool) -> Result<()> {
    conn.execute(
        "UPDATE app_settings SET service_enabled = ?, updated_at = CURRENT_TIMESTAMP WHERE id = 1",
        params![bool_to_i64(enabled)],
    )
    .context("save service enabled state")?;
    Ok(())
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
            bootstrap_dns, default_proxy, resolver_timeout, ipv6_enabled,
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
                        bootstrap_dns: {
                            let raw: String = row.get(5)?;
                            let parsed = json_vec_from_sql(&raw)?;
                            if parsed.is_empty() {
                                Vec::new()
                            } else {
                                parsed
                            }
                        },
                        default_proxy: row.get(6)?,
                        hosts: Vec::new(),
                        host_statuses: Vec::new(),
                        routes: Vec::new(),
                        route_statuses: Vec::new(),
                        timeout: row.get(7)?,
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
        SELECT name, protocol, host, port, username, password
        FROM proxies
        WHERE deleted_at IS NULL
        ORDER BY sort_order, id
        "#,
    )?;
    let rows = stmt.query_map([], |row| {
        let port: Option<i64> = row.get(3)?;
        Ok(crate::desktop::DesktopProxyConfig {
            name: row.get(0)?,
            protocol: row.get(1)?,
            host: row.get(2)?,
            port: port.map(|port| port.to_string()).unwrap_or_default(),
            username: row.get(4)?,
            password: row.get(5)?,
        })
    })?;
    collect_rows(rows)
}

fn load_desktop_upstreams(conn: &Connection) -> Result<Vec<crate::desktop::DesktopUpstreamConfig>> {
    let mut stmt = conn.prepare(
        r#"
        SELECT name, protocol, host, port, path, server_name, proxy_name
        FROM upstreams
        WHERE deleted_at IS NULL
        ORDER BY sort_order, id
        "#,
    )?;
    let rows = stmt.query_map([], |row| {
        let port: Option<i64> = row.get(3)?;
        Ok(crate::desktop::DesktopUpstreamConfig {
            name: row.get(0)?,
            protocol: row.get(1)?,
            host: row.get(2)?,
            port: port.map(|port| port.to_string()).unwrap_or_default(),
            path: row.get(4)?,
            server_name: row.get(5)?,
            proxy: row.get(6)?,
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

fn insert_dns_history(conn: &mut Connection, events: &[DnsHistoryEvent]) -> Result<()> {
    if events.is_empty() {
        return Ok(());
    }
    let tx = conn
        .transaction()
        .context("begin dns history transaction")?;
    {
        let mut stmt = tx.prepare(
            r#"
            INSERT INTO dns_query_history (
                started_at, domain, record_type, qclass, source, route_id,
                upstream_name, upstream_protocol, duration_ms, attempt_count,
                response_code, min_ttl, error
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )?;
        for event in events {
            stmt.execute(params![
                &event.started_at,
                &event.domain,
                &event.record_type,
                i64::from(event.qclass),
                &event.source,
                i64::from(event.route_id),
                &event.upstream_name,
                &event.upstream_protocol,
                u128_to_i64(event.duration_ms)?,
                usize_to_i64(event.attempt_count)?,
                &event.response_code,
                event.min_ttl.map(u32_to_i64),
                &event.error,
            ])
            .context("insert dns history")?;
        }
    }
    tx.commit().context("commit dns history transaction")?;
    Ok(())
}

fn list_dns_history(
    conn: &mut Connection,
    domain: &str,
    limit: usize,
    offset: usize,
) -> Result<DnsHistoryList> {
    let limit = limit.clamp(1, 500);
    let domain = domain.trim();
    let total = conn.query_row(
        r#"
        SELECT COUNT(*)
        FROM dns_query_history
        WHERE (?1 = '' OR lower(domain) LIKE '%' || lower(?1) || '%')
        "#,
        params![domain],
        |row| row.get::<_, i64>(0),
    )?;

    let mut stmt = conn.prepare(
        r#"
        SELECT
            id, started_at, domain, record_type, source, route_id,
            upstream_name, upstream_protocol, duration_ms, attempt_count,
            response_code, min_ttl, error
        FROM dns_query_history
        WHERE (?1 = '' OR lower(domain) LIKE '%' || lower(?1) || '%')
        ORDER BY started_at DESC, id DESC
        LIMIT ?2 OFFSET ?3
        "#,
    )?;
    let rows = stmt.query_map(
        params![domain, usize_to_i64(limit)?, usize_to_i64(offset)?],
        |row| {
            Ok(DnsHistoryEntry {
                id: row.get(0)?,
                started_at: row.get(1)?,
                domain: row.get(2)?,
                record_type: row.get(3)?,
                source: row.get(4)?,
                route_id: i64_to_i32(row.get(5)?)?,
                upstream_name: row.get(6)?,
                upstream_protocol: row.get(7)?,
                duration_ms: i64_to_u128(row.get(8)?)?,
                attempt_count: i64_to_usize(row.get(9)?)?,
                response_code: row.get(10)?,
                min_ttl: row.get::<_, Option<i64>>(11)?.map(i64_to_u32).transpose()?,
                error: row.get(12)?,
            })
        },
    )?;
    Ok(DnsHistoryList {
        items: collect_rows(rows)?,
        total: i64_to_usize(total)?,
    })
}

fn dns_history_top_domains(
    conn: &mut Connection,
    limit: usize,
) -> Result<Vec<DnsHistoryTopDomain>> {
    let mut stmt = conn.prepare(
        r#"
        SELECT
            domain,
            COUNT(*),
            MAX(started_at),
            COALESCE(AVG(duration_ms), 0)
        FROM dns_query_history
        GROUP BY domain
        ORDER BY COUNT(*) DESC, MAX(started_at) DESC
        LIMIT ?
        "#,
    )?;
    let rows = stmt.query_map(params![usize_to_i64(limit.clamp(1, 100))?], |row| {
        Ok(DnsHistoryTopDomain {
            domain: row.get(0)?,
            count: i64_to_usize(row.get(1)?)?,
            last_seen_at: row.get(2)?,
            average_duration_ms: row.get(3)?,
        })
    })?;
    collect_rows(rows)
}

fn clear_dns_history(conn: &mut Connection) -> Result<usize> {
    conn.execute("DELETE FROM dns_query_history", [])
        .context("clear dns history")
}

fn cleanup_dns_history(
    conn: &mut Connection,
    retention_days: i64,
    max_entries: usize,
) -> Result<usize> {
    let cutoff = (Utc::now() - ChronoDuration::days(retention_days.max(1))).to_rfc3339();
    let mut deleted = conn
        .execute(
            "DELETE FROM dns_query_history WHERE started_at < ?",
            params![cutoff],
        )
        .context("cleanup old dns history")?;
    deleted += conn
        .execute(
            r#"
            DELETE FROM dns_query_history
            WHERE id IN (
                SELECT id
                FROM dns_query_history
                ORDER BY started_at DESC, id DESC
                LIMIT -1 OFFSET ?
            )
            "#,
            params![usize_to_i64(max_entries.max(1))?],
        )
        .context("trim dns history")?;
    Ok(deleted)
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

fn i64_to_i32(value: i64) -> rusqlite::Result<i32> {
    i32::try_from(value).map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Integer, Box::new(err))
    })
}

fn i64_to_u128(value: i64) -> rusqlite::Result<u128> {
    u128::try_from(value).map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Integer, Box::new(err))
    })
}

fn u128_to_i64(value: u128) -> Result<i64> {
    i64::try_from(value).context("integer is too large")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_enabled_survives_config_save() {
        let path = temp_store_path("state");
        let store = ConfigStore::open(path.clone()).expect("open config store");

        assert!(!store.service_enabled().expect("load initial state"));
        store
            .save_service_enabled(true)
            .expect("save service enabled");
        store
            .save_config(desktop_config_from_core(&default_local_config()))
            .expect("save config");
        assert!(store.service_enabled().expect("load saved state"));

        drop(store);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn dns_history_persists_min_ttl() {
        let path = temp_store_path("history");
        let store = ConfigStore::open(path.clone()).expect("open config store");

        store
            .insert_dns_history(&[DnsHistoryEvent {
                started_at: Utc::now().to_rfc3339(),
                domain: "ttl.example".into(),
                record_type: "A".into(),
                qclass: 1,
                source: "upstream".into(),
                route_id: 0,
                upstream_name: "test".into(),
                upstream_protocol: "udp".into(),
                duration_ms: 12,
                attempt_count: 1,
                response_code: "NOERROR".into(),
                min_ttl: Some(42),
                error: String::new(),
            }])
            .expect("insert history");

        let history = store
            .list_dns_history("ttl.example", 10, 0)
            .expect("load history");

        assert_eq!(history.items.len(), 1);
        assert_eq!(history.items[0].min_ttl, Some(42));

        drop(store);
        let _ = std::fs::remove_file(path);
    }

    fn temp_store_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "autodns-store-{name}-{}-{}.sqlite3",
            std::process::id(),
            Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ))
    }
}
