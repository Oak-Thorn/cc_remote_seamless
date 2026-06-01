use crate::platform::{IMMessage, IMPlatform, MessageSender};
use serde::Deserialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use tracing::{info, warn};

pub struct TelegramPlatform {
    id: String,
    bot_token: String,
    chat_id: String,
    message_senders: Mutex<Vec<MessageSender>>,
    running: AtomicBool,
}

#[derive(Debug, Deserialize)]
struct TelegramResponse<T> {
    ok: bool,
    result: Option<T>,
}

#[derive(Debug, Deserialize)]
struct TelegramUpdate {
    update_id: i64,
    message: Option<TelegramMessageObj>,
}

#[derive(Debug, Deserialize)]
struct TelegramMessageObj {
    chat: TelegramChat,
    text: Option<String>,
    from: Option<TelegramUser>,
}

#[derive(Debug, Deserialize)]
struct TelegramChat {
    id: i64,
}

#[derive(Debug, Deserialize)]
struct TelegramUser {
    username: Option<String>,
    first_name: Option<String>,
}

impl TelegramPlatform {
    pub fn new(id: &str, bot_token: &str, chat_id: &str) -> Self {
        Self {
            id: id.to_string(),
            bot_token: bot_token.to_string(),
            chat_id: chat_id.to_string(),
            message_senders: Mutex::new(vec![]),
            running: AtomicBool::new(false),
        }
    }

    fn api_url(&self, method: &str) -> String {
        format!("https://api.telegram.org/bot{}/{}", self.bot_token, method)
    }
}

#[async_trait::async_trait]
impl IMPlatform for TelegramPlatform {
    fn id(&self) -> &str {
        &self.id
    }

    async fn connect(&self) -> Result<(), String> {
        self.running.store(true, Ordering::SeqCst);

        let bot_token = self.bot_token.clone();
        let allowed_chat_id = self.chat_id.clone();
        let senders = self.message_senders.lock().unwrap().clone();

        tokio::spawn(async move {
            let client = reqwest::Client::new();
            let mut offset: i64 = 0;

            loop {
                let url = format!(
                    "https://api.telegram.org/bot{}/getUpdates?offset={}&timeout=30",
                    bot_token, offset
                );
                let resp = match client.get(&url).send().await {
                    Ok(r) => r,
                    Err(e) => {
                        warn!("Telegram poll error: {}", e);
                        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                        continue;
                    }
                };

                let body = match resp.json::<TelegramResponse<Vec<TelegramUpdate>>>().await {
                    Ok(b) => b,
                    Err(e) => {
                        warn!("Telegram parse error: {}", e);
                        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                        continue;
                    }
                };

                if !body.ok {
                    warn!("Telegram API returned ok=false");
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    continue;
                }

                if let Some(updates) = body.result {
                    for update in updates {
                        offset = update.update_id + 1;
                        if let Some(message) = update.message {
                            let chat_id_str = message.chat.id.to_string();
                            if chat_id_str != allowed_chat_id {
                                continue;
                            }
                            if let Some(text) = message.text {
                                let sender = message.from
                                    .map(|u| u.username.unwrap_or(u.first_name.unwrap_or_default()))
                                    .unwrap_or_default();
                                let msg = IMMessage {
                                    chat_id: chat_id_str,
                                    text,
                                    sender,
                                    platform: "telegram".to_string(),
                                    timestamp: std::time::SystemTime::now()
                                        .duration_since(std::time::UNIX_EPOCH)
                                        .unwrap()
                                        .as_secs(),
                                };
                                for tx in &senders {
                                    let _ = tx.send(msg.clone());
                                }
                            }
                        }
                    }
                }
            }
        });

        info!("Telegram platform '{}' connected (long polling)", self.id);
        Ok(())
    }

    async fn send_text(&self, chat_id: &str, text: &str) -> Result<(), String> {
        let client = reqwest::Client::new();
        let url = self.api_url("sendMessage");
        let resp = client.post(&url)
            .json(&serde_json::json!({
                "chat_id": chat_id,
                "text": text,
                "parse_mode": "Markdown",
            }))
            .send()
            .await
            .map_err(|e| format!("Telegram send failed: {}", e))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("Telegram API error {}: {}", status, body));
        }

        info!("Telegram send_text: chat_id={} len={}", chat_id, text.len());
        Ok(())
    }

    async fn send_card(&self, chat_id: &str, card: serde_json::Value) -> Result<(), String> {
        let text = card.get("elements")
            .and_then(|e| e.as_array())
            .map(|elems| {
                elems.iter()
                    .filter_map(|e| e.get("content").and_then(|c| c.as_str()))
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_else(|| serde_json::to_string_pretty(&card).unwrap_or_default());

        self.send_text(chat_id, &text).await
    }

    fn subscribe(&self, sender: MessageSender) {
        self.message_senders.lock().unwrap().push(sender);
    }

    async fn disconnect(&self) {
        self.running.store(false, Ordering::SeqCst);
    }
}
