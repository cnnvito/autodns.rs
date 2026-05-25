use crate::config::desktop_config_dir;
use chrono::{Local, NaiveDate};
use parking_lot::Mutex;
use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::sync::mpsc::{self, RecvTimeoutError, SyncSender, TrySendError};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

const LOG_RETENTION_DAYS: i64 = 7;
const LOG_CHANNEL_CAPACITY: usize = 4096;
const LOG_FLUSH_INTERVAL: Duration = Duration::from_millis(250);
const LOG_FLUSH_BATCH_SIZE: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

#[derive(Clone)]
pub struct LogBuffer {
    sender: SyncSender<LogCommand>,
    min_level: Arc<Mutex<LogLevel>>,
}

struct LogCommand {
    line: Vec<u8>,
    flush: bool,
}

struct FileLogger {
    dir: PathBuf,
    current_date: Option<NaiveDate>,
    writer: Option<BufWriter<File>>,
}

impl LogBuffer {
    pub fn new(_capacity: usize) -> Self {
        let dir = desktop_config_dir()
            .map(|path| path.join("logs"))
            .unwrap_or_else(|_| PathBuf::from("logs"));
        let (sender, receiver) = mpsc::sync_channel(LOG_CHANNEL_CAPACITY);
        thread::spawn(move || run_log_writer(receiver, FileLogger::new(dir)));

        Self {
            sender,
            min_level: Arc::new(Mutex::new(LogLevel::Info)),
        }
    }

    pub fn set_level(&self, level: &str) {
        *self.min_level.lock() = LogLevel::from_str(level);
    }

    pub fn enabled(&self, level: &str) -> bool {
        LogLevel::from_str(level) >= *self.min_level.lock()
    }

    pub fn debug_enabled(&self) -> bool {
        self.enabled("debug")
    }

    pub fn push(&self, level: impl Into<String>, message: impl Into<String>) {
        let level = level.into();
        if !self.enabled(&level) {
            return;
        }
        let line = format!(
            "{} [{}] {}\n",
            Local::now().to_rfc3339(),
            level.to_ascii_uppercase(),
            message.into()
        );
        let flush = matches!(LogLevel::from_str(&level), LogLevel::Warn | LogLevel::Error);
        let command = LogCommand {
            line: line.into_bytes(),
            flush,
        };
        if flush {
            let _ = self.sender.send(command);
        } else {
            match self.sender.try_send(command) {
                Ok(()) | Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => {}
            }
        }
    }
}

fn run_log_writer(receiver: mpsc::Receiver<LogCommand>, mut logger: FileLogger) {
    loop {
        match receiver.recv_timeout(LOG_FLUSH_INTERVAL) {
            Ok(command) => {
                let mut should_flush = logger.write(&command.line).is_ok() && command.flush;
                let mut count = 1;
                while count < LOG_FLUSH_BATCH_SIZE {
                    let Ok(command) = receiver.try_recv() else {
                        break;
                    };
                    should_flush |= logger.write(&command.line).is_ok() && command.flush;
                    count += 1;
                }
                if should_flush || count >= LOG_FLUSH_BATCH_SIZE {
                    logger.flush();
                }
            }
            Err(RecvTimeoutError::Timeout) => logger.flush(),
            Err(RecvTimeoutError::Disconnected) => {
                logger.flush();
                break;
            }
        }
    }
}

impl FileLogger {
    fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            current_date: None,
            writer: None,
        }
    }

    fn write(&mut self, line: &[u8]) -> std::io::Result<()> {
        self.ensure_writer()?;
        if let Some(writer) = &mut self.writer {
            writer.write_all(line)?;
        }
        Ok(())
    }

    fn flush(&mut self) {
        if let Some(writer) = &mut self.writer {
            let _ = writer.flush();
        }
    }

    fn ensure_writer(&mut self) -> std::io::Result<()> {
        let today = Local::now().date_naive();
        if self.current_date == Some(today) && self.writer.is_some() {
            return Ok(());
        }

        fs::create_dir_all(&self.dir)?;
        self.cleanup_old_logs(today);
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.log_path(today))?;
        self.writer = Some(BufWriter::new(file));
        self.current_date = Some(today);
        Ok(())
    }

    fn log_path(&self, date: NaiveDate) -> PathBuf {
        self.dir
            .join(format!("autodns-{}.log", date.format("%Y-%m-%d")))
    }

    fn cleanup_old_logs(&self, today: NaiveDate) {
        let Ok(entries) = fs::read_dir(&self.dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(date) = log_date_from_path(&path) else {
                continue;
            };
            if (today - date).num_days() >= LOG_RETENTION_DAYS {
                let _ = fs::remove_file(path);
            }
        }
    }
}

fn log_date_from_path(path: &std::path::Path) -> Option<NaiveDate> {
    let file_name = path.file_name()?.to_str()?;
    let date = file_name.strip_prefix("autodns-")?.strip_suffix(".log")?;
    NaiveDate::parse_from_str(date, "%Y-%m-%d").ok()
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
    fn parses_log_date_from_file_name() {
        let path = PathBuf::from("autodns-2026-05-25.log");

        let date = log_date_from_path(&path).expect("parse log date");

        assert_eq!(date, NaiveDate::from_ymd_opt(2026, 5, 25).unwrap());
    }

    #[test]
    fn ignores_non_log_file_names() {
        assert!(log_date_from_path(&PathBuf::from("notes.txt")).is_none());
        assert!(log_date_from_path(&PathBuf::from("autodns-latest.log")).is_none());
    }

    #[test]
    fn enabled_respects_min_level() {
        let logs = LogBuffer::new(10);
        logs.set_level("warn");

        assert!(!logs.debug_enabled());
        assert!(!logs.enabled("info"));
        assert!(logs.enabled("warn"));
        assert!(logs.enabled("error"));
    }
}
