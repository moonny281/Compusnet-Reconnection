//! In-memory ring log (last N entries) + event emission for the log window.

use crate::types::{LogEntry, LogLevel};
use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct RingLog {
    buf: Mutex<VecDeque<LogEntry>>,
    cap: usize,
    /// Optional file sink path; opened lazily on first write.
    file_path: Option<std::path::PathBuf>,
}

impl RingLog {
    pub fn new(cap: usize) -> Self {
        let file_path = std::env::var_os("APPDATA").map(|appdata| {
            let mut p = std::path::PathBuf::from(appdata);
            p.push("campus-net-reconnector");
            p.push("logs");
            p.push("app.log");
            p
        });
        if let Some(p) = &file_path {
            if let Some(dir) = p.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
        }
        Self {
            buf: Mutex::new(VecDeque::with_capacity(cap)),
            cap,
            file_path,
        }
    }

    pub fn info(&self, msg: impl Into<String>) {
        self.push(LogLevel::Info, msg.into());
    }

    pub fn warn(&self, msg: impl Into<String>) {
        self.push(LogLevel::Warn, msg.into());
    }

    pub fn error(&self, msg: impl Into<String>) {
        self.push(LogLevel::Error, msg.into());
    }

    fn push(&self, level: LogLevel, message: String) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        let entry = LogEntry {
            ts: now.as_millis() as u64,
            level,
            message: message.clone(),
        };
        {
            let mut buf = self.buf.lock().unwrap();
            if buf.len() >= self.cap {
                buf.pop_front();
            }
            buf.push_back(entry);
        }
        self.append_file(&format_log_line(now, level, &message));
    }

    pub fn snapshot(&self) -> Vec<LogEntry> {
        self.buf.lock().unwrap().iter().cloned().collect()
    }

    /// Most recent entry, or None when empty.
    pub fn last(&self) -> Option<LogEntry> {
        self.buf.lock().unwrap().back().cloned()
    }

    pub fn clear(&self) {
        self.buf.lock().unwrap().clear();
    }

    fn append_file(&self, line: &str) {
        let Some(path) = &self.file_path else { return };
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        {
            use std::io::Write;
            let _ = writeln!(f, "{line}");
        }
    }
}

fn format_log_line(now: std::time::Duration, level: LogLevel, msg: &str) -> String {
    let lvl = match level {
        LogLevel::Info => "INFO",
        LogLevel::Warn => "WARN",
        LogLevel::Error => "ERROR",
    };
    format!("[{}] {lvl} {msg}", now.as_secs())
}

/// Mask anything that looks like a password in messages destined for logs.
pub fn mask_password(msg: &str, password: &str) -> String {
    if password.is_empty() {
        return msg.to_string();
    }
    msg.replace(password, "******")
}
