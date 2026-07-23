//! Logging subsystem: a terminal (stdout) layer that is always on and a rolling
//! file layer under `~/.cc-remote/logs` that is gated by a runtime toggle.
//!
//! The module is split by concern to keep each piece cohesive and the public
//! surface small (`init`, `log_dir`, `set_enabled`, `cleanup_old_logs`,
//! `RETAIN_DAYS`) so callers only depend on this façade:
//!   - [`paths`]   — where logs live and the filename scheme.
//!   - [`gate`]    — the runtime enable toggle and the gated file writer.
//!   - [`cleanup`] — retention (deleting old files).

mod cleanup;
mod gate;
mod paths;

use std::path::PathBuf;

use tracing_appender::non_blocking::{NonBlockingBuilder, WorkerGuard};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

use gate::GatedMakeWriter;
use paths::{FILE_PREFIX, FILE_SUFFIX};

pub use cleanup::{cleanup_old_logs, RETAIN_DAYS};
pub use gate::set_enabled;
pub use paths::log_dir;

/// Initialize the terminal + file logging layers. Returns the appender's
/// [`WorkerGuard`], which the caller MUST keep alive for the lifetime of the
/// process: dropping it flushes any buffered lines to disk. Holding it in a
/// `run()` local (rather than a `static`, which never runs `Drop`) guarantees
/// the file captures the tail of the log at shutdown, matching the terminal.
pub fn init(enabled_initial: bool, dir: PathBuf) -> WorkerGuard {
    set_enabled(enabled_initial);

    let file_appender = tracing_appender::rolling::Builder::new()
        .rotation(tracing_appender::rolling::Rotation::DAILY)
        .filename_prefix(FILE_PREFIX)
        .filename_suffix(FILE_SUFFIX)
        .build(&dir)
        .expect("failed to build rolling log appender");
    // lossy(false): block briefly rather than silently drop lines when the
    // buffer is full, so the persisted file never diverges from the terminal
    // (which writes synchronously) while logging is enabled.
    let (non_blocking, guard) = NonBlockingBuilder::default()
        .lossy(false)
        .finish(file_appender);

    let stdout_layer = tracing_subscriber::fmt::layer();
    let file_layer = tracing_subscriber::fmt::layer()
        .with_ansi(false)
        .with_writer(GatedMakeWriter::new(non_blocking));

    tracing_subscriber::registry()
        .with(stdout_layer)
        .with(file_layer)
        .init();

    guard
}
