//! Human-readable window-identity labels for sessions.
//!
//! In a parallel multi-window workflow, a bare session id (or a silent active
//! drift) makes it easy to send a prompt to the wrong window. A label turns a
//! session into something recognizable at a glance: `project · #shortid`.

/// Number of leading session-id characters used as the disambiguating short id.
const SHORT_ID_LEN: usize = 6;

/// Build a window label from a session id and its working directory.
///
/// Format: `<project> · #<shortid>` where `project` is the last path segment of
/// `working_dir`. Falls back gracefully when the working dir is missing.
///
/// Examples:
/// - `("a3f2c1d9...", Some("/Volumes/green/project/cc_remote_seamless"))`
///   → `cc_remote_seamless · #a3f2c1`
/// - `("a3f2c1d9...", None)` → `#a3f2c1`
pub fn window_label(session_id: &str, working_dir: Option<&str>) -> String {
    let short = short_id(session_id);
    match project_name(working_dir) {
        Some(project) => format!("{} · #{}", project, short),
        None => format!("#{}", short),
    }
}

/// Last path segment of the working dir, ignoring a trailing slash.
/// Returns `None` when the dir is absent or yields no usable segment.
fn project_name(working_dir: Option<&str>) -> Option<String> {
    let dir = working_dir?.trim_end_matches('/');
    if dir.is_empty() {
        return None;
    }
    dir.rsplit('/').next().filter(|s| !s.is_empty()).map(|s| s.to_string())
}

/// First `SHORT_ID_LEN` characters of the session id (char-safe).
fn short_id(session_id: &str) -> String {
    session_id.chars().take(SHORT_ID_LEN).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn label_with_project_and_short_id() {
        let label = window_label("a3f2c1d9e0", Some("/Volumes/green/project/cc_remote_seamless"));
        assert_eq!(label, "cc_remote_seamless · #a3f2c1");
    }

    #[test]
    fn label_without_working_dir_uses_short_id_only() {
        let label = window_label("a3f2c1d9e0", None);
        assert_eq!(label, "#a3f2c1");
    }

    #[test]
    fn label_trims_trailing_slash() {
        let label = window_label("a3f2c1d9e0", Some("/Volumes/green/project/seamless/"));
        assert_eq!(label, "seamless · #a3f2c1");
    }

    #[test]
    fn label_empty_working_dir_falls_back_to_short_id() {
        let label = window_label("a3f2c1d9e0", Some(""));
        assert_eq!(label, "#a3f2c1");
    }

    #[test]
    fn short_id_handles_session_shorter_than_limit() {
        let label = window_label("ab12", Some("/tmp/proj"));
        assert_eq!(label, "proj · #ab12");
    }

    #[test]
    fn short_id_is_char_safe_for_multibyte_ids() {
        // Defensive: session ids are ascii hex in practice, but never panic.
        let label = window_label("日本語セッション", None);
        assert_eq!(label, "#日本語セッシ");
    }
}
