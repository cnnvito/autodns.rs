use chrono::Utc;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::Arc;

type LogListener = Arc<dyn Fn(LogEntry) + Send + Sync>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogEntry {
    pub id: u64,
    pub time: String,
    pub level: String,
    pub message: String,
}

#[derive(Clone)]
pub struct LogBuffer {
    inner: Arc<Mutex<VecDeque<LogEntry>>>,
    min_level: Arc<Mutex<LogLevel>>,
    next_id: Arc<Mutex<u64>>,
    listener: Arc<Mutex<Option<LogListener>>>,
    capacity: usize,
}

impl LogBuffer {
    pub fn new(capacity: usize) -> Self {
        Self {
            inner: Arc::new(Mutex::new(VecDeque::with_capacity(capacity))),
            min_level: Arc::new(Mutex::new(LogLevel::Info)),
            next_id: Arc::new(Mutex::new(0)),
            listener: Arc::new(Mutex::new(None)),
            capacity,
        }
    }

    pub fn set_listener(&self, listener: impl Fn(LogEntry) + Send + Sync + 'static) {
        *self.listener.lock() = Some(Arc::new(listener));
    }

    pub fn set_level(&self, level: &str) {
        *self.min_level.lock() = LogLevel::from_str(level);
    }

    pub fn push(&self, level: impl Into<String>, message: impl Into<String>) {
        let level = level.into();
        if LogLevel::from_str(&level) < *self.min_level.lock() {
            return;
        }
        let entry = {
            let mut next_id = self.next_id.lock();
            *next_id += 1;
            LogEntry {
                id: *next_id,
                time: Utc::now().to_rfc3339(),
                level,
                message: message.into(),
            }
        };
        {
            let mut entries = self.inner.lock();
            if entries.len() >= self.capacity {
                entries.pop_front();
            }
            entries.push_back(entry.clone());
        }
        if let Some(listener) = self.listener.lock().as_ref().cloned() {
            listener(entry);
        }
    }

    pub fn entries(&self) -> Vec<LogEntry> {
        self.inner.lock().iter().cloned().collect()
    }

    pub fn entries_since(&self, since: u64) -> Vec<LogEntry> {
        self.inner
            .lock()
            .iter()
            .filter(|entry| entry.id > since)
            .cloned()
            .collect()
    }
}

impl LogLevel {
    fn from_str(level: &str) -> Self {
        match level.to_lowercase().as_str() {
            "debug" => Self::Debug,
            "warn" => Self::Warn,
            "error" => Self::Error,
            _ => Self::Info,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_buffer_filters_below_min_level() {
        let logs = LogBuffer::new(10);
        logs.set_level("warn");

        logs.push("debug", "debug message");
        logs.push("info", "info message");
        logs.push("warn", "warn message");
        logs.push("error", "error message");

        let entries = logs.entries();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].level, "warn");
        assert_eq!(entries[1].level, "error");
    }
}
