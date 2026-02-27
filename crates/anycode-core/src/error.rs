use thiserror::Error;

#[derive(Debug, Error)]
pub enum AnycodeError {
    #[error("config error: {0}")]
    Config(String),

    #[error("database error: {0}")]
    Database(#[from] tokio_rusqlite::Error),

    #[error("docker error: {0}")]
    Docker(#[from] bollard::errors::Error),

    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("sandbox error: {0}")]
    Sandbox(String),

    #[error("messaging error: {0}")]
    Messaging(String),

    #[error("session error: {0}")]
    Session(String),

    #[error("timeout: {0}")]
    Timeout(String),

    #[error("not found: {0}")]
    NotFound(String),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("{0}")]
    Internal(String),
}

pub type Result<T> = std::result::Result<T, AnycodeError>;
