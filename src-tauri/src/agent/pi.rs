use crate::agent::{AgentConnector, AgentEvent, EventSender, SessionInfo, SessionState};
use crate::hook::server::HookEvent;
use std::collections::HashMap;
use std::sync::{Mutex, RwLock};
use std::time::Instant;
use tracing::info;

struct PiSession {
    session_id: String,
    state: SessionState,
    working_dir: Option<String>,
    pid: Option<i32>,
    inject_port: Option<u16>,
    last_event: Instant,
}

pub struct PiConnector {
    sessions: RwLock<HashMap<String, PiSession>>,
    event_senders: Mutex<Vec<EventSender>>,
}

impl PiConnector {
    pub fn new() -> Self {
        Self {
            sessions: RwLock::new(HashMap::new()),
            event_senders: Mutex::new(vec![]),
        }
    }

    pub fn handle_hook_event(&self, event: HookEvent) {
        match event {
            HookEvent::PiSessionStart { session_id, cwd, pid, inject_port } => {
                self.upsert(&session_id, SessionState::Idle, cwd, pid, inject_port);
            }
            HookEvent::PiSessionEnd { session_id } => {
                self.sessions.write().unwrap().remove(&session_id);
                self.emit(AgentEvent::StateChange { session_id, state: SessionState::Idle });
            }
            HookEvent::PiInput { session_id, cwd, .. } => {
                self.upsert(&session_id, SessionState::Busy, cwd, None, None);
            }
            HookEvent::PiPreToolUse { session_id, cwd, .. } => {
                self.upsert(&session_id, SessionState::Busy, cwd, None, None);
            }
            HookEvent::PiPostToolUse { session_id, cwd, .. } => {
                self.upsert(&session_id, SessionState::Busy, cwd, None, None);
            }
            HookEvent::PiPermissionRequest { session_id, tool, input, cwd, .. } => {
                self.upsert(&session_id, SessionState::WaitingPermission, cwd, None, None);
                self.emit(AgentEvent::PermissionRequest { session_id, tool, input });
            }
            HookEvent::PiStop { session_id, cwd } => {
                self.upsert(&session_id, SessionState::Idle, cwd, None, None);
            }
            HookEvent::PiAgentStart { session_id, cwd } => {
                self.upsert(&session_id, SessionState::Busy, cwd, None, None);
            }
            HookEvent::PiPreCompact { session_id, cwd } => {
                self.upsert(&session_id, SessionState::Busy, cwd, None, None);
            }
            HookEvent::PiPostCompact { session_id, cwd } => {
                self.upsert(&session_id, SessionState::Busy, cwd, None, None);
            }
            _ => {}
        }
    }

    fn upsert(&self, session_id: &str, state: SessionState, cwd: Option<String>, pid: Option<i32>, inject_port: Option<u16>) {
        let mut sessions = self.sessions.write().unwrap();
        let prev = sessions.get(session_id).map(|s| s.state.clone());
        let entry = sessions.entry(session_id.to_string()).or_insert_with(|| PiSession {
            session_id: session_id.to_string(),
            state: state.clone(),
            working_dir: cwd.clone(),
            pid,
            inject_port,
            last_event: Instant::now(),
        });
        if cwd.is_some() {
            entry.working_dir = cwd;
        }
        if pid.is_some() {
            entry.pid = pid;
        }
        if inject_port.is_some() {
            entry.inject_port = inject_port;
        }
        entry.last_event = Instant::now();
        entry.state = state.clone();
        drop(sessions);

        if prev.as_ref() != Some(&state) {
            self.emit(AgentEvent::StateChange { session_id: session_id.to_string(), state });
        }
    }

    pub fn discover_existing_sessions(&self) {
        let home = match std::env::var("HOME") {
            Ok(h) => h,
            Err(_) => return,
        };
        let registry_dir = std::path::Path::new(&home).join(".cc-remote").join("pi-sessions");
        let entries = match std::fs::read_dir(&registry_dir) {
            Ok(e) => e,
            Err(_) => return,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let content = match std::fs::read_to_string(&path) {
                Ok(c) => c,
                Err(_) => continue,
            };
            let info: serde_json::Value = match serde_json::from_str(&content) {
                Ok(v) => v,
                Err(_) => { let _ = std::fs::remove_file(&path); continue; }
            };
            let session_id = info.get("session_id").and_then(|v| v.as_str()).unwrap_or_default().to_string();
            let pid = info.get("pid").and_then(|v| v.as_i64()).map(|p| p as i32);
            let alive = pid.map(|p| p > 0 && process_alive(p)).unwrap_or(false);
            if !alive || session_id.is_empty() {
                let _ = std::fs::remove_file(&path);
                continue;
            }
            let cwd = info.get("cwd").and_then(|v| v.as_str()).map(|s| s.to_string());
            let inject_port = info.get("inject_port").and_then(|v| v.as_u64()).map(|p| p as u16);
            info!("Discovered existing Pi session: {} (pid={:?}, cwd={:?})", session_id, pid, cwd);
            self.upsert(&session_id, SessionState::Idle, cwd, pid, inject_port);
        }
    }

    pub fn reap_stale(&self) {
        let stale: Vec<String> = {
            let sessions = self.sessions.read().unwrap();
            sessions.iter()
                .filter(|(_, s)| {
                    s.last_event.elapsed().as_secs() > 300
                        || s.pid.map(|p| !process_alive(p)).unwrap_or(false)
                })
                .map(|(id, _)| id.clone())
                .collect()
        };
        for id in stale {
            self.sessions.write().unwrap().remove(&id);
            info!("Pi session {} stale, removed", id);
            self.emit(AgentEvent::StateChange { session_id: id, state: SessionState::Idle });
        }
    }

    fn emit(&self, event: AgentEvent) {
        let senders = self.event_senders.lock().unwrap();
        for tx in senders.iter() {
            let _ = tx.send(event.clone());
        }
    }
}

#[async_trait::async_trait]
impl AgentConnector for PiConnector {
    fn id(&self) -> &str {
        "pi"
    }

    async fn discover_sessions(&self) -> Vec<SessionInfo> {
        self.sessions.read().unwrap().values().map(|s| SessionInfo {
            id: s.session_id.clone(),
            agent: "pi".to_string(),
            state: s.state.clone(),
            working_dir: s.working_dir.clone(),
        }).collect()
    }

    async fn inject_input(&self, session_id: &str, text: &str) -> Result<(), String> {
        let (port, pid) = {
            let sessions = self.sessions.read().unwrap();
            let s = sessions.get(session_id)
                .ok_or_else(|| format!("pi session {} not found", session_id))?;
            (s.inject_port, s.pid)
        };
        if let Some(p) = port {
            match inject_via_http(p, text).await {
                Ok(()) => return Ok(()),
                Err(e) => tracing::warn!("pi http inject failed (port={}): {}, falling back to TTY", p, e),
            }
        }
        let pid = pid.ok_or_else(|| format!("pi session {} has no pid for fallback", session_id))?;
        inject_via_tty(pid, text).await
    }

    fn subscribe(&self, sender: EventSender) {
        self.event_senders.lock().unwrap().push(sender);
    }

    fn rediscover(&self) {
        self.discover_existing_sessions();
    }
}

fn process_alive(pid: i32) -> bool {
    unsafe { libc::kill(pid, 0) == 0 }
}

async fn inject_via_http(port: u16, text: &str) -> Result<(), String> {
    let url = format!("http://127.0.0.1:{}/inject", port);
    let body = serde_json::json!({ "text": text }).to_string();
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| format!("client build: {}", e))?;
    let res = client.post(&url)
        .header("content-type", "application/json")
        .body(body)
        .send().await
        .map_err(|e| format!("post failed: {}", e))?;
    if !res.status().is_success() {
        return Err(format!("inject status {}", res.status()));
    }
    info!("Injected to pi via http port={}", port);
    Ok(())
}

async fn inject_via_tty(pid: i32, text: &str) -> Result<(), String> {
    let tty_output = tokio::process::Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "tty="])
        .output()
        .await
        .map_err(|e| format!("ps failed: {}", e))?;
    let tty_name = String::from_utf8_lossy(&tty_output.stdout).trim().to_string();
    if tty_name.is_empty() || tty_name == "??" {
        return Err(format!("cannot find TTY for pi (pid={})", pid));
    }
    let tty_path = format!("/dev/{}", tty_name);

    let terminal_app = detect_terminal_app(pid).await;
    let escaped_text = text.replace('\\', "\\\\").replace('"', "\\\"");

    let script = match terminal_app.as_str() {
        "iTerm2" | "iTerm" => format!(
            r#"
set the clipboard to "{text}"
tell application "iTerm2"
    repeat with w in windows
        repeat with t in tabs of w
            repeat with s in sessions of t
                if tty of s is "{tty}" then
                    tell t to select
                    tell w
                        set index to 1
                    end tell
                end if
            end repeat
        end repeat
    end repeat
    activate
end tell
delay 0.15
tell application "System Events"
    tell process "iTerm2"
        keystroke "v" using command down
        delay 0.1
        keystroke return
    end tell
end tell
return "ok"
"#,
            tty = tty_path, text = escaped_text,
        ),
        _ => format!(
            r#"
set the clipboard to "{text}"
tell application "Terminal"
    repeat with w in windows
        repeat with t in tabs of w
            if tty of t is "{tty}" then
                set selected of t to true
                set frontmost of w to true
            end if
        end repeat
    end repeat
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
            tty = tty_path, text = escaped_text,
        ),
    };

    let output = tokio::process::Command::new("osascript")
        .args(["-e", &script])
        .output()
        .await
        .map_err(|e| format!("osascript failed: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("AppleScript inject failed: {}", stderr.trim()));
    }

    info!("Injected input to pi session pid={} via {}", pid, terminal_app);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::mpsc;

    #[test]
    fn pi_stop_transitions_busy_to_idle() {
        let connector = PiConnector::new();
        let (tx, mut rx) = mpsc::unbounded_channel();
        connector.subscribe(tx);

        // Start session and make it busy
        connector.handle_hook_event(HookEvent::PiSessionStart {
            session_id: "s1".into(), cwd: Some("/tmp".into()), pid: Some(1234), inject_port: None,
        });
        connector.handle_hook_event(HookEvent::PiInput {
            session_id: "s1".into(), text: "hello".into(), cwd: None,
        });

        // Drain events so far
        while rx.try_recv().is_ok() {}

        // Send PiStop — should transition to Idle
        connector.handle_hook_event(HookEvent::PiStop {
            session_id: "s1".into(), cwd: None,
        });

        let event = rx.try_recv().expect("should emit StateChange to Idle");
        match event {
            AgentEvent::StateChange { session_id, state } => {
                assert_eq!(session_id, "s1");
                assert_eq!(state, SessionState::Idle);
            }
            _ => panic!("expected StateChange, got {:?}", event),
        }
    }
}

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
