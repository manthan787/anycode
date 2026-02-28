use std::time::Duration;

use async_trait::async_trait;
use futures::stream::StreamExt;
use futures::SinkExt;
use serde_json::Value;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message as WsMessage;
use tracing::{debug, error, info, warn};

use crate::error::{AnycodeError, Result};

use super::traits::{InboundEvent, MessagingProvider, OutboundMessage};

pub struct SlackProvider {
    app_token: String,
    bot_token: String,
    client: reqwest::Client,
}

impl SlackProvider {
    pub fn new(app_token: &str, bot_token: &str) -> Self {
        Self {
            app_token: app_token.to_string(),
            bot_token: bot_token.to_string(),
            client: reqwest::Client::new(),
        }
    }

    /// Post a message to a Slack channel. Returns the message timestamp (ts).
    async fn post_message(&self, channel: &str, text: &str, blocks: Option<Value>) -> Result<String> {
        let mut body = serde_json::json!({
            "channel": channel,
            "text": text,
        });

        if let Some(blocks) = blocks {
            body["blocks"] = blocks;
        }

        let resp: Value = self
            .client
            .post("https://slack.com/api/chat.postMessage")
            .bearer_auth(&self.bot_token)
            .json(&body)
            .send()
            .await
            .map_err(|e| AnycodeError::Messaging(format!("chat.postMessage failed: {e}")))?
            .json()
            .await
            .map_err(|e| AnycodeError::Messaging(format!("chat.postMessage parse failed: {e}")))?;

        if resp["ok"].as_bool() != Some(true) {
            return Err(AnycodeError::Messaging(format!(
                "chat.postMessage error: {}",
                resp["error"].as_str().unwrap_or("unknown")
            )));
        }

        resp["ts"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| AnycodeError::Messaging("no ts in postMessage response".into()))
    }

    /// Update an existing Slack message. Returns the message timestamp.
    async fn update_message(&self, channel: &str, ts: &str, text: &str) -> Result<String> {
        let body = serde_json::json!({
            "channel": channel,
            "ts": ts,
            "text": text,
        });

        let resp: Value = self
            .client
            .post("https://slack.com/api/chat.update")
            .bearer_auth(&self.bot_token)
            .json(&body)
            .send()
            .await
            .map_err(|e| AnycodeError::Messaging(format!("chat.update failed: {e}")))?
            .json()
            .await
            .map_err(|e| AnycodeError::Messaging(format!("chat.update parse failed: {e}")))?;

        if resp["ok"].as_bool() != Some(true) {
            return Err(AnycodeError::Messaging(format!(
                "chat.update error: {}",
                resp["error"].as_str().unwrap_or("unknown")
            )));
        }

        resp["ts"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| AnycodeError::Messaging("no ts in update response".into()))
    }

    /// Build Block Kit actions block from button rows.
    fn build_button_blocks(buttons: &[Vec<(String, String)>]) -> Value {
        let mut elements = Vec::new();
        for row in buttons {
            for (label, data) in row {
                elements.push(serde_json::json!({
                    "type": "button",
                    "text": {
                        "type": "plain_text",
                        "text": label,
                    },
                    "value": data,
                    "action_id": data,
                }));
            }
        }

        serde_json::json!([
            {
                "type": "section",
                "text": {
                    "type": "mrkdwn",
                    "text": " ",
                }
            },
            {
                "type": "actions",
                "elements": elements,
            }
        ])
    }
}

#[async_trait]
impl MessagingProvider for SlackProvider {
    async fn send_message(&self, msg: OutboundMessage) -> Result<String> {
        // edit_message_id encodes "channel:ts" for Slack
        if let Some(ref edit_id) = msg.edit_message_id {
            self.update_message(&msg.chat_id, edit_id, &msg.text).await
        } else {
            let blocks = if msg.buttons.is_empty() {
                None
            } else {
                Some(Self::build_button_blocks(&msg.buttons))
            };
            self.post_message(&msg.chat_id, &msg.text, blocks).await
        }
    }

    async fn answer_callback(&self, _query_id: &str, _text: &str) -> Result<()> {
        // ACK happens at the WebSocket layer for Socket Mode — this is a no-op.
        Ok(())
    }

    async fn subscribe(&self) -> Result<mpsc::UnboundedReceiver<InboundEvent>> {
        let (tx, rx) = mpsc::unbounded_channel();
        let app_token = self.app_token.clone();
        let client = self.client.clone();

        tokio::spawn(async move {
            let max_retries: u32 = 10;
            let initial_backoff = Duration::from_secs(1);
            let max_backoff = Duration::from_secs(30);

            let mut retries: u32 = 0;
            let mut backoff = initial_backoff;

            loop {
                // Get a fresh WSS URL
                let ws_url = match get_ws_url_with_client(&client, &app_token).await {
                    Ok(url) => url,
                    Err(e) => {
                        error!("Failed to get Slack WS URL: {e}");
                        retries += 1;
                        if retries > max_retries {
                            error!("Slack: max retries exceeded, giving up");
                            return;
                        }
                        tokio::time::sleep(backoff).await;
                        backoff = (backoff * 2).min(max_backoff);
                        continue;
                    }
                };

                info!("Connecting to Slack Socket Mode");
                let ws_result = tokio_tungstenite::connect_async(&ws_url).await;

                let ws_stream = match ws_result {
                    Ok((stream, _)) => stream,
                    Err(e) => {
                        error!("Slack WS connect failed: {e}");
                        retries += 1;
                        if retries > max_retries {
                            error!("Slack: max retries exceeded, giving up");
                            return;
                        }
                        tokio::time::sleep(backoff).await;
                        backoff = (backoff * 2).min(max_backoff);
                        continue;
                    }
                };

                info!("Slack Socket Mode connected");
                retries = 0;
                backoff = initial_backoff;

                let (mut write, mut read) = ws_stream.split();

                while let Some(msg_result) = read.next().await {
                    let msg = match msg_result {
                        Ok(m) => m,
                        Err(e) => {
                            warn!("Slack WS read error: {e}");
                            break;
                        }
                    };

                    let text = match msg {
                        WsMessage::Text(t) => t,
                        WsMessage::Ping(data) => {
                            let _ = write.send(WsMessage::Pong(data)).await;
                            continue;
                        }
                        WsMessage::Close(_) => {
                            info!("Slack WS closed by server");
                            break;
                        }
                        _ => continue,
                    };

                    let envelope: Value = match serde_json::from_str(&text) {
                        Ok(v) => v,
                        Err(e) => {
                            warn!("Slack: failed to parse envelope: {e}");
                            continue;
                        }
                    };

                    // ACK the envelope
                    if let Some(envelope_id) = envelope["envelope_id"].as_str() {
                        let ack = serde_json::json!({ "envelope_id": envelope_id });
                        if let Err(e) = write
                            .send(WsMessage::Text(ack.to_string().into()))
                            .await
                        {
                            warn!("Failed to ACK Slack envelope: {e}");
                            break;
                        }
                    }

                    let envelope_type = envelope["type"].as_str().unwrap_or("");
                    match envelope_type {
                        "events_api" => {
                            if let Some(event) = parse_event_payload(&envelope) {
                                if tx.send(event).is_err() {
                                    debug!("Slack event channel closed");
                                    return;
                                }
                            }
                        }
                        "interactive" => {
                            if let Some(event) = parse_interactive_payload(&envelope) {
                                if tx.send(event).is_err() {
                                    debug!("Slack event channel closed");
                                    return;
                                }
                            }
                        }
                        "disconnect" => {
                            info!("Slack disconnect notice, will reconnect");
                            break;
                        }
                        "hello" => {
                            debug!("Slack hello received");
                        }
                        other => {
                            debug!("Slack: unhandled envelope type: {other}");
                        }
                    }
                }

                info!("Slack WS disconnected, reconnecting...");
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
        });

        Ok(rx)
    }

    async fn send_file(&self, chat_id: &str, filename: &str, data: Vec<u8>) -> Result<()> {
        let part = reqwest::multipart::Part::bytes(data)
            .file_name(filename.to_string())
            .mime_str("application/octet-stream")
            .map_err(|e| AnycodeError::Messaging(format!("multipart error: {e}")))?;

        let form = reqwest::multipart::Form::new()
            .text("channels", chat_id.to_string())
            .text("filename", filename.to_string())
            .text("title", filename.to_string())
            .part("file", part);

        let resp: Value = self
            .client
            .post("https://slack.com/api/files.upload")
            .bearer_auth(&self.bot_token)
            .multipart(form)
            .send()
            .await
            .map_err(|e| AnycodeError::Messaging(format!("files.upload failed: {e}")))?
            .json()
            .await
            .map_err(|e| AnycodeError::Messaging(format!("files.upload parse failed: {e}")))?;

        if resp["ok"].as_bool() != Some(true) {
            return Err(AnycodeError::Messaging(format!(
                "files.upload error: {}",
                resp["error"].as_str().unwrap_or("unknown")
            )));
        }

        Ok(())
    }
}

/// Standalone helper so the spawned task can call without &self.
async fn get_ws_url_with_client(client: &reqwest::Client, app_token: &str) -> Result<String> {
    let resp: Value = client
        .post("https://slack.com/api/apps.connections.open")
        .bearer_auth(app_token)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .send()
        .await
        .map_err(|e| AnycodeError::Messaging(format!("connections.open failed: {e}")))?
        .json()
        .await
        .map_err(|e| AnycodeError::Messaging(format!("connections.open parse failed: {e}")))?;

    if resp["ok"].as_bool() != Some(true) {
        return Err(AnycodeError::Messaging(format!(
            "connections.open error: {}",
            resp["error"].as_str().unwrap_or("unknown")
        )));
    }

    resp["url"]
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| AnycodeError::Messaging("no url in connections.open response".into()))
}

/// Parse an events_api envelope into an InboundEvent.
fn parse_event_payload(envelope: &Value) -> Option<InboundEvent> {
    let event = &envelope["payload"]["event"];
    let event_type = event["type"].as_str()?;

    match event_type {
        "message" => {
            // Ignore bot messages and subtypes (edits, joins, etc.)
            if event.get("bot_id").is_some() || event.get("subtype").is_some() {
                return None;
            }

            let channel = event["channel"].as_str()?.to_string();
            let user = event["user"].as_str()?.to_string();
            let text = event["text"].as_str().unwrap_or("").to_string();

            if let Some(stripped) = text.strip_prefix('/') {
                let mut parts = stripped.splitn(2, ' ');
                let command = parts.next().unwrap_or("").to_string();
                let args = parts.next().unwrap_or("").to_string();
                Some(InboundEvent::Command {
                    chat_id: channel,
                    user_id: user,
                    command,
                    args,
                })
            } else {
                Some(InboundEvent::Message {
                    chat_id: channel,
                    user_id: user,
                    text,
                })
            }
        }
        "app_mention" => {
            let channel = event["channel"].as_str()?.to_string();
            let user = event["user"].as_str()?.to_string();
            let text = event["text"].as_str().unwrap_or("").to_string();

            // Strip the mention prefix (e.g., "<@U123> /task ...")
            let text = if let Some(rest) = text.strip_prefix('<') {
                rest.find('>')
                    .map(|i| rest[i + 1..].trim().to_string())
                    .unwrap_or(text)
            } else {
                text
            };

            if let Some(stripped) = text.strip_prefix('/') {
                let mut parts = stripped.splitn(2, ' ');
                let command = parts.next().unwrap_or("").to_string();
                let args = parts.next().unwrap_or("").to_string();
                Some(InboundEvent::Command {
                    chat_id: channel,
                    user_id: user,
                    command,
                    args,
                })
            } else {
                Some(InboundEvent::Message {
                    chat_id: channel,
                    user_id: user,
                    text,
                })
            }
        }
        _ => None,
    }
}

/// Parse an interactive (block_actions) envelope into an InboundEvent.
fn parse_interactive_payload(envelope: &Value) -> Option<InboundEvent> {
    let payload = &envelope["payload"];
    let payload_type = payload["type"].as_str()?;

    if payload_type != "block_actions" {
        return None;
    }

    let action = payload["actions"].as_array()?.first()?;
    let action_value = action["value"].as_str().unwrap_or("");

    let channel = payload["channel"]["id"].as_str()?.to_string();
    let user = payload["user"]["id"].as_str()?.to_string();
    let message_ts = payload["message"]["ts"].as_str().unwrap_or("").to_string();

    Some(InboundEvent::CallbackQuery {
        query_id: String::new(), // Slack ACKs at WS layer
        chat_id: channel,
        user_id: user,
        message_id: message_ts,
        data: action_value.to_string(),
    })
}
