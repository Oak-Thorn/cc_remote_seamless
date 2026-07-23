use crate::agent::{AgentConnector, AgentEvent, EventSender, SessionInfo, SessionState};
use crate::hook::server::HookEvent;
use std::collections::HashMap;
use std::sync::{Mutex, RwLock};
use std::time::Instant;
use tracing::info;

/// Within this window after a hook event, the live hook stream is authoritative
/// and file-based reconciliation must NOT override the in-memory state. The CC
/// session file's `status` lags real state, so an unguarded reconcile flips an
/// actively-busy session to Idle between tool calls.
const RECONCILE_HOOK_GUARD_SECS: u64 = 5;

struct PtyConnection {
    session_id: String,
    state: SessionState,
    working_dir: Option<String>,
    last_event: Instant,
}

pub struct ClaudeCodeConnector {
    sessions: RwLock<HashMap<String, PtyConnection>>,
    event_senders: Mutex<Vec<EventSender>>,
    #[allow(dead_code)]
    socket_dir: String,
}

impl ClaudeCodeConnector {
    pub fn new(socket_dir: &str) -> Self {
        Self {
            sessions: RwLock::new(HashMap::new()),
            event_senders: Mutex::new(vec![]),
            socket_dir: socket_dir.to_string(),
        }
    }

    pub fn discover_existing_sessions(&self) {
        let home = match std::env::var("HOME") {
            Ok(h) => h,
            Err(e) => { tracing::warn!("HOME not set: {}", e); return; }
        };
        let sessions_dir = std::path::Path::new(&home).join(".claude").join("sessions");
        tracing::info!("Scanning sessions dir: {}", sessions_dir.display());
        let entries = match std::fs::read_dir(&sessions_dir) {
            Ok(e) => e,
            Err(e) => { tracing::warn!("Cannot read sessions dir: {}", e); return; }
        };
        let mut count = 0;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                tracing::debug!("Skipping non-json: {}", path.display());
                continue;
            }
            match std::fs::read_to_string(&path) {
                Ok(content) => {
                    match serde_json::from_str::<serde_json::Value>(&content) {
                        Ok(info) => {
                            let pid = info.get("pid").and_then(|v| v.as_u64()).unwrap_or(0) as i32;
                            let alive = pid > 0 && process_alive(pid);
                            let session_id = info.get("sessionId").and_then(|v| v.as_str()).unwrap_or_default().to_string();
                            tracing::info!("Session file: {} pid={} alive={} id={}", path.display(), pid, alive, session_id);
                            if alive && !session_id.is_empty() {
                                let cwd = info.get("cwd").and_then(|v| v.as_str()).map(|s| s.to_string());
                                let raw_status = info.get("status").and_then(|v| v.as_str());
                                let state = match raw_status {
                                    Some("busy") => SessionState::Busy,
                                    Some("idle") => SessionState::Idle,
                                    other => {
                                        tracing::warn!(
                                            "Session {} has unexpected status {:?}, defaulting to Idle on initial discovery",
                                            session_id, other
                                        );
                                        SessionState::Idle
                                    }
                                };
                                info!("Discovered existing CC session: {} (pid={}, cwd={:?})", session_id, pid, cwd);
                                self.update_state(&session_id, state, cwd);
                                count += 1;
                            }
                        }
                        Err(e) => tracing::warn!("Failed to parse session file {}: {}", path.display(), e),
                    }
                }
                Err(e) => tracing::warn!("Failed to read session file {}: {}", path.display(), e),
            }
        }
        let total_in_map = self.sessions.read().unwrap().len();
        tracing::info!("Discover complete: {} active sessions found, {} total in HashMap", count, total_in_map);
    }

    pub fn handle_hook_event(&self, event: HookEvent) {
        match event {
            HookEvent::SessionStart { session_id, cwd } => {
                self.update_state(&session_id, SessionState::Idle, cwd);
            }
            HookEvent::Stop { session_id, cwd, .. } => {
                self.update_state(&session_id, SessionState::Idle, cwd);
            }
            HookEvent::PromptSubmit { session_id, cwd, .. } => {
                self.update_state(&session_id, SessionState::Busy, cwd);
            }
            HookEvent::PreToolUse { session_id, cwd, .. } => {
                self.update_state(&session_id, SessionState::Busy, cwd);
            }
            HookEvent::PermissionRequest { session_id, tool, input, cwd, .. } => {
                // When the auto-approve toggle will resolve this request
                // immediately (see the hook loop in lib.rs), skip the
                // WaitingPermission transition: the session shouldn't flicker
                // into "waiting" — nor play the permission sound — for a
                // request no human will ever act on. Tools that require user
                // input are never auto-approved, so they still wait here.
                let will_auto_approve = crate::auto_approve::is_enabled()
                    && !crate::auto_approve::requires_user_input(&tool);
                if !will_auto_approve {
                    self.update_state(&session_id, SessionState::WaitingPermission, cwd);
                }
                self.emit(AgentEvent::PermissionRequest { session_id, tool, input });
            }
            HookEvent::SessionEnd { session_id } => {
                self.sessions.write().unwrap().remove(&session_id);
                self.emit(AgentEvent::StateChange { session_id, state: SessionState::Idle });
            }
            HookEvent::PostToolUse { session_id, cwd, .. } => {
                self.update_state(&session_id, SessionState::Busy, cwd);
            }
            HookEvent::PostToolUseFailure { session_id, cwd, .. } => {
                self.update_state(&session_id, SessionState::Busy, cwd);
            }
            HookEvent::StopFailure { session_id, cwd } => {
                self.update_state(&session_id, SessionState::Idle, cwd);
            }
            HookEvent::SubagentStart { session_id, cwd } => {
                self.update_state(&session_id, SessionState::Busy, cwd);
            }
            HookEvent::SubagentStop { session_id, cwd } => {
                self.update_state(&session_id, SessionState::Busy, cwd);
            }
            HookEvent::Notification { session_id, cwd, notification_type } => {
                if notification_type.as_deref() == Some("permission_prompt") {
                    self.update_state(&session_id, SessionState::WaitingPermission, cwd);
                }
            }
            HookEvent::Elicitation { session_id, cwd, .. } => {
                self.update_state(&session_id, SessionState::WaitingPermission, cwd);
            }
            HookEvent::PreCompact { session_id, cwd } => {
                self.update_state(&session_id, SessionState::Busy, cwd);
            }
            HookEvent::PostCompact { session_id, cwd } => {
                self.update_state(&session_id, SessionState::Busy, cwd);
            }
            _ => {}
        }
    }

    fn update_state(&self, session_id: &str, state: SessionState, cwd: Option<String>) {
        self.update_state_from(session_id, state, cwd, "hook");
    }

    fn update_state_from(&self, session_id: &str, state: SessionState, cwd: Option<String>, source: &str) {
        let mut sessions = self.sessions.write().unwrap();
        let prev_state = sessions.get(session_id).map(|c| c.state.clone());
        if let Some(conn) = sessions.get_mut(session_id) {
            if cwd.is_some() {
                conn.working_dir = cwd;
            }
            conn.state = state.clone();
            conn.last_event = Instant::now();
        } else {
            sessions.insert(session_id.to_string(), PtyConnection {
                session_id: session_id.to_string(),
                state: state.clone(),
                working_dir: cwd,
                last_event: Instant::now(),
            });
        }
        drop(sessions);

        if prev_state.as_ref() != Some(&state) {
            info!(
                "CC state transition: session={} {:?} -> {:?} (source={})",
                session_id, prev_state, state, source
            );
            self.emit(AgentEvent::StateChange {
                session_id: session_id.to_string(),
                state,
            });
        }
    }

    /// Re-read session files and reconcile state for sessions that might have
    /// gone idle without us receiving a Stop hook event.
    pub fn reconcile_from_files(&self) {
        let home = match std::env::var("HOME") {
            Ok(h) => h,
            Err(_) => return,
        };
        let sessions_dir = std::path::Path::new(&home).join(".claude").join("sessions");
        let entries = match std::fs::read_dir(&sessions_dir) {
            Ok(e) => e,
            Err(_) => return,
        };

        let mut known_ids: std::collections::HashSet<String> = {
            let sessions = self.sessions.read().unwrap();
            sessions.keys().cloned().collect()
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            if let Ok(content) = std::fs::read_to_string(&path) {
                if let Ok(info) = serde_json::from_str::<serde_json::Value>(&content) {
                    let pid = info.get("pid").and_then(|v| v.as_u64()).unwrap_or(0) as i32;
                    let session_id = info.get("sessionId").and_then(|v| v.as_str()).unwrap_or_default().to_string();
                    if session_id.is_empty() {
                        continue;
                    }
                    known_ids.remove(&session_id);

                    let alive = pid > 0 && process_alive(pid);
                    if !alive {
                        let mut sessions = self.sessions.write().unwrap();
                        if sessions.remove(&session_id).is_some() {
                            drop(sessions);
                            tracing::info!("Reconcile: session {} (pid={}) no longer alive, removed", session_id, pid);
                            self.emit(AgentEvent::StateChange {
                                session_id,
                                state: SessionState::Idle,
                            });
                        }
                        continue;
                    }

                    let raw_status = info.get("status").and_then(|v| v.as_str());
                    let file_state = match raw_status {
                        Some("busy") => Some(SessionState::Busy),
                        Some("idle") => Some(SessionState::Idle),
                        other => {
                            tracing::warn!(
                                "Reconcile: session {} has unexpected status {:?}, skipping override",
                                session_id, other
                            );
                            None
                        }
                    };
                    let Some(file_state) = file_state else { continue };

                    let (cur_state, age) = {
                        let sessions = self.sessions.read().unwrap();
                        match sessions.get(&session_id) {
                            Some(conn) => (Some(conn.state.clone()), conn.last_event.elapsed()),
                            None => (None, std::time::Duration::ZERO),
                        }
                    };

                    let Some(cur_state) = cur_state else {
                        let cwd = info.get("cwd").and_then(|v| v.as_str()).map(|s| s.to_string());
                        tracing::info!("Reconcile: discovered new session {} (pid={}, status={:?})", session_id, pid, raw_status);
                        self.update_state_from(&session_id, file_state, cwd, "reconcile-discover");
                        continue;
                    };

                    // Cases where the CC session file is ahead of our hook-driven state:
                    //  - Busy -> Idle: CC finished without emitting a Stop hook.
                    //  - WaitingPermission -> Busy/Idle: the permission was resolved
                    //    outside our /permission hook (e.g. answered in CC's own
                    //    terminal prompt). We never get a hook to leave WaitingPermission,
                    //    so without this the popup coordinator keeps the popup on screen
                    //    until the 590s waiter timeout.
                    let went_idle = cur_state == SessionState::Busy && file_state == SessionState::Idle;
                    let left_waiting = cur_state == SessionState::WaitingPermission
                        && matches!(file_state, SessionState::Busy | SessionState::Idle);
                    if !went_idle && !left_waiting {
                        continue;
                    }
                    // Guard against racing a just-arrived hook / a lagging file (the
                    // file may still show the pre-permission "busy" right after we set
                    // WaitingPermission). Only adopt the file state once ours has been
                    // stable for the guard window; a genuinely-waiting session's file
                    // reads "waiting" and is skipped above, so this cannot dismiss a
                    // popup the user is still meant to answer.
                    if age.as_secs() < RECONCILE_HOOK_GUARD_SECS {
                        tracing::info!(
                            "Reconcile: session {} file says {:?} but last hook {:?} ago (< {}s guard), keeping {:?}",
                            session_id, file_state, age, RECONCILE_HOOK_GUARD_SECS, cur_state
                        );
                        continue;
                    }
                    tracing::info!(
                        "Reconcile: session {} {:?} -> {:?} from file (last hook {:?} ago)",
                        session_id, cur_state, file_state, age
                    );
                    let cwd = info.get("cwd").and_then(|v| v.as_str()).map(|s| s.to_string());
                    self.update_state_from(&session_id, file_state, cwd, "reconcile");
                }
            }
        }

        // Remove sessions whose files no longer exist
        for orphan_id in known_ids {
            let mut sessions = self.sessions.write().unwrap();
            if sessions.remove(&orphan_id).is_some() {
                drop(sessions);
                tracing::info!("Reconcile: session {} has no file, removed", orphan_id);
                self.emit(AgentEvent::StateChange {
                    session_id: orphan_id,
                    state: SessionState::Idle,
                });
            }
        }
    }

    fn emit(&self, event: AgentEvent) {
        let senders = self.event_senders.lock().unwrap();
        for tx in senders.iter() {
            let _ = tx.send(event.clone());
        }
    }

    fn find_pid_for_session(&self, session_id: &str) -> Result<i32, String> {
        let home = std::env::var("HOME").map_err(|e| format!("HOME not set: {}", e))?;
        let sessions_dir = std::path::Path::new(&home).join(".claude").join("sessions");
        let entries = std::fs::read_dir(&sessions_dir)
            .map_err(|e| format!("cannot read sessions dir: {}", e))?;
        for entry in entries.flatten() {
            if entry.path().extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            if let Ok(content) = std::fs::read_to_string(entry.path()) {
                if let Ok(info) = serde_json::from_str::<serde_json::Value>(&content) {
                    let sid = info.get("sessionId").and_then(|v| v.as_str()).unwrap_or_default();
                    if sid == session_id {
                        return info.get("pid").and_then(|v| v.as_i64()).map(|p| p as i32)
                            .ok_or_else(|| format!("no pid in session file for {}", session_id));
                    }
                }
            }
        }
        Err(format!("session file not found for {}", session_id))
    }
}

#[async_trait::async_trait]
impl AgentConnector for ClaudeCodeConnector {
    fn id(&self) -> &str {
        "claude-code"
    }

    async fn discover_sessions(&self) -> Vec<SessionInfo> {
        let sessions = self.sessions.read().unwrap();
        let result: Vec<SessionInfo> = sessions.values().map(|conn| SessionInfo {
            id: conn.session_id.clone(),
            agent: "claude-code".to_string(),
            state: conn.state.clone(),
            working_dir: conn.working_dir.clone(),
        }).collect();
        tracing::debug!("discover_sessions() returning {} sessions: {:?}", result.len(), result.iter().map(|s| &s.id).collect::<Vec<_>>());
        result
    }

    async fn inject_input(&self, session_id: &str, text: &str) -> Result<(), String> {
        info!("Inject to CC (app->agent): session={} text_len={}", session_id, text.len());
        {
            let sessions = self.sessions.read().unwrap();
            if !sessions.contains_key(session_id) {
                return Err(format!("session {} not found", session_id));
            }
        }

        let pid = self.find_pid_for_session(session_id)?;

        let tty_output = tokio::process::Command::new("ps")
            .args(["-p", &pid.to_string(), "-o", "tty="])
            .output()
            .await
            .map_err(|e| format!("ps failed: {}", e))?;
        let tty_name = String::from_utf8_lossy(&tty_output.stdout).trim().to_string();
        if tty_name.is_empty() || tty_name == "??" {
            return Err(format!("cannot find TTY for session {} (pid={})", session_id, pid));
        }
        let tty_path = format!("/dev/{}", tty_name);

        // Detect terminal emulator by walking up the process tree
        let terminal_app = detect_terminal_app(pid).await;
        info!("Session {} pid={} tty={} terminal={}", session_id, pid, tty_path, terminal_app);

        let escaped_text = text.replace('\\', "\\\\").replace('"', "\\\"");

        // Special control characters: send keystroke instead of clipboard paste
        let is_control = text == "\x1b" || text == "\x03" || text == "\r";

        let script = if is_control {
            let keystroke_cmd = match text {
                "\x1b" => "key code 53", // Escape
                "\x03" => "keystroke \"c\" using control down", // Ctrl+C
                "\r" => "key code 36", // Return
                _ => unreachable!(),
            };
            match terminal_app.as_str() {
                "iTerm2" | "iTerm" => format!(
                    r#"
with timeout of 30 seconds
    tell application "iTerm2"
        repeat with w in windows
            repeat with t in tabs of w
                repeat with s in sessions of t
                    if tty of s is "{tty}" then
                        tell t to select
                        tell w
                            set index to 1
                        end tell
                        activate
                        delay 0.1
                        tell application "System Events"
                            tell process "iTerm2"
                                {keystroke}
                            end tell
                        end tell
                        return "ok"
                    end if
                end repeat
            end repeat
        end repeat
    end tell
end timeout
return "not_found"
"#,
                    tty = tty_path,
                    keystroke = keystroke_cmd,
                ),
                _ => format!(
                    r#"
with timeout of 30 seconds
    tell application "Terminal"
        repeat with w in windows
            repeat with t in tabs of w
                if tty of t is "{tty}" then
                    set selected of t to true
                    set frontmost of w to true
                    activate
                    delay 0.1
                    tell application "System Events"
                        tell process "Terminal"
                            {keystroke}
                        end tell
                    end tell
                    return "ok"
                end if
            end repeat
        end repeat
    end tell
end timeout
return "not_found"
"#,
                    tty = tty_path,
                    keystroke = keystroke_cmd,
                ),
            }
        } else {
        match terminal_app.as_str() {
            "iTerm2" | "iTerm" => {
                format!(
                    r#"
tell application "iTerm2"
    repeat with w in windows
        repeat with t in tabs of w
            repeat with s in sessions of t
                if tty of s is "{tty}" then
                    tell s to write text "{text}"
                    return "ok"
                end if
            end repeat
        end repeat
    end repeat
end tell
return "not_found"
"#,
                    tty = tty_path,
                    text = escaped_text,
                )
            }
            _ => {
                format!(
                    r#"
set the clipboard to "{text}"
tell application "Terminal"
    set found to false
    repeat with w in windows
        repeat with t in tabs of w
            if tty of t is "{tty}" then
                set selected of t to true
                set frontmost of w to true
                set found to true
                exit repeat
            end if
        end repeat
        if found then exit repeat
    end repeat
    if not found then return "not_found"
    activate
end tell
delay 0.15
tell application "System Events"
    tell process "Terminal"
        keystroke "v" using command down
        delay 0.1
        keystroke return
    end tell
end tell
return "ok"
"#,
                    tty = tty_path,
                    text = escaped_text,
                )
            }
        }
        };

        let output = tokio::process::Command::new("osascript")
            .args(["-e", &script])
            .output()
            .await
            .map_err(|e| format!("osascript failed: {}", e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("AppleScript inject failed ({}): {}", terminal_app, stderr.trim()));
        }

        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if stdout == "not_found" {
            return Err(format!("TTY {} not found in {} tabs", tty_path, terminal_app));
        }

        info!("Injected input to session {} via {} on {}", session_id, terminal_app, tty_path);
        Ok(())
    }

    fn subscribe(&self, sender: EventSender) {
        self.event_senders.lock().unwrap().push(sender);
    }

    fn rediscover(&self) {
        self.discover_existing_sessions();
    }
}

fn process_alive(pid: i32) -> bool {
    #[cfg(unix)]
    { unsafe { libc::kill(pid, 0) == 0 } }
    #[cfg(windows)]
    {
        use std::process::Command;
        Command::new("tasklist")
            .args(["/FI", &format!("PID eq {}", pid), "/NH"])
            .output()
            .map(|o| !String::from_utf8_lossy(&o.stdout).contains("No tasks"))
            .unwrap_or(false)
    }
}

/// Walk up the process tree from PID to find the terminal emulator app name.
async fn detect_terminal_app(pid: i32) -> String {
    let mut current = pid;
    for _ in 0..10 {
        let output = tokio::process::Command::new("ps")
            .args(["-p", &current.to_string(), "-o", "ppid=,comm="])
            .output()
            .await;
        let output = match output {
            Ok(o) => o,
            Err(_) => break,
        };
        let line = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let parts: Vec<&str> = line.splitn(2, char::is_whitespace).collect();
        if parts.len() < 2 { break; }
        let ppid: i32 = match parts[0].trim().parse() {
            Ok(p) => p,
            Err(_) => break,
        };
        let comm = parts[1].trim();
        if comm.contains("iTerm") { return "iTerm2".to_string(); }
        if comm.contains("Terminal") { return "Terminal".to_string(); }
        if comm.contains("Alacritty") { return "Alacritty".to_string(); }
        if comm.contains("kitty") { return "kitty".to_string(); }
        if ppid <= 1 { break; }
        current = ppid;
    }
    "Terminal".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::mpsc;

    #[test]
    fn hook_creates_session_with_cwd() {
        let connector = ClaudeCodeConnector::new("/tmp");
        connector.handle_hook_event(HookEvent::PromptSubmit {
            session_id: "s1".into(),
            prompt: None,
            cwd: Some("/home/user/project".into()),
        });

        let rt = tokio::runtime::Runtime::new().unwrap();
        let sessions = rt.block_on(connector.discover_sessions());
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].id, "s1");
        assert_eq!(sessions[0].state, SessionState::Busy);
        assert_eq!(sessions[0].working_dir, Some("/home/user/project".to_string()));
    }

    #[test]
    fn event_emitted_on_state_change() {
        let connector = ClaudeCodeConnector::new("/tmp");
        let (tx, mut rx) = mpsc::unbounded_channel();
        connector.subscribe(tx);

        connector.handle_hook_event(HookEvent::PromptSubmit { session_id: "s1".into(), prompt: None, cwd: None });

        let event = rx.try_recv().unwrap();
        match event {
            AgentEvent::StateChange { session_id, state } => {
                assert_eq!(session_id, "s1");
                assert_eq!(state, SessionState::Busy);
            }
            _ => panic!("wrong event"),
        }
    }
}
