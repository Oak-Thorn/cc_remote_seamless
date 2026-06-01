use tauri::{AppHandle, Emitter, Manager, WebviewWindowBuilder, WebviewUrl};
use tracing::info;

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

pub fn show_permission_popup(
    app: &AppHandle,
    session_id: &str,
    tool: &str,
    input: &str,
    request_id: &str,
) -> Result<(), String> {
    if let Some(win) = app.get_webview_window("permission") {
        let _ = win.close();
    }

    let url = format!(
        "/?view=permission&session={}&tool={}&input={}&request_id={}",
        url_encode(session_id),
        url_encode(tool),
        url_encode(input),
        url_encode(request_id),
    );

    WebviewWindowBuilder::new(app, "permission", WebviewUrl::App(url.into()))
        .title("Permission")
        .inner_size(420.0, 320.0)
        .always_on_top(true)
        .resizable(false)
        .decorations(false)
        .build()
        .map_err(|e| e.to_string())?;

    let _ = app.emit("permission-request-shown", ());
    info!("Permission popup shown: tool={}", tool);
    Ok(())
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
