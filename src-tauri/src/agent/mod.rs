use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SessionState {
    Idle,
    Busy,
    WaitingPermission,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    pub id: String,
    pub agent: String,
    pub state: SessionState,
    #[serde(rename = "workingDir")]
    pub working_dir: Option<String>,
}

#[derive(Debug, Clone)]
pub enum AgentEvent {
    StateChange { session_id: String, state: SessionState },
    Output { session_id: String, text: String },
    PermissionRequest { session_id: String, tool: String, input: String },
}

pub type EventSender = mpsc::UnboundedSender<AgentEvent>;

#[async_trait::async_trait]
pub trait AgentConnector: Send + Sync {
    fn id(&self) -> &str;
    async fn discover_sessions(&self) -> Vec<SessionInfo>;
    async fn inject_input(&self, session_id: &str, text: &str) -> Result<(), String>;
    fn subscribe(&self, sender: EventSender);
    fn rediscover(&self) {}
}

pub mod claude_code;
pub mod pi;
