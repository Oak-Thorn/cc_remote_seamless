use std::sync::atomic::{AtomicBool, Ordering};

/// Global toggle controlling whether incoming tool permission requests are
/// automatically approved. Mirrors the process-wide flag pattern used by
/// `logging`. The frontend "Function" settings tab drives this via the
/// `set_auto_approve` command; the hook event loop reads it per request.
static ENABLED: AtomicBool = AtomicBool::new(false);

pub fn set_enabled(enabled: bool) {
    ENABLED.store(enabled, Ordering::Relaxed);
}

pub fn is_enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}
