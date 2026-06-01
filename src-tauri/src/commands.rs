use crate::agent::SessionInfo;
use crate::engine::store::StoredMessage;
use crate::engine::Engine;
use crate::hook::server::{PermissionResponse, PermissionWaiters};
use std::sync::Arc;
use tauri::{AppHandle, Manager, State};
use tokio::sync::Mutex;

pub type EngineState = Arc<Mutex<Engine>>;

#[tauri::command]
pub async fn get_sessions(engine: State<'_, EngineState>) -> Result<Vec<SessionInfo>, String> {
    let eng = engine.lock().await;
    let sessions = eng.get_sessions().await;
    tracing::debug!("get_sessions command: returning {} sessions", sessions.len());
    Ok(sessions)
}

#[tauri::command]
pub async fn get_messages(
    engine: State<'_, EngineState>,
    session_id: String,
    limit: Option<usize>,
) -> Result<Vec<StoredMessage>, String> {
    let eng = engine.lock().await;
    Ok(eng.messages.get_by_session(&session_id, limit.unwrap_or(50)))
}

#[tauri::command]
pub async fn get_all_messages(
    engine: State<'_, EngineState>,
    limit: Option<usize>,
) -> Result<Vec<StoredMessage>, String> {
    let eng = engine.lock().await;
    Ok(eng.messages.get_all(limit.unwrap_or(50)))
}

#[tauri::command]
pub async fn bind_session(
    engine: State<'_, EngineState>,
    chat_id: String,
    agent_id: String,
    session_id: String,
) -> Result<(), String> {
    let eng = engine.lock().await;
    eng.bindings.bind(&chat_id, &agent_id, &session_id);
    Ok(())
}

#[tauri::command]
pub async fn inject_input(
    engine: State<'_, EngineState>,
    session_id: String,
    text: String,
) -> Result<(), String> {
    let eng = engine.lock().await;
    for agent in eng.agents_iter() {
        let sessions = agent.discover_sessions().await;
        if sessions.iter().any(|s| s.id == session_id) {
            return agent.inject_input(&session_id, &text).await;
        }
    }
    Err(format!("session {} not found", session_id))
}

#[tauri::command]
pub async fn respond_permission(
    waiters: State<'_, PermissionWaiters>,
    request_id: String,
    behavior: String,
    message: Option<String>,
) -> Result<(), String> {
    tracing::info!("respond_permission called: request_id={}, behavior={}, message={:?}", request_id, behavior, message);
    let mut waiters_map = waiters.lock().await;
    let keys: Vec<String> = waiters_map.keys().cloned().collect();
    tracing::info!("pending permission waiters: {:?}", keys);
    let entry = waiters_map.remove(&request_id);
    drop(waiters_map);
    match entry {
        Some(waiter_entry) => {
            let resolved_behavior = if behavior == "deny" { "deny" } else { "allow" };
            let updated_permissions = if behavior == "allowAlways" && !waiter_entry.suggestions.is_empty() {
                Some(waiter_entry.suggestions)
            } else {
                None
            };
            let response = PermissionResponse {
                behavior: resolved_behavior.to_string(),
                message: if resolved_behavior == "deny" {
                    Some("Denied by user".to_string())
                } else {
                    message
                },
                updated_permissions,
            };
            tracing::info!("respond_permission: sending response behavior={} has_permissions={}", resolved_behavior, response.updated_permissions.is_some());
            let _ = waiter_entry.sender.send(response);
            Ok(())
        }
        None => {
            tracing::error!("respond_permission: NO waiter found for request_id={}", request_id);
            Err("No pending permission request".to_string())
        }
    }
}

#[tauri::command]
pub async fn pin_session(
    engine: State<'_, EngineState>,
    session_id: String,
) -> Result<(), String> {
    let eng = engine.lock().await;
    eng.bindings.bind_pinned_session(&session_id);
    Ok(())
}

#[tauri::command]
pub async fn get_active_session(
    engine: State<'_, EngineState>,
) -> Result<Option<String>, String> {
    let eng = engine.lock().await;
    Ok(eng.bindings.get_active_session_id())
}

#[tauri::command]
pub async fn get_config_path() -> Result<String, String> {
    directories::BaseDirs::new()
        .map(|d| d.home_dir().join(".cc-remote").join("config.toml").to_string_lossy().to_string())
        .ok_or_else(|| "Cannot determine home directory".to_string())
}

#[tauri::command]
pub async fn get_home_dir() -> Result<String, String> {
    directories::BaseDirs::new()
        .map(|d| d.home_dir().to_string_lossy().to_string())
        .ok_or_else(|| "Cannot determine home directory".to_string())
}

#[tauri::command]
pub async fn open_config_dir() -> Result<(), String> {
    let dir = directories::BaseDirs::new()
        .map(|d| d.home_dir().join(".cc-remote"))
        .ok_or_else(|| "Cannot determine home directory".to_string())?;
    if !dir.exists() {
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(dir.to_string_lossy().to_string())
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(dir.to_string_lossy().to_string())
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub async fn read_config_file() -> Result<String, String> {
    let path = directories::BaseDirs::new()
        .map(|d| d.home_dir().join(".cc-remote").join("config.toml"))
        .ok_or_else(|| "Cannot determine home directory".to_string())?;
    if !path.exists() {
        return Ok(String::new());
    }
    std::fs::read_to_string(&path).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn open_settings(app: AppHandle) -> Result<(), String> {
    crate::window::open_settings_window(&app)
}

#[tauri::command]
pub async fn open_terminal(session_id: String) -> Result<(), String> {
    let home = std::env::var("HOME").map_err(|e| format!("HOME not set: {}", e))?;
    let sessions_dir = std::path::Path::new(&home).join(".claude").join("sessions");

    // Find PID from session files
    let mut target_pid: Option<i32> = None;
    if let Ok(entries) = std::fs::read_dir(&sessions_dir) {
        for entry in entries.flatten() {
            if entry.path().extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            if let Ok(content) = std::fs::read_to_string(entry.path()) {
                if let Ok(info) = serde_json::from_str::<serde_json::Value>(&content) {
                    let sid = info.get("sessionId").and_then(|v| v.as_str()).unwrap_or_default();
                    if sid == session_id {
                        target_pid = info.get("pid").and_then(|v| v.as_i64()).map(|p| p as i32);
                        break;
                    }
                }
            }
        }
    }

    let pid = target_pid.ok_or_else(|| format!("session {} not found in files", session_id))?;

    // Get TTY of the process
    let tty_output = tokio::process::Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "tty="])
        .output()
        .await
        .map_err(|e| format!("ps failed: {}", e))?;
    let tty_name = String::from_utf8_lossy(&tty_output.stdout).trim().to_string();
    if tty_name.is_empty() {
        return Err(format!("cannot find TTY for pid {}", pid));
    }
    let tty_path = format!("/dev/{}", tty_name);

    #[cfg(target_os = "macos")]
    {
        let script = format!(
            r#"tell application "Terminal"
    activate
    repeat with w in windows
        repeat with t in tabs of w
            if tty of t is "{}" then
                set selected of t to true
                set index of w to 1
                return
            end if
        end repeat
    end repeat
end tell"#,
            tty_path
        );
        tokio::process::Command::new("osascript")
            .arg("-e")
            .arg(&script)
            .output()
            .await
            .map_err(|e| format!("osascript failed: {}", e))?;
    }
    Ok(())
}

#[tauri::command]
pub async fn set_sound_preference(
    kind: String,
    name: String,
    engine: State<'_, EngineState>,
) -> Result<(), String> {
    let mut eng = engine.lock().await;
    match kind.as_str() {
        "idle" => eng.set_idle_sound(name),
        "permission" => eng.set_permission_sound(name),
        _ => return Err(format!("Unknown sound kind: {}", kind)),
    }
    Ok(())
}

#[tauri::command]
pub async fn play_sound(name: String, app: AppHandle) -> Result<(), String> {
    let sound_path = resolve_sound_path(&name, Some(&app));
    std::thread::spawn(move || {
        #[cfg(target_os = "macos")]
        {
            let _ = std::process::Command::new("afplay").arg(&sound_path).spawn();
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = sound_path; // suppress unused warning
        }
    });
    Ok(())
}

/// Supported audio extensions, in order of preference.
const SOUND_EXTENSIONS: &[&str] = &["aiff", "mp3", "wav"];

/// Resolve a sound file path. Tries bundled resources first, then system fallback.
/// Supports .aiff, .mp3, .wav extensions.
fn resolve_sound_path(name: &str, app: Option<&AppHandle>) -> String {
    let search_dirs = build_search_dirs(app);

    for dir in &search_dirs {
        for ext in SOUND_EXTENSIONS {
            let path = dir.join(format!("{}.{}", name, ext));
            if path.exists() {
                return path.to_string_lossy().to_string();
            }
        }
    }

    // Final fallback: macOS system sounds (only .aiff)
    format!("/System/Library/Sounds/{}.aiff", name)
}

/// Build list of directories to search for sound files, in priority order.
fn build_search_dirs(app: Option<&AppHandle>) -> Vec<std::path::PathBuf> {
    let mut dirs = Vec::new();

    // User custom sounds (highest priority)
    if let Some(base) = directories::BaseDirs::new() {
        dirs.push(base.home_dir().join(".cc-remote").join("sounds"));
    }

    // Tauri resource directory (bundled in production)
    if let Some(app_handle) = app {
        if let Ok(resource_dir) = app_handle.path().resource_dir() {
            dirs.push(resource_dir.join("resources").join("sounds"));
        }
    }

    // Dev mode: relative to project root
    if let Ok(cargo_dir) = std::env::var("CARGO_MANIFEST_DIR") {
        dirs.push(std::path::Path::new(&cargo_dir).join("resources").join("sounds"));
    }

    dirs
}

/// List available sound files from the resources directory.
/// Returns sound names (filename without extension) for files with supported extensions.
#[tauri::command]
pub async fn list_available_sounds(app: AppHandle) -> Result<Vec<SoundInfo>, String> {
    let search_dirs = build_search_dirs(Some(&app));
    let mut seen = std::collections::HashSet::new();
    let mut sounds = Vec::new();

    for dir in &search_dirs {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_file() { continue; }
                let ext = path.extension()
                    .and_then(|e| e.to_str())
                    .map(|e| e.to_lowercase())
                    .unwrap_or_default();
                if !SOUND_EXTENSIONS.contains(&ext.as_str()) { continue; }
                let name = path.file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or_default()
                    .to_string();
                if name.is_empty() || seen.contains(&name) { continue; }
                seen.insert(name.clone());
                sounds.push(SoundInfo { name, extension: ext });
            }
        }
    }

    sounds.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(sounds)
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SoundInfo {
    pub name: String,
    pub extension: String,
}

#[tauri::command]
pub async fn read_icon_svg(name: String, app: AppHandle) -> Result<String, String> {
    let mut dirs = Vec::new();
    if let Some(base) = directories::BaseDirs::new() {
        dirs.push(base.home_dir().join(".cc-remote").join("icons"));
    }
    if let Ok(resource_dir) = app.path().resource_dir() {
        dirs.push(resource_dir.join("icons"));
    }
    if let Ok(cargo_dir) = std::env::var("CARGO_MANIFEST_DIR") {
        dirs.push(std::path::Path::new(&cargo_dir).join("resources").join("icons"));
    }
    for dir in &dirs {
        let path = dir.join(format!("{}.svg", name));
        if path.exists() {
            return std::fs::read_to_string(&path).map_err(|e| e.to_string());
        }
    }
    Err(format!("Icon not found: {}", name))
}

#[tauri::command]
pub async fn list_available_icons(app: AppHandle) -> Result<Vec<String>, String> {
    let mut dirs = Vec::new();

    // User custom icons (highest priority)
    if let Some(base) = directories::BaseDirs::new() {
        dirs.push(base.home_dir().join(".cc-remote").join("icons"));
    }

    // Bundled resources
    if let Ok(resource_dir) = app.path().resource_dir() {
        dirs.push(resource_dir.join("icons"));
    }
    if let Ok(cargo_dir) = std::env::var("CARGO_MANIFEST_DIR") {
        dirs.push(std::path::Path::new(&cargo_dir).join("resources").join("icons"));
    }

    let mut seen = std::collections::HashSet::new();
    let mut icons = Vec::new();

    for dir in &dirs {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("svg") { continue; }
                let name = path.file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or_default()
                    .to_string();
                if name.is_empty() || seen.contains(&name) { continue; }
                seen.insert(name.clone());
                icons.push(name);
            }
        }
    }

    icons.sort();
    Ok(icons)
}

#[tauri::command]
pub async fn open_custom_dir(kind: String) -> Result<String, String> {
    let base = directories::BaseDirs::new()
        .ok_or_else(|| "Cannot determine home directory".to_string())?;
    let dir = base.home_dir().join(".cc-remote").join(&kind);
    if !dir.exists() {
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    }
    let path_str = dir.to_string_lossy().to_string();
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&path_str)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(&path_str)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    Ok(path_str)
}

/// Scan all SVG files in the icons resource directory and replace hardcoded
/// fill values with `currentColor` — both in root <svg> fill attribute and
/// in inline <style> CSS blocks (e.g. `.st0{fill:#000000;}`).
#[tauri::command]
pub async fn fix_svg_icons(app: AppHandle) -> Result<u32, String> {
    let mut dirs = Vec::new();
    if let Some(base) = directories::BaseDirs::new() {
        dirs.push(base.home_dir().join(".cc-remote").join("icons"));
    }
    if let Ok(resource_dir) = app.path().resource_dir() {
        dirs.push(resource_dir.join("icons"));
    }
    if let Ok(cargo_dir) = std::env::var("CARGO_MANIFEST_DIR") {
        dirs.push(std::path::Path::new(&cargo_dir).join("resources").join("icons"));
    }

    let mut fixed_count: u32 = 0;
    for dir in &dirs {
        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("svg") {
                continue;
            }
            let content = match std::fs::read_to_string(&path) {
                Ok(c) => c,
                Err(_) => continue,
            };

            let mut modified = content.clone();
            let mut changed = false;

            // Fix 1: fill="..." attribute on the root <svg> element
            if let Some(svg_start) = modified.find("<svg") {
                if let Some(svg_tag_end) = modified[svg_start..].find('>') {
                    let svg_tag_abs_end = svg_start + svg_tag_end;
                    let svg_tag = modified[svg_start..svg_tag_abs_end].to_string();
                    if let Some(fill_start) = svg_tag.find("fill=\"") {
                        let abs_fill_start = svg_start + fill_start;
                        let val_start = abs_fill_start + 6;
                        if let Some(val_end) = modified[val_start..].find('"') {
                            let fill_val = &modified[val_start..val_start + val_end];
                            if fill_val != "currentColor" {
                                modified = format!(
                                    "{}fill=\"currentColor\"{}",
                                    &modified[..abs_fill_start],
                                    &modified[val_start + val_end + 1..]
                                );
                                changed = true;
                            }
                        }
                    }
                }
            }

            // Fix 2: fill:... in CSS <style> blocks (e.g. fill:#000000 or fill: #HEX)
            while let Some(style_start) = modified.find("<style") {
                let style_end = match modified[style_start..].find("</style>") {
                    Some(pos) => style_start + pos + 8,
                    None => break,
                };
                let style_block = &modified[style_start..style_end];
                let new_block = fix_css_fills(style_block);
                if new_block != style_block {
                    modified = format!(
                        "{}{}{}",
                        &modified[..style_start],
                        new_block,
                        &modified[style_end..]
                    );
                    changed = true;
                }
                break;
            }

            if changed {
                let _ = std::fs::write(&path, modified);
                fixed_count += 1;
            }
        }
    }
    Ok(fixed_count)
}

/// Replace hardcoded fill color values in a CSS block with `currentColor`.
fn fix_css_fills(style_block: &str) -> String {
    let mut result = String::with_capacity(style_block.len());
    let mut remaining = style_block;
    while let Some(idx) = remaining.find("fill:") {
        result.push_str(&remaining[..idx]);
        result.push_str("fill:");
        let after_fill = &remaining[idx + 5..];
        let val_start = after_fill.len() - after_fill.trim_start().len();
        let trimmed = after_fill.trim_start();
        let val_end = trimmed.find(|c: char| c == ';' || c == '}').unwrap_or(trimmed.len());
        let fill_val = trimmed[..val_end].trim();
        if fill_val != "currentColor" {
            result.push_str("currentColor");
        } else {
            result.push_str(&after_fill[..val_start + val_end]);
        }
        remaining = &after_fill[val_start + val_end..];
    }
    result.push_str(remaining);
    result
}
