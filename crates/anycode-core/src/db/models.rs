use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub chat_id: i64,
    pub agent: String,
    pub prompt: String,
    pub repo_url: Option<String>,
    pub sandbox_id: Option<String>,
    pub sandbox_port: Option<i64>,
    pub session_api_id: Option<String>,
    pub status: SessionStatus,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    Pending,
    Starting,
    Running,
    Completed,
    Failed,
    Cancelled,
}

impl SessionStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Starting => "starting",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "pending" => Self::Pending,
            "starting" => Self::Starting,
            "running" => Self::Running,
            "completed" => Self::Completed,
            "failed" => Self::Failed,
            "cancelled" => Self::Cancelled,
            _ => Self::Failed,
        }
    }

    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingInteraction {
    pub id: String,
    pub session_id: String,
    pub kind: InteractionKind,
    pub question_id: Option<String>,
    pub permission_id: Option<String>,
    pub telegram_message_id: Option<i64>,
    pub payload: Option<String>,
    pub resolved: bool,
    pub created_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InteractionKind {
    Question,
    Permission,
}

impl InteractionKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Question => "question",
            Self::Permission => "permission",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "question" => Self::Question,
            _ => Self::Permission,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventLogEntry {
    pub id: i64,
    pub session_id: String,
    pub event_type: String,
    pub payload: Option<String>,
    pub created_at: String,
}
