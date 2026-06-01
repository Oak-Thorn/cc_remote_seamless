pub mod router;
pub mod slash;
pub mod store;

use crate::agent::{AgentConnector, AgentEvent, SessionInfo};
use crate::hook::server::PermissionWaiters;
use crate::platform::IMMessage;
use router::BindingStore;
use store::MessageStore;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tracing::{info, warn};

static mut APP_START_TIME: Option<Instant> = None;

pub fn init_start_time() {
    unsafe { APP_START_TIME = Some(Instant::now()); }
}

fn should_play_sound() -> bool {
    unsafe {
        APP_START_TIME.map(|t| t.elapsed().as_secs() >= 5).unwrap_or(true)
    }
}

pub struct Engine {
    agents: HashMap<String, Arc<dyn AgentConnector>>,
    platforms: HashMap<String, Arc<dyn crate::platform::IMPlatform>>,
    agent_platforms: HashMap<String, Vec<String>>,
    platform_chat_ids: HashMap<String, String>,
    pub bindings: Arc<BindingStore>,
    pub messages: Arc<MessageStore>,
    pub permission_waiters: Option<PermissionWaiters>,
    pub last_binding_change: std::sync::Mutex<Option<String>>,
    idle_sound: String,
    permission_sound: String,
    resource_dir: Option<String>,
}

impl Engine {
    pub fn new(store_path: &str, idle_sound: String, permission_sound: String, resource_dir: Option<String>) -> Result<Self, String> {
        Ok(Self {
            agents: HashMap::new(),
            platforms: HashMap::new(),
            agent_platforms: HashMap::new(),
            platform_chat_ids: HashMap::new(),
            bindings: Arc::new(BindingStore::new()),
            messages: Arc::new(MessageStore::open(store_path)?),
            permission_waiters: None,
            last_binding_change: std::sync::Mutex::new(None),
            idle_sound,
            permission_sound,
            resource_dir,
        })
    }

    pub fn register_agent(&mut self, agent: Arc<dyn AgentConnector>) {
        let id = agent.id().to_string();
        self.agents.insert(id, agent);
    }

    pub fn register_platform(&mut self, platform: Arc<dyn crate::platform::IMPlatform>) {
        let id = platform.id().to_string();
        self.platforms.insert(id, platform);
    }

    pub fn set_agent_platforms(&mut self, mapping: HashMap<String, Vec<String>>) {
        self.agent_platforms = mapping;
    }

    pub fn set_idle_sound(&mut self, name: String) {
        self.idle_sound = name;
    }

    pub fn set_permission_sound(&mut self, name: String) {
        self.permission_sound = name;
    }

    pub fn set_platform_chat_id(&mut self, platform_id: &str, chat_id: &str) {
        self.platform_chat_ids.insert(platform_id.to_string(), chat_id.to_string());
    }

    fn platforms_for_agent(&self, agent_id: &str) -> Vec<&str> {
        self.agent_platforms.get(agent_id)
            .map(|ids| ids.iter().map(|s| s.as_str()).collect())
            .unwrap_or_else(|| self.platforms.keys().map(|s| s.as_str()).collect())
    }

    pub fn agents_iter(&self) -> impl Iterator<Item = &Arc<dyn AgentConnector>> {
        self.agents.values()
    }

    pub async fn forward_to_platforms(&self, agent_id: &str, session_id: &str, text: &str) {
        let platform_ids = self.platforms_for_agent(agent_id);
        for pid in platform_ids {
            if let Some(platform) = self.platforms.get(pid) {
                if let Some(chat_id) = self.platform_chat_ids.get(pid) {
                    let binding = self.bindings.get(chat_id);
                    if binding.as_ref().map(|b| b.session_id.as_str()) == Some(session_id) {
                        tracing::info!("Forwarding to platform={} chat_id={}", pid, chat_id);
                        if let Err(e) = platform.send_text(chat_id, text).await {
                            warn!("Forward to {} failed: {}", pid, e);
                        }
                    }
                }
            }
        }
    }

    pub async fn forward_card_to_platforms(&self, agent_id: &str, session_id: &str, card: serde_json::Value) {
        let platform_ids = self.platforms_for_agent(agent_id);
        for pid in platform_ids {
            if let Some(platform) = self.platforms.get(pid) {
                if let Some(chat_id) = self.platform_chat_ids.get(pid) {
                    let binding = self.bindings.get(chat_id);
                    if binding.as_ref().map(|b| b.session_id.as_str()) == Some(session_id) {
                        if let Err(e) = platform.send_card(chat_id, card.clone()).await {
                            warn!("Forward card to {} failed: {}", pid, e);
                        }
                    }
                }
            }
        }
    }

    pub async fn get_sessions(&self) -> Vec<SessionInfo> {
        let mut all = vec![];
        for agent in self.agents.values() {
            all.extend(agent.discover_sessions().await);
        }
        all
    }

    pub async fn handle_im_message(&self, msg: IMMessage) {
        let timestamp = msg.timestamp as i64;

        // Slash command routing
        if msg.text.starts_with('/') {
            let waiters = self.permission_waiters.clone()
                .unwrap_or_else(|| Arc::new(tokio::sync::Mutex::new(HashMap::new())));
            let result = slash::execute(&msg.text, &msg.chat_id, &self.bindings, &self.agents, &waiters, &self.messages).await;
            match result {
                slash::SlashResult::Reply(text) => {
                    if let Some(platform) = self.platforms.get(&msg.platform) {
                        let _ = platform.send_text(&msg.chat_id, &text).await;
                    }
                    return;
                }
                slash::SlashResult::BindingChanged { reply, session_id } => {
                    if let Some(platform) = self.platforms.get(&msg.platform) {
                        let _ = platform.send_text(&msg.chat_id, &reply).await;
                    }
                    self.last_binding_change.lock().unwrap().replace(session_id);
                    return;
                }
                slash::SlashResult::Inject(text) => {
                    let binding = match self.bindings.get(&msg.chat_id) {
                        Some(b) => b,
                        None => {
                            warn!("Inject: no binding for chat_id={}", msg.chat_id);
                            if let Some(platform) = self.platforms.get(&msg.platform) {
                                let _ = platform.send_text(&msg.chat_id, "No session bound. Use /switch <id> or /radar first.").await;
                            }
                            return;
                        }
                    };
                    self.messages.insert(&binding.session_id, &msg.platform, &text, timestamp).ok();
                    match self.agents.get(&binding.agent_id) {
                        Some(agent) => {
                            info!("Injecting to agent={} session={}", binding.agent_id, binding.session_id);
                            if let Err(e) = agent.inject_input(&binding.session_id, &text).await {
                                warn!("Inject failed: {}", e);
                                if let Some(platform) = self.platforms.get(&msg.platform) {
                                    let _ = platform.send_text(&msg.chat_id, &format!("Inject failed: {}", e)).await;
                                }
                            }
                        }
                        None => {
                            warn!("Inject: agent '{}' not found in registry", binding.agent_id);
                            if let Some(platform) = self.platforms.get(&msg.platform) {
                                let _ = platform.send_text(&msg.chat_id, &format!("Agent '{}' not found", binding.agent_id)).await;
                            }
                        }
                    }
                    return;
                }
                slash::SlashResult::Noop => return,
            }
        }

        // Non-command text: ignore and hint user to use /p
        if let Some(platform) = self.platforms.get(&msg.platform) {
            let _ = platform.send_text(&msg.chat_id, "Use /p <text> to send prompt, or /help for commands").await;
        }
    }

    pub async fn handle_agent_event(&self, event: AgentEvent) {
        info!("Received agent event: {:?}", event);
        match event {
            AgentEvent::Output { ref session_id, ref text } => {
                let now = chrono::Utc::now().timestamp();
                self.messages.insert(session_id, "agent", text, now).ok();

                let platform_ids = self.platforms_for_agent("claude-code");
                for pid in &platform_ids {
                    if let Some(platform) = self.platforms.get(*pid) {
                        if let Some(chat_id) = self.platform_chat_ids.get(*pid) {
                            let binding = self.bindings.get(chat_id);
                            if binding.as_ref().map(|b| b.session_id.as_str()) != Some(session_id.as_str()) {
                                continue;
                            }
                            self.bindings.store_last_output(chat_id, text);
                            if self.bindings.is_muted(chat_id) {
                                info!("Muted, skipping forward to {}", pid);
                                continue;
                            }
                            if let Err(e) = platform.send_text(chat_id, text).await {
                                warn!("Send to {} failed: {}", pid, e);
                            }
                        }
                    }
                }
            }
            AgentEvent::StateChange { ref session_id, ref state } => {
                info!("Session {} state -> {:?}", session_id, state);
                match state {
                    crate::agent::SessionState::Idle => {
                        if should_play_sound() { play_sound(&self.idle_sound, self.resource_dir.as_deref()); }
                    }
                    crate::agent::SessionState::WaitingPermission => {
                        if should_play_sound() { play_sound(&self.permission_sound, self.resource_dir.as_deref()); }
                    }
                    _ => {}
                }
            }
            AgentEvent::PermissionRequest { ref session_id, ref tool, .. } => {
                info!("Permission request: session={} tool={}", session_id, tool);
            }
        }
    }
}

fn play_sound(name: &str, resource_dir: Option<&str>) {
    let path = resolve_engine_sound_path(name, resource_dir);
    std::thread::spawn(move || {
        #[cfg(target_os = "macos")]
        {
            let _ = std::process::Command::new("afplay").arg(&path).spawn();
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = path; // suppress unused warning
        }
    });
}

fn resolve_engine_sound_path(name: &str, resource_dir: Option<&str>) -> String {
    let sound_exts = ["aiff", "mp3", "wav"];

    // User custom sounds (highest priority)
    if let Some(base) = directories::BaseDirs::new() {
        let custom_dir = base.home_dir().join(".cc-remote").join("sounds");
        for ext in &sound_exts {
            let path = custom_dir.join(format!("{}.{}", name, ext));
            if path.exists() {
                return path.to_string_lossy().to_string();
            }
        }
    }

    // Tauri resource directory (bundled sounds)
    if let Some(res_dir) = resource_dir {
        let sounds_dir = std::path::Path::new(res_dir).join("sounds");
        for ext in &sound_exts {
            let path = sounds_dir.join(format!("{}.{}", name, ext));
            if path.exists() {
                return path.to_string_lossy().to_string();
            }
        }
    }

    // Try relative to the project root (dev mode)
    if let Ok(cargo_dir) = std::env::var("CARGO_MANIFEST_DIR") {
        let sounds_dir = std::path::Path::new(&cargo_dir)
            .join("resources")
            .join("sounds");
        for ext in &sound_exts {
            let path = sounds_dir.join(format!("{}.{}", name, ext));
            if path.exists() {
                return path.to_string_lossy().to_string();
            }
        }
    }

    // Fallback: macOS system sounds (only .aiff)
    format!("/System/Library/Sounds/{}.aiff", name)
}
