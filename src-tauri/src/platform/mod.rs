use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IMMessage {
    pub chat_id: String,
    pub text: String,
    pub sender: String,
    pub platform: String,
    pub timestamp: u64,
}

pub type MessageSender = mpsc::UnboundedSender<IMMessage>;

#[async_trait::async_trait]
pub trait IMPlatform: Send + Sync {
    fn id(&self) -> &str;
    async fn connect(&self) -> Result<(), String>;
    async fn send_text(&self, chat_id: &str, text: &str) -> Result<(), String>;
    async fn send_card(&self, _chat_id: &str, _card: serde_json::Value) -> Result<(), String> {
        Ok(())
    }
    fn subscribe(&self, sender: MessageSender);
    async fn disconnect(&self);
}

pub mod feishu;
pub mod telegram;
