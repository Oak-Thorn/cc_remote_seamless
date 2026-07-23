use std::path::PathBuf;

/// Filename scheme for rolling logs: `cc-remote.YYYY-MM-DD.log`. Defined once
/// here so the appender, the cleanup pass, and date parsing all agree on it.
pub(super) const FILE_PREFIX: &str = "cc-remote";
pub(super) const FILE_SUFFIX: &str = "log";

/// Directory where log files are stored: `~/.cc-remote/logs` (falling back to a
/// relative path if the home directory cannot be resolved).
pub fn log_dir() -> PathBuf {
    directories::BaseDirs::new()
        .map(|d| d.home_dir().join(".cc-remote").join("logs"))
        .unwrap_or_else(|| PathBuf::from(".cc-remote/logs"))
}
