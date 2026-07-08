use tauri::{AppHandle, Emitter, Manager, WebviewWindowBuilder, WebviewUrl};
use tracing::info;

pub mod popup_queue;

fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 2);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => {
                out.push_str(&format!("%{:02X}", b));
            }
        }
    }
    out
}

pub async fn show_permission_popup(
    app: &AppHandle,
    session_id: &str,
    tool: &str,
    input: &str,
    request_id: &str,
) -> Result<(), String> {
    // A previous popup may still be open. `close()` only requests a close and
    // is processed asynchronously on the main event loop, so building a new
    // window with the same "permission" label immediately afterwards races the
    // teardown and fails with "a window with label `permission` already
    // exists" — which left no popup at all. Use `destroy()` (frees the label
    // without a close-request round-trip) and then wait until the label is
    // actually gone before rebuilding.
    if let Some(win) = app.get_webview_window("permission") {
        let _ = win.destroy();
        const MAX_WAIT_MS: u64 = 1000;
        const POLL_MS: u64 = 20;
        let mut waited = 0;
        while app.get_webview_window("permission").is_some() && waited < MAX_WAIT_MS {
            tokio::time::sleep(std::time::Duration::from_millis(POLL_MS)).await;
            waited += POLL_MS;
        }
    }

    // Only the request_id goes in the URL. Tool/session/input are fetched by
    // the popup via the get_permission_request command — embedding a large
    // input (e.g. an ExitPlanMode plan) here overflowed the webview's request
    // header limit and left the window blank.
    let url = format!("/?view=permission&request_id={}", url_encode(request_id));

    let _ = session_id;
    let height = popup_height(tool, input);

    WebviewWindowBuilder::new(app, "permission", WebviewUrl::App(url.into()))
        .title("Permission")
        .inner_size(420.0, height)
        .always_on_top(true)
        // On macOS an unfocused window swallows its first click just to become
        // key, so the radio needed two clicks. Request focus on build and let
        // the first mouse-down pass through to the webview content.
        .focused(true)
        .accept_first_mouse(true)
        .resizable(false)
        .decorations(false)
        .build()
        .map_err(|e| e.to_string())?;

    let _ = app.emit("permission-request-shown", ());
    info!("Permission popup shown: tool={} height={}", tool, height);
    Ok(())
}

/// AskUserQuestion popups grow with the number of questions/options so the
/// content and the Submit button stay on-screen. Other tools use a fixed size.
fn popup_height(tool: &str, input: &str) -> f64 {
    const BASE: f64 = 120.0; // titlebar + header + Submit + padding
    const PER_QUESTION: f64 = 56.0; // header + question text + gap
    const PER_OPTION: f64 = 50.0; // one option row
    const MAX: f64 = 720.0;
    const DEFAULT: f64 = 320.0;

    if tool != "AskUserQuestion" {
        return DEFAULT;
    }
    let Ok(obj) = serde_json::from_str::<serde_json::Value>(input) else {
        return DEFAULT;
    };
    let Some(questions) = obj.get("questions").and_then(|v| v.as_array()) else {
        return DEFAULT;
    };
    let mut h = BASE;
    for q in questions {
        h += PER_QUESTION;
        let opts = q.get("options").and_then(|v| v.as_array()).map(|o| o.len()).unwrap_or(0);
        // +1 for the synthetic "Other" row on single-select questions
        let is_multi = q.get("multiSelect").and_then(|v| v.as_bool()).unwrap_or(false);
        let rows = if is_multi { opts } else { opts + 1 };
        h += rows as f64 * PER_OPTION;
    }
    h.min(MAX).max(DEFAULT)
}

pub fn open_settings_window(app: &AppHandle) -> Result<(), String> {
    if let Some(win) = app.get_webview_window("settings") {
        win.set_focus().map_err(|e| e.to_string())?;
        return Ok(());
    }

    WebviewWindowBuilder::new(app, "settings", WebviewUrl::App("/?view=settings".into()))
        .title("Settings")
        .inner_size(560.0, 400.0)
        .min_inner_size(400.0, 300.0)
        .resizable(true)
        .decorations(false)
        .center()
        .build()
        .map_err(|e| e.to_string())?;

    Ok(())
}

pub fn open_main_window(app: &AppHandle) -> Result<(), String> {
    if let Some(win) = app.get_webview_window("main") {
        win.set_focus().map_err(|e| e.to_string())?;
        return Ok(());
    }

    WebviewWindowBuilder::new(app, "main", WebviewUrl::App("/?view=main".into()))
        .title("CC Remote Seamless")
        .inner_size(600.0, 400.0)
        .build()
        .map_err(|e| e.to_string())?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_question_tool_uses_default_height() {
        assert_eq!(popup_height("Bash", "{\"command\":\"ls\"}"), 320.0);
    }

    #[test]
    fn single_question_grows_above_default() {
        let input = serde_json::json!({
            "questions": [{
                "header": "h", "question": "q?", "multiSelect": false,
                "options": [{"label":"A","description":"d"},{"label":"B","description":"d"}]
            }]
        }).to_string();
        // 120 + 56 + (2+1 "Other")*50 = 326
        assert_eq!(popup_height("AskUserQuestion", &input), 326.0);
    }

    #[test]
    fn many_questions_clamp_to_max() {
        let q = serde_json::json!({
            "header": "h", "question": "q?", "multiSelect": false,
            "options": [{"label":"A","description":"d"},{"label":"B","description":"d"},
                        {"label":"C","description":"d"},{"label":"D","description":"d"}]
        });
        let input = serde_json::json!({ "questions": [q.clone(), q.clone(), q.clone(), q] }).to_string();
        assert_eq!(popup_height("AskUserQuestion", &input), 720.0);
    }

    #[test]
    fn multiselect_has_no_other_row() {
        let multi = serde_json::json!({
            "questions": [{
                "header": "h", "question": "q?", "multiSelect": true,
                "options": [{"label":"A","description":"d"},{"label":"B","description":"d"}]
            }]
        }).to_string();
        // 120 + 56 + 2*50 = 276 → clamped up to DEFAULT 320
        assert_eq!(popup_height("AskUserQuestion", &multi), 320.0);
    }

    #[test]
    fn malformed_input_falls_back_to_default() {
        assert_eq!(popup_height("AskUserQuestion", "not json"), 320.0);
    }
}
