use crate::logging::LogBuffer;
use crate::store::ConfigStore;
use std::sync::atomic::{AtomicU64, Ordering};
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

#[derive(Clone)]
pub(crate) struct DnsHistoryRecorder {
    sender: Option<SyncSender<DnsHistoryEvent>>,
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
    pub error: String,
}

impl DnsHistoryRecorder {
    pub(crate) fn disabled() -> Self {
        Self {
            sender: None,
            dropped: Arc::new(AtomicU64::new(0)),
        }
    }

    pub(crate) fn start(store: ConfigStore, logs: LogBuffer) -> Self {
        let (sender, receiver) = mpsc::sync_channel(HISTORY_QUEUE_CAPACITY);
        let dropped = Arc::new(AtomicU64::new(0));
        thread::spawn(move || {
            let mut writer = DnsHistoryWriter {
                store,
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
            dropped,
        }
    }

    pub(crate) fn record(&self, event: DnsHistoryEvent) {
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
}

struct DnsHistoryWriter {
    store: ConfigStore,
    logs: LogBuffer,
    pending: Vec<DnsHistoryEvent>,
    last_cleanup: Instant,
}

impl DnsHistoryWriter {
    fn flush(&mut self) {
        if self.pending.is_empty() {
            return;
        }
        if let Err(err) = self.store.insert_dns_history(&self.pending) {
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
            .store
            .cleanup_dns_history(HISTORY_RETENTION_DAYS, HISTORY_MAX_ENTRIES)
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
