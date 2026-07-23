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

/// Tools that require the user to make a choice (not just grant/deny a
/// side effect). Auto-approving these is meaningless: the permission is
/// granted but no answer is produced, so the request must always be routed
/// to the user regardless of the auto-approve toggle.
pub fn requires_user_input(tool: &str) -> bool {
    matches!(tool, "AskUserQuestion")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ask_user_question_requires_user_input() {
        assert!(requires_user_input("AskUserQuestion"));
    }

    #[test]
    fn side_effect_tools_do_not_require_user_input() {
        assert!(!requires_user_input("Bash"));
        assert!(!requires_user_input("Edit"));
        assert!(!requires_user_input("Write"));
    }
}
