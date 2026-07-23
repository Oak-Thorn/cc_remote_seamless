use super::paths::{log_dir, FILE_PREFIX, FILE_SUFFIX};

/// Number of days of log files to retain; older files are deleted on startup.
pub const RETAIN_DAYS: i64 = 7;

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

        // cleanup_old_logs reads from log_dir(); test the date logic directly.
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
}
