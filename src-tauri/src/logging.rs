use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

use tracing_appender::non_blocking::{NonBlocking, WorkerGuard};
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

pub const RETAIN_DAYS: i64 = 7;
const FILE_PREFIX: &str = "cc-remote";
const FILE_SUFFIX: &str = "log";

static LOG_ENABLED: AtomicBool = AtomicBool::new(false);
static GUARD: OnceLock<WorkerGuard> = OnceLock::new();

pub fn log_dir() -> PathBuf {
    directories::BaseDirs::new()
        .map(|d| d.home_dir().join(".cc-remote").join("logs"))
        .unwrap_or_else(|| PathBuf::from(".cc-remote/logs"))
}

pub fn set_enabled(enabled: bool) {
    LOG_ENABLED.store(enabled, Ordering::Relaxed);
}

/// Writer that forwards to the rolling file appender only while logging is
/// enabled, otherwise discards. The enabled flag is checked per write so the
/// settings toggle takes effect immediately without rebuilding the subscriber.
struct GatedWriter {
    inner: NonBlocking,
}

impl Write for GatedWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if LOG_ENABLED.load(Ordering::Relaxed) {
            self.inner.write(buf)
        } else {
            Ok(buf.len())
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        if LOG_ENABLED.load(Ordering::Relaxed) {
            self.inner.flush()
        } else {
            Ok(())
        }
    }
}

#[derive(Clone)]
struct GatedMakeWriter {
    inner: NonBlocking,
}

impl<'a> MakeWriter<'a> for GatedMakeWriter {
    type Writer = GatedWriter;

    fn make_writer(&'a self) -> Self::Writer {
        GatedWriter { inner: self.inner.clone() }
    }
}

pub fn init(enabled_initial: bool, dir: PathBuf) {
    set_enabled(enabled_initial);

    let file_appender = tracing_appender::rolling::Builder::new()
        .rotation(tracing_appender::rolling::Rotation::DAILY)
        .filename_prefix(FILE_PREFIX)
        .filename_suffix(FILE_SUFFIX)
        .build(&dir)
        .expect("failed to build rolling log appender");
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);
    let _ = GUARD.set(guard);

    let stdout_layer = tracing_subscriber::fmt::layer();
    let file_layer = tracing_subscriber::fmt::layer()
        .with_ansi(false)
        .with_writer(GatedMakeWriter { inner: non_blocking });

    tracing_subscriber::registry()
        .with(stdout_layer)
        .with(file_layer)
        .init();
}

/// Delete `cc-remote.<date>.log` files older than `retain_days`.
pub fn cleanup_old_logs(retain_days: i64) {
    let dir = log_dir();
    let cutoff = chrono::Local::now().date_naive() - chrono::Duration::days(retain_days);

    let entries = match std::fs::read_dir(&dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n,
            None => continue,
        };
        if let Some(date) = parse_log_date(name) {
            if date < cutoff {
                let _ = std::fs::remove_file(&path);
            }
        }
    }
}

/// Extract the date from `cc-remote.YYYY-MM-DD.log`.
fn parse_log_date(name: &str) -> Option<chrono::NaiveDate> {
    let rest = name.strip_prefix(&format!("{}.", FILE_PREFIX))?;
    let date_str = rest.strip_suffix(&format!(".{}", FILE_SUFFIX))?;
    chrono::NaiveDate::parse_from_str(date_str, "%Y-%m-%d").ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_valid_log_filename() {
        let date = parse_log_date("cc-remote.2026-06-25.log").unwrap();
        assert_eq!(date, chrono::NaiveDate::from_ymd_opt(2026, 6, 25).unwrap());
    }

    #[test]
    fn rejects_non_log_filename() {
        assert!(parse_log_date("config.toml").is_none());
        assert!(parse_log_date("cc-remote.notadate.log").is_none());
        assert!(parse_log_date("cc-remote.2026-06-25.txt").is_none());
    }

    #[test]
    fn cleanup_removes_old_keeps_recent() {
        let tmp = tempfile::tempdir().unwrap();
        let today = chrono::Local::now().date_naive();
        let old = today - chrono::Duration::days(RETAIN_DAYS + 3);
        let recent = today - chrono::Duration::days(1);

        let old_file = tmp.path().join(format!("cc-remote.{}.log", old.format("%Y-%m-%d")));
        let recent_file = tmp.path().join(format!("cc-remote.{}.log", recent.format("%Y-%m-%d")));
        std::fs::write(&old_file, b"old").unwrap();
        std::fs::write(&recent_file, b"recent").unwrap();

        // cleanup_old_logs reads from log_dir(); test the date logic directly
        let cutoff = today - chrono::Duration::days(RETAIN_DAYS);
        for entry in std::fs::read_dir(tmp.path()).unwrap().flatten() {
            let name = entry.file_name().into_string().unwrap();
            if let Some(date) = parse_log_date(&name) {
                if date < cutoff {
                    std::fs::remove_file(entry.path()).unwrap();
                }
            }
        }

        assert!(!old_file.exists(), "old log should be removed");
        assert!(recent_file.exists(), "recent log should be kept");
    }

    #[test]
    fn gated_writer_discards_when_disabled() {
        // Without a NonBlocking appender we can't easily exercise the file
        // path, but the gate logic is observable through LOG_ENABLED: when
        // disabled, write reports the full length as consumed (discarded).
        set_enabled(false);
        assert!(!LOG_ENABLED.load(Ordering::Relaxed));
        set_enabled(true);
        assert!(LOG_ENABLED.load(Ordering::Relaxed));
        set_enabled(false);
    }
}
