use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, Ordering};

use tracing_appender::non_blocking::NonBlocking;
use tracing_subscriber::fmt::MakeWriter;

static LOG_ENABLED: AtomicBool = AtomicBool::new(false);

/// Enable or disable persisting logs to the file layer at runtime. The terminal
/// layer is unaffected (always on); only the gated file writer honours this
/// flag. It is checked per write so the settings toggle takes effect
/// immediately without rebuilding the subscriber.
pub fn set_enabled(enabled: bool) {
    LOG_ENABLED.store(enabled, Ordering::Relaxed);
}

fn is_enabled() -> bool {
    LOG_ENABLED.load(Ordering::Relaxed)
}

/// Writer that forwards to the rolling file appender only while logging is
/// enabled, otherwise discards the bytes (reporting them as written).
pub(super) struct GatedWriter {
    inner: NonBlocking,
}

impl Write for GatedWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if is_enabled() {
            self.inner.write(buf)
        } else {
            Ok(buf.len())
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        if is_enabled() {
            self.inner.flush()
        } else {
            Ok(())
        }
    }
}

/// `MakeWriter` producing [`GatedWriter`]s over a shared non-blocking appender.
#[derive(Clone)]
pub(super) struct GatedMakeWriter {
    inner: NonBlocking,
}

impl GatedMakeWriter {
    pub(super) fn new(inner: NonBlocking) -> Self {
        Self { inner }
    }
}

impl<'a> MakeWriter<'a> for GatedMakeWriter {
    type Writer = GatedWriter;

    fn make_writer(&'a self) -> Self::Writer {
        GatedWriter { inner: self.inner.clone() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_enabled_toggles_flag() {
        set_enabled(false);
        assert!(!is_enabled());
        set_enabled(true);
        assert!(is_enabled());
        set_enabled(false);
    }
}
