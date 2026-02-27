use serde::{Deserialize, Serialize};

/// Events emitted by the Sandbox Agent SDK via SSE.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SandboxEvent {
    #[serde(rename = "session.started")]
    SessionStarted {
        session_id: String,
    },
    #[serde(rename = "session.ended")]
    SessionEnded {
        session_id: String,
    },
    #[serde(rename = "item.started")]
    ItemStarted {
        session_id: String,
        item_id: String,
        #[serde(default)]
        item_type: Option<String>,
    },
    #[serde(rename = "item.delta")]
    ItemDelta {
        session_id: String,
        item_id: String,
        #[serde(default)]
        delta: String,
    },
    #[serde(rename = "item.completed")]
    ItemCompleted {
        session_id: String,
        item_id: String,
        #[serde(default)]
        content: Option<String>,
    },
    #[serde(rename = "question.requested")]
    QuestionRequested {
        session_id: String,
        question_id: String,
        #[serde(default)]
        text: String,
        #[serde(default)]
        options: Vec<QuestionOption>,
    },
    #[serde(rename = "question.resolved")]
    QuestionResolved {
        session_id: String,
        question_id: String,
    },
    #[serde(rename = "permission.requested")]
    PermissionRequested {
        session_id: String,
        permission_id: String,
        #[serde(default)]
        description: String,
        #[serde(default)]
        command: Option<String>,
    },
    #[serde(rename = "permission.resolved")]
    PermissionResolved {
        session_id: String,
        permission_id: String,
    },
    #[serde(rename = "error")]
    Error {
        #[serde(default)]
        session_id: Option<String>,
        #[serde(default)]
        message: String,
    },
}

impl SandboxEvent {
    pub fn event_type(&self) -> &'static str {
        match self {
            Self::SessionStarted { .. } => "session.started",
            Self::SessionEnded { .. } => "session.ended",
            Self::ItemStarted { .. } => "item.started",
            Self::ItemDelta { .. } => "item.delta",
            Self::ItemCompleted { .. } => "item.completed",
            Self::QuestionRequested { .. } => "question.requested",
            Self::QuestionResolved { .. } => "question.resolved",
            Self::PermissionRequested { .. } => "permission.requested",
            Self::PermissionResolved { .. } => "permission.resolved",
            Self::Error { .. } => "error",
        }
    }

    pub fn session_id(&self) -> Option<&str> {
        match self {
            Self::SessionStarted { session_id, .. }
            | Self::SessionEnded { session_id, .. }
            | Self::ItemStarted { session_id, .. }
            | Self::ItemDelta { session_id, .. }
            | Self::ItemCompleted { session_id, .. }
            | Self::QuestionRequested { session_id, .. }
            | Self::QuestionResolved { session_id, .. }
            | Self::PermissionRequested { session_id, .. }
            | Self::PermissionResolved { session_id, .. } => Some(session_id),
            Self::Error { session_id, .. } => session_id.as_deref(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuestionOption {
    pub label: String,
    #[serde(default)]
    pub value: String,
}

/// Session creation request payload.
#[derive(Debug, Serialize)]
pub struct CreateSessionRequest {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
}

/// Message send request payload.
#[derive(Debug, Serialize)]
pub struct SendMessageRequest {
    pub message: String,
}

/// Question reply payload.
#[derive(Debug, Serialize)]
pub struct QuestionReplyRequest {
    pub answer: String,
}

/// Permission reply payload.
#[derive(Debug, Serialize)]
pub struct PermissionReplyRequest {
    pub approved: bool,
}

/// Health check response.
#[derive(Debug, Deserialize)]
pub struct HealthResponse {
    #[serde(default)]
    pub status: String,
}
