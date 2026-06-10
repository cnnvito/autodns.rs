use crate::desktop::{DnsHistoryList, DnsHistoryOverview, DnsHistoryTopDomain};
use crate::logging::LogBuffer;
use crate::store::ConfigStore;
use anyhow::Result;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, SyncSender, TrySendError};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

const HISTORY_QUEUE_CAPACITY: usize = 4096;
const HISTORY_FLUSH_INTERVAL: Duration = Duration::from_millis(500);
const HISTORY_FLUSH_BATCH_SIZE: usize = 100;
const HISTORY_CLEANUP_INTERVAL: Duration = Duration::from_secs(600);
const HISTORY_RETENTION_DAYS: i64 = 14;
const HISTORY_MAX_ENTRIES: usize = 100_000;

pub(crate) type DnsHistoryBackendRef = Arc<dyn DnsHistoryBackend>;

pub(crate) trait DnsHistoryBackend: Send + Sync {
    fn insert(&self, events: &[DnsHistoryEvent]) -> Result<()>;

    fn list(
        &self,
        domain: &str,
        status_filter: &str,
        window: &str,
        upstream_name: &str,
        limit: usize,
        offset: usize,
    ) -> Result<DnsHistoryList>;

    fn top_domains(
        &self,
        limit: usize,
        domain: &str,
        status_filter: &str,
        window: &str,
        upstream_name: &str,
    ) -> Result<Vec<DnsHistoryTopDomain>>;

    fn upstream_names(&self, limit: usize) -> Result<Vec<String>>;

    fn overview(&self) -> Result<DnsHistoryOverview>;

    fn clear(&self) -> Result<usize>;

    fn cleanup(&self, retention_days: i64, max_entries: usize) -> Result<usize>;
}

pub(crate) struct SqliteDnsHistoryBackend {
    store: ConfigStore,
}

impl SqliteDnsHistoryBackend {
    pub(crate) fn shared(store: ConfigStore) -> DnsHistoryBackendRef {
        Arc::new(Self { store })
    }
}

impl DnsHistoryBackend for SqliteDnsHistoryBackend {
    fn insert(&self, events: &[DnsHistoryEvent]) -> Result<()> {
        self.store.insert_dns_history(events)
    }

    fn list(
        &self,
        domain: &str,
        status_filter: &str,
        window: &str,
        upstream_name: &str,
        limit: usize,
        offset: usize,
    ) -> Result<DnsHistoryList> {
        self.store
            .list_dns_history(domain, status_filter, window, upstream_name, limit, offset)
    }

    fn top_domains(
        &self,
        limit: usize,
        domain: &str,
        status_filter: &str,
        window: &str,
        upstream_name: &str,
    ) -> Result<Vec<DnsHistoryTopDomain>> {
        self.store
            .dns_history_top_domains(limit, domain, status_filter, window, upstream_name)
    }

    fn upstream_names(&self, limit: usize) -> Result<Vec<String>> {
        self.store.dns_history_upstream_names(limit)
    }

    fn overview(&self) -> Result<DnsHistoryOverview> {
        self.store.dns_history_overview()
    }

    fn clear(&self) -> Result<usize> {
        self.store.clear_dns_history()
    }

    fn cleanup(&self, retention_days: i64, max_entries: usize) -> Result<usize> {
        self.store.cleanup_dns_history(retention_days, max_entries)
    }
}

#[derive(Clone)]
pub(crate) struct DnsHistoryRecorder {
    sender: Option<SyncSender<DnsHistoryEvent>>,
    enabled: Arc<AtomicBool>,
    dropped: Arc<AtomicU64>,
}

#[derive(Clone, Debug)]
pub(crate) struct DnsHistoryEvent {
    pub started_at: String,
    pub domain: String,
    pub record_type: String,
    pub qclass: u16,
    pub source: String,
    pub route_id: i32,
    pub upstream_name: String,
    pub upstream_protocol: String,
    pub duration_ms: u128,
    pub attempt_count: usize,
    pub response_code: String,
    pub min_ttl: Option<u32>,
    pub error: String,
}

impl DnsHistoryRecorder {
    pub(crate) fn disabled() -> Self {
        Self {
            sender: None,
            enabled: Arc::new(AtomicBool::new(false)),
            dropped: Arc::new(AtomicU64::new(0)),
        }
    }

    pub(crate) fn start(backend: DnsHistoryBackendRef, logs: LogBuffer, enabled: bool) -> Self {
        let (sender, receiver) = mpsc::sync_channel(HISTORY_QUEUE_CAPACITY);
        let dropped = Arc::new(AtomicU64::new(0));
        thread::spawn(move || {
            let mut writer = DnsHistoryWriter {
                backend,
                logs,
                pending: Vec::with_capacity(HISTORY_FLUSH_BATCH_SIZE),
                last_cleanup: Instant::now() - HISTORY_CLEANUP_INTERVAL,
            };
            writer.cleanup();
            loop {
                match receiver.recv_timeout(HISTORY_FLUSH_INTERVAL) {
                    Ok(event) => {
                        writer.pending.push(event);
                        while writer.pending.len() < HISTORY_FLUSH_BATCH_SIZE {
                            let Ok(event) = receiver.try_recv() else {
                                break;
                            };
                            writer.pending.push(event);
                        }
                        writer.flush();
                    }
                    Err(RecvTimeoutError::Timeout) => writer.flush(),
                    Err(RecvTimeoutError::Disconnected) => {
                        writer.flush();
                        break;
                    }
                }
            }
        });

        Self {
            sender: Some(sender),
            enabled: Arc::new(AtomicBool::new(enabled)),
            dropped,
        }
    }

    pub(crate) fn record(&self, event: DnsHistoryEvent) {
        if !self.enabled.load(Ordering::Relaxed) {
            return;
        }
        let Some(sender) = &self.sender else {
            return;
        };
        match sender.try_send(event) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => {
                self.dropped.fetch_add(1, Ordering::Relaxed);
            }
            Err(TrySendError::Disconnected(_)) => {}
        }
    }

    pub(crate) fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::Relaxed);
    }
}

struct DnsHistoryWriter {
    backend: DnsHistoryBackendRef,
    logs: LogBuffer,
    pending: Vec<DnsHistoryEvent>,
    last_cleanup: Instant,
}

impl DnsHistoryWriter {
    fn flush(&mut self) {
        if self.pending.is_empty() {
            return;
        }
        if let Err(err) = self.backend.insert(&self.pending) {
            self.logs
                .push("warn", format!("write dns history failed: {err}"));
        }
        self.pending.clear();
        if self.last_cleanup.elapsed() >= HISTORY_CLEANUP_INTERVAL {
            self.cleanup();
        }
    }

    fn cleanup(&mut self) {
        match self
            .backend
            .cleanup(HISTORY_RETENTION_DAYS, HISTORY_MAX_ENTRIES)
        {
            Ok(_) => {
                self.last_cleanup = Instant::now();
            }
            Err(err) => {
                self.logs
                    .push("warn", format!("cleanup dns history failed: {err}"));
                self.last_cleanup = Instant::now();
            }
        }
    }
}
