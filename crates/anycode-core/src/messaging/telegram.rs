use async_trait::async_trait;
use teloxide::prelude::*;
use teloxide::types::{
    InlineKeyboardButton, InlineKeyboardMarkup, InputFile, MessageId, ParseMode,
};
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

use crate::error::{AnycodeError, Result};

use super::traits::{InboundEvent, MessagingProvider, OutboundMessage};

pub struct TelegramProvider {
    bot: Bot,
}

impl TelegramProvider {
    pub fn new(token: &str) -> Self {
        let bot = Bot::new(token);
        Self { bot }
    }
}

#[async_trait]
impl MessagingProvider for TelegramProvider {
    async fn send_message(&self, msg: OutboundMessage) -> Result<i64> {
        let chat_id = ChatId(msg.chat_id);

        if let Some(edit_id) = msg.edit_message_id {
            let result = self
                .bot
                .edit_message_text(chat_id, MessageId(edit_id as i32), &msg.text)
                .parse_mode(ParseMode::MarkdownV2)
                .await;

            match result {
                Ok(m) => Ok(m.id.0 as i64),
                Err(e) => {
                    warn!("Markdown edit failed, retrying plain: {e}");
                    let m = self
                        .bot
                        .edit_message_text(chat_id, MessageId(edit_id as i32), &msg.text)
                        .await
                        .map_err(|e| AnycodeError::Messaging(e.to_string()))?;
                    Ok(m.id.0 as i64)
                }
            }
        } else {
            let mut request = self.bot.send_message(chat_id, &msg.text);

            if !msg.buttons.is_empty() {
                let keyboard: Vec<Vec<InlineKeyboardButton>> = msg
                    .buttons
                    .iter()
                    .map(|row| {
                        row.iter()
                            .map(|(label, data)| InlineKeyboardButton::callback(label, data))
                            .collect()
                    })
                    .collect();
                request = request.reply_markup(InlineKeyboardMarkup::new(keyboard));
            }

            let result = request.await;
            match result {
                Ok(m) => {
                    debug!("Sent message {} to chat {}", m.id.0, msg.chat_id);
                    Ok(m.id.0 as i64)
                }
                Err(e) => Err(AnycodeError::Messaging(e.to_string())),
            }
        }
    }

    async fn answer_callback(&self, query_id: &str, text: &str) -> Result<()> {
        self.bot
            .answer_callback_query(teloxide::types::CallbackQueryId(query_id.to_string()))
            .text(text)
            .await
            .map_err(|e| AnycodeError::Messaging(e.to_string()))?;
        Ok(())
    }

    async fn subscribe(&self) -> Result<mpsc::UnboundedReceiver<InboundEvent>> {
        let (tx, rx) = mpsc::unbounded_channel();
        let bot = self.bot.clone();

        tokio::spawn(async move {
            info!("Starting Telegram polling");

            let handler = dptree::entry()
                .branch(
                    Update::filter_message().endpoint({
                        let tx = tx.clone();
                        move |msg: Message| {
                            let tx = tx.clone();
                            async move {
                                let chat_id = msg.chat.id.0;
                                let user_id =
                                    msg.from.as_ref().map(|u| u.id.0 as i64).unwrap_or(0);
                                let text = msg.text().unwrap_or("").to_string();

                                if let Some(stripped) = text.strip_prefix('/') {
                                    let mut parts = stripped.splitn(2, ' ');
                                    let command = parts
                                        .next()
                                        .unwrap_or("")
                                        .split('@')
                                        .next()
                                        .unwrap_or("");
                                    let args = parts.next().unwrap_or("").to_string();
                                    let _ = tx.send(InboundEvent::Command {
                                        chat_id,
                                        user_id,
                                        command: command.to_string(),
                                        args,
                                    });
                                } else {
                                    let _ = tx.send(InboundEvent::Message {
                                        chat_id,
                                        user_id,
                                        text,
                                    });
                                }
                                respond(())
                            }
                        }
                    }),
                )
                .branch(
                    Update::filter_callback_query().endpoint({
                        let tx = tx.clone();
                        move |q: CallbackQuery| {
                            let tx = tx.clone();
                            async move {
                                let query_id = q.id.to_string();
                                let data = q.data.unwrap_or_default();
                                let (chat_id, message_id) = match q.message {
                                    Some(ref m) => {
                                        let cid = m.chat().id.0;
                                        let mid = m
                                            .regular_message()
                                            .map(|rm| rm.id.0 as i64)
                                            .unwrap_or(0);
                                        (cid, mid)
                                    }
                                    None => (0, 0),
                                };
                                let user_id = q.from.id.0 as i64;
                                let _ = tx.send(InboundEvent::CallbackQuery {
                                    query_id,
                                    chat_id,
                                    user_id,
                                    message_id,
                                    data,
                                });
                                respond(())
                            }
                        }
                    }),
                );

            Dispatcher::builder(bot, handler)
                .enable_ctrlc_handler()
                .build()
                .dispatch()
                .await;

            error!("Telegram dispatcher exited");
        });

        Ok(rx)
    }

    async fn send_file(&self, chat_id: i64, filename: &str, data: Vec<u8>) -> Result<()> {
        let file = InputFile::memory(data).file_name(filename.to_string());
        self.bot
            .send_document(ChatId(chat_id), file)
            .await
            .map_err(|e| AnycodeError::Messaging(e.to_string()))?;
        Ok(())
    }
}
