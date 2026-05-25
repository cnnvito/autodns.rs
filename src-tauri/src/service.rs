use crate::desktop::{
    ApplyConfigAction, ApplyConfigResult, ConfigDocument, DesktopConfig, DesktopStatus,
    DnsLookupResult, SystemDnsAdapter, SystemDnsSettings, SystemDnsStatus,
};
use crate::dns::{
    build_proxy_health, build_upstream_health, mark_proxies_unknown, mark_upstreams_unknown,
    start_runtime, HealthListener, RunningRuntime,
};
use crate::logging::LogBuffer;
use crate::store::ConfigStore;
use crate::system_dns;
use anyhow::{anyhow, Result};
use chrono::Utc;
use parking_lot::Mutex;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

pub struct DesktopService {
    inner: Mutex<ServiceState>,
    store: Mutex<Option<ConfigStore>>,
    system_dns_adapters: Mutex<SystemDnsAdapterCache>,
    status_listener: Mutex<Option<HealthListener>>,
    logs: LogBuffer,
    allow_quit: AtomicBool,
}

#[derive(Default)]
struct ServiceState {
    status: DesktopStatus,
    runtime: Option<RunningRuntime>,
}

#[derive(Default)]
struct SystemDnsAdapterCache {
    adapters: Vec<SystemDnsAdapter>,
    last_error: Option<String>,
}

impl DesktopService {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(ServiceState::default()),
            store: Mutex::new(None),
            system_dns_adapters: Mutex::new(SystemDnsAdapterCache::default()),
            status_listener: Mutex::new(None),
            logs: LogBuffer::new(1000),
            allow_quit: AtomicBool::new(false),
        }
    }

    pub fn initialize(&self) -> Result<()> {
        let store = ConfigStore::open_default()?;
        let store_path = store.path().to_string_lossy().to_string();
        let mut inner = self.inner.lock();
        inner.status.config_path = String::new();
        *self.store.lock() = Some(store);
        self.logs
            .push("info", format!("desktop database ready: {store_path}"));
        Ok(())
    }

    pub async fn start(&self, _config_path: String) -> Result<()> {
        {
            let inner = self.inner.lock();
            if inner.status.running {
                return Err(anyhow!("autodns is already running"));
            }
        }

        let core = self.store()?.runtime_config()?;
        self.logs.set_level(&core.log.level);
        let runtime = start_runtime(core.clone(), self.logs.clone()).await?;
        if let Some(listener) = self.status_listener.lock().clone() {
            runtime.set_health_listener(listener);
        }
        let status = DesktopStatus {
            running: true,
            config_path: String::new(),
            mode: core.server.mode.clone(),
            listen: core.server.listen.clone(),
            upstreams: core.resolver.upstreams.len(),
            routes: core.resolver.routes.len(),
            default_upstreams: core.resolver.upstreams.len(),
            started_at: Some(Utc::now().to_rfc3339()),
            last_error: None,
            upstream_health: build_upstream_health(&core, Some(&runtime.view().health)),
            proxy_health: build_proxy_health(&core, Some(&runtime.view().health)),
        };

        let mut inner = self.inner.lock();
        inner.status = status;
        inner.runtime = Some(runtime);
        self.logs.push(
            "info",
            format!("desktop runtime started on {}", core.server.listen),
        );
        Ok(())
    }

    pub async fn stop(&self) -> Result<()> {
        let runtime = {
            let mut inner = self.inner.lock();
            if !inner.status.running {
                return Ok(());
            }
            let runtime = inner.runtime.take();
            let upstream_health = inner.status.upstream_health.clone();
            let proxy_health = inner.status.proxy_health.clone();
            inner.status.running = false;
            inner.status.upstream_health = mark_upstreams_unknown(&upstream_health);
            inner.status.proxy_health = mark_proxies_unknown(&proxy_health);
            runtime
        };
        if let Some(runtime) = runtime {
            runtime.stop().await;
        }
        self.logs.push("info", "desktop runtime stopped");
        Ok(())
    }

    pub fn status(&self) -> DesktopStatus {
        let mut inner = self.inner.lock();
        let view = inner.runtime.as_ref().map(|runtime| runtime.view());
        if let Some(view) = view {
            inner.status.upstream_health = build_upstream_health(&view.config, Some(&view.health));
            inner.status.proxy_health = build_proxy_health(&view.config, Some(&view.health));
        }
        inner.status.clone()
    }

    pub fn set_status_listener(&self, listener: impl Fn() + Send + Sync + 'static) {
        let listener: HealthListener = Arc::new(listener);
        *self.status_listener.lock() = Some(listener.clone());
        if let Some(runtime) = self.inner.lock().runtime.as_ref() {
            runtime.set_health_listener(listener);
        }
    }

    pub fn clear_dns_cache(&self) -> usize {
        let inner = self.inner.lock();
        let Some(runtime) = inner.runtime.as_ref() else {
            return 0;
        };
        runtime.clear_cache()
    }

    pub async fn lookup_domain(
        &self,
        domain: String,
        record_type: String,
    ) -> Result<DnsLookupResult> {
        let resolver = {
            let inner = self.inner.lock();
            inner
                .runtime
                .as_ref()
                .ok_or_else(|| anyhow!("DNS service is not running"))?
                .resolver()
        };
        resolver.lookup(&domain, &record_type).await
    }

    pub fn managed_config(&self) -> Result<ConfigDocument> {
        self.store()?.load_document()
    }

    pub fn validate_config(&self, cfg: DesktopConfig) -> Result<()> {
        self.store()?.validate_config(cfg)
    }

    pub async fn apply_config(&self, doc: ConfigDocument) -> Result<ApplyConfigResult> {
        self.store()?.save_document(doc)?;
        if let Ok(core) = self.store()?.runtime_config() {
            self.logs.set_level(&core.log.level);
        }
        if self.status().running {
            let core = self.store()?.runtime_config()?;
            if self.try_reload(core.clone()).await? {
                return Ok(ApplyConfigResult {
                    action: ApplyConfigAction::HotReloaded,
                    status: self.status(),
                });
            }
            self.stop().await?;
            self.start(String::new()).await?;
            return Ok(ApplyConfigResult {
                action: ApplyConfigAction::Restarted,
                status: self.status(),
            });
        }
        Ok(ApplyConfigResult {
            action: ApplyConfigAction::Saved,
            status: self.status(),
        })
    }

    pub fn system_dns_status(&self, force: bool) -> Result<SystemDnsStatus> {
        let store = self.store()?;
        let listen = store
            .runtime_config()
            .map(|cfg| cfg.server.listen)
            .unwrap_or_default();
        let (adapters, last_error) = self.system_dns_adapters(force);
        system_dns::status_from_adapters(&store, &listen, adapters, last_error)
    }

    pub fn save_system_dns_settings(&self, settings: SystemDnsSettings) -> Result<SystemDnsStatus> {
        let store = self.store()?;
        let listen = store
            .runtime_config()
            .map(|cfg| cfg.server.listen)
            .unwrap_or_default();
        system_dns::save_settings(&store, settings)?;
        let (adapters, last_error) = self.cached_system_dns_adapters();
        system_dns::status_from_adapters(&store, &listen, adapters, last_error)
    }

    pub fn apply_system_dns(&self) -> Result<SystemDnsStatus> {
        let store = self.store()?;
        let listen = store
            .runtime_config()
            .map(|cfg| cfg.server.listen)
            .unwrap_or_default();
        let cache = self.system_dns_adapters.lock();
        if cache.adapters.is_empty() {
            return Err(anyhow!(
                "network adapters have not been loaded yet; refresh system DNS adapters first"
            ));
        }
        let mut adapters = cache.adapters.clone();
        drop(cache);

        system_dns::apply(&store, &mut adapters)?;

        let mut cache = self.system_dns_adapters.lock();
        cache.adapters = adapters.clone();
        cache.last_error = None;
        system_dns::status_from_adapters(&store, &listen, adapters, None)
    }

    pub fn restore_system_dns(&self) -> Result<SystemDnsStatus> {
        let store = self.store()?;
        let listen = store
            .runtime_config()
            .map(|cfg| cfg.server.listen)
            .unwrap_or_default();
        let cache = self.system_dns_adapters.lock();
        if cache.adapters.is_empty() {
            return Err(anyhow!(
                "network adapters have not been loaded yet; refresh system DNS adapters first"
            ));
        }
        let mut adapters = cache.adapters.clone();
        drop(cache);

        system_dns::restore(&store, &mut adapters)?;

        let mut cache = self.system_dns_adapters.lock();
        cache.adapters = adapters.clone();
        cache.last_error = None;
        system_dns::status_from_adapters(&store, &listen, adapters, None)
    }

    pub fn record_error(&self, stage: &str, err: &str) {
        let mut inner = self.inner.lock();
        inner.status.last_error = Some(err.to_string());
        self.logs.push("error", format!("{stage}: {err}"));
    }

    pub fn allow_quit(&self) -> bool {
        self.allow_quit.load(Ordering::SeqCst)
    }

    pub fn set_allow_quit(&self) {
        self.allow_quit.store(true, Ordering::SeqCst);
    }

    fn store(&self) -> Result<ConfigStore> {
        self.store
            .lock()
            .clone()
            .ok_or_else(|| anyhow!("configuration database is not initialized"))
    }

    fn cached_system_dns_adapters(&self) -> (Vec<SystemDnsAdapter>, Option<String>) {
        let cache = self.system_dns_adapters.lock();
        (cache.adapters.clone(), cache.last_error.clone())
    }

    fn system_dns_adapters(&self, force: bool) -> (Vec<SystemDnsAdapter>, Option<String>) {
        if force {
            match system_dns::read_adapters() {
                Ok(adapters) => {
                    let mut cache = self.system_dns_adapters.lock();
                    cache.adapters = adapters;
                    cache.last_error = None;
                }
                Err(err) => {
                    let mut cache = self.system_dns_adapters.lock();
                    cache.last_error = Some(err.to_string());
                }
            }
        }

        self.cached_system_dns_adapters()
    }

    async fn try_reload(&self, core: crate::config::CoreConfig) -> Result<bool> {
        let mut runtime = {
            let mut inner = self.inner.lock();
            let Some(runtime) = inner.runtime.take() else {
                return Ok(false);
            };
            runtime
        };

        if !runtime.can_reload(&core) {
            let mut inner = self.inner.lock();
            inner.runtime = Some(runtime);
            return Ok(false);
        }

        runtime.reload(core.clone(), self.logs.clone()).await?;
        if let Some(listener) = self.status_listener.lock().clone() {
            runtime.set_health_listener(listener);
        }
        self.logs.set_level(&core.log.level);
        let status = DesktopStatus {
            running: true,
            config_path: String::new(),
            mode: core.server.mode.clone(),
            listen: core.server.listen.clone(),
            upstreams: core.resolver.upstreams.len(),
            routes: core.resolver.routes.len(),
            default_upstreams: core.resolver.upstreams.len(),
            started_at: self.status().started_at,
            last_error: None,
            upstream_health: build_upstream_health(&core, Some(&runtime.view().health)),
            proxy_health: build_proxy_health(&core, Some(&runtime.view().health)),
        };
        let mut inner = self.inner.lock();
        inner.status = status;
        inner.runtime = Some(runtime);
        Ok(true)
    }
}
