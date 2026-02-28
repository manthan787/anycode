use async_trait::async_trait;
use tokio::sync::mpsc;

use crate::error::Result;

/// An outbound message to send via messaging platform.
#[derive(Debug, Clone)]
pub struct OutboundMessage {
    pub chat_id: String,
    pub text: String,
    /// If set, edit this existing message instead of sending a new one.
    pub edit_message_id: Option<String>,
    /// Inline keyboard buttons: Vec of rows, each row is Vec of (label, callback_data).
    pub buttons: Vec<Vec<(String, String)>>,
}

/// An inbound event from the messaging platform.
#[derive(Debug, Clone)]
pub enum InboundEvent {
    Command {
        chat_id: String,
        user_id: String,
        command: String,
        args: String,
    },
    Message {
        chat_id: String,
        user_id: String,
        text: String,
    },
    CallbackQuery {
        query_id: String,
        chat_id: String,
        user_id: String,
        message_id: String,
        data: String,
    },
}

#[async_trait]
pub trait MessagingProvider: Send + Sync + 'static {
    /// Send a message (or edit an existing one). Returns the message ID.
    async fn send_message(&self, msg: OutboundMessage) -> Result<String>;

    /// Acknowledge a callback query (button press).
    async fn answer_callback(&self, query_id: &str, text: &str) -> Result<()>;

    /// Subscribe to inbound events. Returns a receiver channel.
    async fn subscribe(&self) -> Result<mpsc::UnboundedReceiver<InboundEvent>>;

    /// Upload a file to a chat.
    async fn send_file(&self, chat_id: &str, filename: &str, data: Vec<u8>) -> Result<()>;
}
