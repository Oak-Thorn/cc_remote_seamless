use crate::platform::{IMMessage, IMPlatform, MessageSender};
use serde::{Deserialize, Serialize};
use std::process::Stdio;
use std::sync::Mutex;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc;
use tracing::{info, error};

#[derive(Debug, Serialize)]
struct SidecarCommand {
    #[serde(rename = "type")]
    cmd_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    app_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    app_secret: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    chat_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    card: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct SidecarEvent {
    #[serde(rename = "type")]
    event_type: String,
    chat_id: Option<String>,
    text: Option<String>,
    sender: Option<String>,
    #[allow(dead_code)]
    message_id: Option<String>,
    #[allow(dead_code)]
    reason: Option<String>,
    message: Option<String>,
}

pub struct FeishuPlatform {
    id: String,
    app_id: String,
    app_secret: String,
    sidecar_path: String,
    stdin_tx: Mutex<Option<mpsc::UnboundedSender<String>>>,
    message_senders: Mutex<Vec<MessageSender>>,
}

impl FeishuPlatform {
    pub fn new(id: &str, app_id: &str, app_secret: &str, sidecar_path: &str) -> Self {
        Self {
            id: id.to_string(),
            app_id: app_id.to_string(),
            app_secret: app_secret.to_string(),
            sidecar_path: sidecar_path.to_string(),
            stdin_tx: Mutex::new(None),
            message_senders: Mutex::new(vec![]),
        }
    }

    fn send_command(&self, cmd: SidecarCommand) -> Result<(), String> {
        let guard = self.stdin_tx.lock().unwrap();
        let tx = guard.as_ref().ok_or("sidecar not running")?;
        let mut line = serde_json::to_string(&cmd).map_err(|e| e.to_string())?;
        line.push('\n');
        tx.send(line).map_err(|e| e.to_string())
    }

    pub fn send_card(&self, chat_id: &str, card: serde_json::Value) -> Result<(), String> {
        info!("Feishu send_card: chat_id={}", chat_id);
        self.send_command(SidecarCommand {
            cmd_type: "send_card".into(),
            app_id: None,
            app_secret: None,
            chat_id: Some(chat_id.to_string()),
            text: None,
            card: Some(card),
        })
    }
}

#[async_trait::async_trait]
impl IMPlatform for FeishuPlatform {
    fn id(&self) -> &str {
        &self.id
    }

    async fn connect(&self) -> Result<(), String> {
        let mut child = Command::new(&self.sidecar_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("spawn sidecar failed: {}", e))?;

        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();

        let (stdin_tx, mut stdin_rx) = mpsc::unbounded_channel::<String>();

        tokio::spawn(async move {
            let mut stdin = stdin;
            while let Some(line) = stdin_rx.recv().await {
                if stdin.write_all(line.as_bytes()).await.is_err() {
                    break;
                }
            }
        });

        *self.stdin_tx.lock().unwrap() = Some(stdin_tx);

        let senders = self.message_senders.lock().unwrap().clone();
        let platform_id = self.id.clone();
        tokio::spawn(async move {
            let reader = BufReader::new(stdout);
            let mut lines = reader.lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if let Ok(event) = serde_json::from_str::<SidecarEvent>(&line) {
                    match event.event_type.as_str() {
                        "connected" => info!("Feishu sidecar connected"),
                        "message_received" => {
                            let msg = IMMessage {
                                chat_id: event.chat_id.unwrap_or_default(),
                                text: event.text.unwrap_or_default(),
                                sender: event.sender.unwrap_or_default(),
                                platform: platform_id.clone(),
                                timestamp: std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .unwrap()
                                    .as_secs(),
                            };
                            for tx in &senders {
                                let _ = tx.send(msg.clone());
                            }
                        }
                        "error" => {
                            error!("Feishu sidecar error: {}", event.message.unwrap_or_default());
                        }
                        _ => {}
                    }
                }
            }
        });

        self.send_command(SidecarCommand {
            cmd_type: "connect".into(),
            app_id: Some(self.app_id.clone()),
            app_secret: Some(self.app_secret.clone()),
            chat_id: None,
            text: None,
            card: None,
        })?;

        Ok(())
    }

    async fn send_text(&self, chat_id: &str, text: &str) -> Result<(), String> {
        info!("Feishu send_text: chat_id={} text_len={}", chat_id, text.len());
        let result = self.send_command(SidecarCommand {
            cmd_type: "send_text".into(),
            app_id: None,
            app_secret: None,
            chat_id: Some(chat_id.to_string()),
            text: Some(text.to_string()),
            card: None,
        });
        match &result {
            Ok(()) => info!("Feishu send_text command queued successfully"),
            Err(e) => error!("Feishu send_text command failed: {}", e),
        }
        result
    }

    async fn send_card(&self, chat_id: &str, card: serde_json::Value) -> Result<(), String> {
        self.send_card(chat_id, card)
    }

    fn subscribe(&self, sender: MessageSender) {
        self.message_senders.lock().unwrap().push(sender);
    }

    async fn disconnect(&self) {
        let _ = self.send_command(SidecarCommand {
            cmd_type: "disconnect".into(),
            app_id: None,
            app_secret: None,
            chat_id: None,
            text: None,
            card: None,
        });
        *self.stdin_tx.lock().unwrap() = None;
    }
}
