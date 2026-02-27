use tokio_rusqlite::Connection;

use crate::error::Result;

use super::models::*;

const SCHEMA: &str = include_str!("../../../../migrations/001_initial.sql");

#[derive(Clone)]
pub struct Repository {
    conn: Connection,
}

impl Repository {
    pub async fn new(path: &str) -> Result<Self> {
        let conn = Connection::open(path).await?;
        conn.call(|conn| {
            conn.execute_batch(SCHEMA)?;
            Ok(())
        })
        .await?;
        Ok(Self { conn })
    }

    pub async fn new_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory().await?;
        conn.call(|conn| {
            conn.execute_batch(SCHEMA)?;
            Ok(())
        })
        .await?;
        Ok(Self { conn })
    }

    // --- Sessions ---

    pub async fn create_session(&self, session: &Session) -> Result<()> {
        let s = session.clone();
        self.conn
            .call(move |conn| {
                conn.execute(
                    "INSERT INTO sessions (id, chat_id, agent, prompt, repo_url, sandbox_id, sandbox_port, session_api_id, status, created_at, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                    rusqlite::params![
                        s.id,
                        s.chat_id,
                        s.agent,
                        s.prompt,
                        s.repo_url,
                        s.sandbox_id,
                        s.sandbox_port,
                        s.session_api_id,
                        s.status.as_str(),
                        s.created_at,
                        s.updated_at,
                    ],
                )?;
                Ok(())
            })
            .await?;
        Ok(())
    }

    pub async fn get_session(&self, id: &str) -> Result<Option<Session>> {
        let id = id.to_string();
        self.conn
            .call(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, chat_id, agent, prompt, repo_url, sandbox_id, sandbox_port, session_api_id, status, created_at, updated_at
                     FROM sessions WHERE id = ?1",
                )?;
                let result = stmt
                    .query_row(rusqlite::params![id], |row| {
                        Ok(Session {
                            id: row.get(0)?,
                            chat_id: row.get(1)?,
                            agent: row.get(2)?,
                            prompt: row.get(3)?,
                            repo_url: row.get(4)?,
                            sandbox_id: row.get(5)?,
                            sandbox_port: row.get(6)?,
                            session_api_id: row.get(7)?,
                            status: SessionStatus::from_str(
                                &row.get::<_, String>(8)?,
                            ),
                            created_at: row.get(9)?,
                            updated_at: row.get(10)?,
                        })
                    })
                    .optional()?;
                Ok(result)
            })
            .await
            .map_err(Into::into)
    }

    pub async fn update_session_status(&self, id: &str, status: SessionStatus) -> Result<()> {
        let id = id.to_string();
        self.conn
            .call(move |conn| {
                conn.execute(
                    "UPDATE sessions SET status = ?1, updated_at = datetime('now') WHERE id = ?2",
                    rusqlite::params![status.as_str(), id],
                )?;
                Ok(())
            })
            .await?;
        Ok(())
    }

    pub async fn update_session_status_if_active(
        &self,
        id: &str,
        status: SessionStatus,
    ) -> Result<bool> {
        let id = id.to_string();
        self.conn
            .call(move |conn| {
                let changed = conn.execute(
                    "UPDATE sessions
                     SET status = ?1, updated_at = datetime('now')
                     WHERE id = ?2 AND status IN ('pending', 'starting', 'running')",
                    rusqlite::params![status.as_str(), id],
                )?;
                Ok(changed > 0)
            })
            .await
            .map_err(Into::into)
    }

    pub async fn update_session_sandbox(
        &self,
        id: &str,
        sandbox_id: &str,
        port: u16,
    ) -> Result<()> {
        let id = id.to_string();
        let sandbox_id = sandbox_id.to_string();
        self.conn
            .call(move |conn| {
                conn.execute(
                    "UPDATE sessions SET sandbox_id = ?1, sandbox_port = ?2, updated_at = datetime('now') WHERE id = ?3",
                    rusqlite::params![sandbox_id, port as i64, id],
                )?;
                Ok(())
            })
            .await?;
        Ok(())
    }

    pub async fn update_session_api_id(&self, id: &str, session_api_id: &str) -> Result<()> {
        let id = id.to_string();
        let session_api_id = session_api_id.to_string();
        self.conn
            .call(move |conn| {
                conn.execute(
                    "UPDATE sessions SET session_api_id = ?1, updated_at = datetime('now') WHERE id = ?2",
                    rusqlite::params![session_api_id, id],
                )?;
                Ok(())
            })
            .await?;
        Ok(())
    }

    pub async fn get_active_sessions_for_chat(&self, chat_id: i64) -> Result<Vec<Session>> {
        self.conn
            .call(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, chat_id, agent, prompt, repo_url, sandbox_id, sandbox_port, session_api_id, status, created_at, updated_at
                     FROM sessions WHERE chat_id = ?1 AND status IN ('pending', 'starting', 'running')
                     ORDER BY created_at DESC",
                )?;
                let rows = stmt
                    .query_map(rusqlite::params![chat_id], |row| {
                        Ok(Session {
                            id: row.get(0)?,
                            chat_id: row.get(1)?,
                            agent: row.get(2)?,
                            prompt: row.get(3)?,
                            repo_url: row.get(4)?,
                            sandbox_id: row.get(5)?,
                            sandbox_port: row.get(6)?,
                            session_api_id: row.get(7)?,
                            status: SessionStatus::from_str(
                                &row.get::<_, String>(8)?,
                            ),
                            created_at: row.get(9)?,
                            updated_at: row.get(10)?,
                        })
                    })?
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                Ok(rows)
            })
            .await
            .map_err(Into::into)
    }

    pub async fn get_all_running_sessions(&self) -> Result<Vec<Session>> {
        self.conn
            .call(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, chat_id, agent, prompt, repo_url, sandbox_id, sandbox_port, session_api_id, status, created_at, updated_at
                     FROM sessions WHERE status IN ('pending', 'starting', 'running')
                     ORDER BY created_at DESC",
                )?;
                let rows = stmt
                    .query_map([], |row| {
                        Ok(Session {
                            id: row.get(0)?,
                            chat_id: row.get(1)?,
                            agent: row.get(2)?,
                            prompt: row.get(3)?,
                            repo_url: row.get(4)?,
                            sandbox_id: row.get(5)?,
                            sandbox_port: row.get(6)?,
                            session_api_id: row.get(7)?,
                            status: SessionStatus::from_str(
                                &row.get::<_, String>(8)?,
                            ),
                            created_at: row.get(9)?,
                            updated_at: row.get(10)?,
                        })
                    })?
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                Ok(rows)
            })
            .await
            .map_err(Into::into)
    }

    // --- Pending Interactions ---

    pub async fn create_pending_interaction(&self, pi: &PendingInteraction) -> Result<()> {
        let pi = pi.clone();
        self.conn
            .call(move |conn| {
                conn.execute(
                    "INSERT INTO pending_interactions (id, session_id, kind, question_id, permission_id, telegram_message_id, payload, resolved, created_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                    rusqlite::params![
                        pi.id,
                        pi.session_id,
                        pi.kind.as_str(),
                        pi.question_id,
                        pi.permission_id,
                        pi.telegram_message_id,
                        pi.payload,
                        pi.resolved as i32,
                        pi.created_at,
                    ],
                )?;
                Ok(())
            })
            .await?;
        Ok(())
    }

    pub async fn get_pending_interaction(&self, id: &str) -> Result<Option<PendingInteraction>> {
        let id = id.to_string();
        self.conn
            .call(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, session_id, kind, question_id, permission_id, telegram_message_id, payload, resolved, created_at
                     FROM pending_interactions WHERE id = ?1",
                )?;
                let result = stmt
                    .query_row(rusqlite::params![id], |row| {
                        Ok(PendingInteraction {
                            id: row.get(0)?,
                            session_id: row.get(1)?,
                            kind: InteractionKind::from_str(
                                &row.get::<_, String>(2)?,
                            ),
                            question_id: row.get(3)?,
                            permission_id: row.get(4)?,
                            telegram_message_id: row.get(5)?,
                            payload: row.get(6)?,
                            resolved: row.get::<_, i32>(7)? != 0,
                            created_at: row.get(8)?,
                        })
                    })
                    .optional()?;
                Ok(result)
            })
            .await
            .map_err(Into::into)
    }

    pub async fn resolve_pending_interaction(&self, id: &str) -> Result<()> {
        let id = id.to_string();
        self.conn
            .call(move |conn| {
                conn.execute(
                    "UPDATE pending_interactions SET resolved = 1 WHERE id = ?1",
                    rusqlite::params![id],
                )?;
                Ok(())
            })
            .await?;
        Ok(())
    }

    pub async fn get_unresolved_for_session(
        &self,
        session_id: &str,
    ) -> Result<Vec<PendingInteraction>> {
        let session_id = session_id.to_string();
        self.conn
            .call(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, session_id, kind, question_id, permission_id, telegram_message_id, payload, resolved, created_at
                     FROM pending_interactions WHERE session_id = ?1 AND resolved = 0
                     ORDER BY created_at ASC",
                )?;
                let rows = stmt
                    .query_map(rusqlite::params![session_id], |row| {
                        Ok(PendingInteraction {
                            id: row.get(0)?,
                            session_id: row.get(1)?,
                            kind: InteractionKind::from_str(
                                &row.get::<_, String>(2)?,
                            ),
                            question_id: row.get(3)?,
                            permission_id: row.get(4)?,
                            telegram_message_id: row.get(5)?,
                            payload: row.get(6)?,
                            resolved: row.get::<_, i32>(7)? != 0,
                            created_at: row.get(8)?,
                        })
                    })?
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                Ok(rows)
            })
            .await
            .map_err(Into::into)
    }

    // --- Event Log ---

    pub async fn log_event(
        &self,
        session_id: &str,
        event_type: &str,
        payload: Option<&str>,
    ) -> Result<()> {
        let session_id = session_id.to_string();
        let event_type = event_type.to_string();
        let payload = payload.map(|s| s.to_string());
        self.conn
            .call(move |conn| {
                conn.execute(
                    "INSERT INTO event_log (session_id, event_type, payload) VALUES (?1, ?2, ?3)",
                    rusqlite::params![session_id, event_type, payload],
                )?;
                Ok(())
            })
            .await?;
        Ok(())
    }

    pub async fn get_events_for_session(&self, session_id: &str) -> Result<Vec<EventLogEntry>> {
        let session_id = session_id.to_string();
        self.conn
            .call(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, session_id, event_type, payload, created_at
                     FROM event_log WHERE session_id = ?1 ORDER BY id ASC",
                )?;
                let rows = stmt
                    .query_map(rusqlite::params![session_id], |row| {
                        Ok(EventLogEntry {
                            id: row.get(0)?,
                            session_id: row.get(1)?,
                            event_type: row.get(2)?,
                            payload: row.get(3)?,
                            created_at: row.get(4)?,
                        })
                    })?
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                Ok(rows)
            })
            .await
            .map_err(Into::into)
    }
}

trait QueryRowOptional<T> {
    fn optional(self) -> std::result::Result<Option<T>, rusqlite::Error>;
}

impl<T> QueryRowOptional<T> for std::result::Result<T, rusqlite::Error> {
    fn optional(self) -> std::result::Result<Option<T>, rusqlite::Error> {
        match self {
            Ok(v) => Ok(Some(v)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_create_and_get_session() {
        let repo = Repository::new_in_memory().await.unwrap();

        let session = Session {
            id: "test-1".to_string(),
            chat_id: 12345,
            agent: "claude-code".to_string(),
            prompt: "fix the bug".to_string(),
            repo_url: Some("https://github.com/org/repo".to_string()),
            sandbox_id: None,
            sandbox_port: None,
            session_api_id: None,
            status: SessionStatus::Pending,
            created_at: "2024-01-01T00:00:00".to_string(),
            updated_at: "2024-01-01T00:00:00".to_string(),
        };

        repo.create_session(&session).await.unwrap();

        let fetched = repo.get_session("test-1").await.unwrap().unwrap();
        assert_eq!(fetched.id, "test-1");
        assert_eq!(fetched.chat_id, 12345);
        assert_eq!(fetched.agent, "claude-code");
        assert_eq!(fetched.status, SessionStatus::Pending);
    }

    #[tokio::test]
    async fn test_update_session_status() {
        let repo = Repository::new_in_memory().await.unwrap();

        let session = Session {
            id: "test-2".to_string(),
            chat_id: 12345,
            agent: "claude-code".to_string(),
            prompt: "fix".to_string(),
            repo_url: None,
            sandbox_id: None,
            sandbox_port: None,
            session_api_id: None,
            status: SessionStatus::Pending,
            created_at: "2024-01-01T00:00:00".to_string(),
            updated_at: "2024-01-01T00:00:00".to_string(),
        };

        repo.create_session(&session).await.unwrap();
        repo.update_session_status("test-2", SessionStatus::Running)
            .await
            .unwrap();

        let fetched = repo.get_session("test-2").await.unwrap().unwrap();
        assert_eq!(fetched.status, SessionStatus::Running);
    }

    #[tokio::test]
    async fn test_update_session_status_if_active() {
        let repo = Repository::new_in_memory().await.unwrap();

        let session = Session {
            id: "test-2b".to_string(),
            chat_id: 12345,
            agent: "claude-code".to_string(),
            prompt: "fix".to_string(),
            repo_url: None,
            sandbox_id: None,
            sandbox_port: None,
            session_api_id: None,
            status: SessionStatus::Pending,
            created_at: "2024-01-01T00:00:00".to_string(),
            updated_at: "2024-01-01T00:00:00".to_string(),
        };

        repo.create_session(&session).await.unwrap();

        let changed = repo
            .update_session_status_if_active("test-2b", SessionStatus::Running)
            .await
            .unwrap();
        assert!(changed);

        let fetched = repo.get_session("test-2b").await.unwrap().unwrap();
        assert_eq!(fetched.status, SessionStatus::Running);
    }

    #[tokio::test]
    async fn test_update_session_status_if_active_does_not_override_terminal() {
        let repo = Repository::new_in_memory().await.unwrap();

        let session = Session {
            id: "test-2c".to_string(),
            chat_id: 12345,
            agent: "claude-code".to_string(),
            prompt: "fix".to_string(),
            repo_url: None,
            sandbox_id: None,
            sandbox_port: None,
            session_api_id: None,
            status: SessionStatus::Completed,
            created_at: "2024-01-01T00:00:00".to_string(),
            updated_at: "2024-01-01T00:00:00".to_string(),
        };

        repo.create_session(&session).await.unwrap();

        let changed = repo
            .update_session_status_if_active("test-2c", SessionStatus::Failed)
            .await
            .unwrap();
        assert!(!changed);

        let fetched = repo.get_session("test-2c").await.unwrap().unwrap();
        assert_eq!(fetched.status, SessionStatus::Completed);
    }

    #[tokio::test]
    async fn test_active_sessions_for_chat() {
        let repo = Repository::new_in_memory().await.unwrap();

        for (id, status) in [
            ("s1", SessionStatus::Running),
            ("s2", SessionStatus::Completed),
            ("s3", SessionStatus::Pending),
        ] {
            let session = Session {
                id: id.to_string(),
                chat_id: 100,
                agent: "claude-code".to_string(),
                prompt: "test".to_string(),
                repo_url: None,
                sandbox_id: None,
                sandbox_port: None,
                session_api_id: None,
                status,
                created_at: "2024-01-01T00:00:00".to_string(),
                updated_at: "2024-01-01T00:00:00".to_string(),
            };
            repo.create_session(&session).await.unwrap();
        }

        let active = repo.get_active_sessions_for_chat(100).await.unwrap();
        assert_eq!(active.len(), 2);
    }

    #[tokio::test]
    async fn test_pending_interactions() {
        let repo = Repository::new_in_memory().await.unwrap();

        let session = Session {
            id: "s1".to_string(),
            chat_id: 100,
            agent: "claude-code".to_string(),
            prompt: "test".to_string(),
            repo_url: None,
            sandbox_id: None,
            sandbox_port: None,
            session_api_id: None,
            status: SessionStatus::Running,
            created_at: "2024-01-01T00:00:00".to_string(),
            updated_at: "2024-01-01T00:00:00".to_string(),
        };
        repo.create_session(&session).await.unwrap();

        let pi = PendingInteraction {
            id: "pi-1".to_string(),
            session_id: "s1".to_string(),
            kind: InteractionKind::Question,
            question_id: Some("q-1".to_string()),
            permission_id: None,
            telegram_message_id: Some(999),
            payload: Some(r#"{"text":"Which file?"}"#.to_string()),
            resolved: false,
            created_at: "2024-01-01T00:00:00".to_string(),
        };
        repo.create_pending_interaction(&pi).await.unwrap();

        let fetched = repo.get_pending_interaction("pi-1").await.unwrap().unwrap();
        assert_eq!(fetched.kind, InteractionKind::Question);
        assert!(!fetched.resolved);

        repo.resolve_pending_interaction("pi-1").await.unwrap();

        let fetched = repo.get_pending_interaction("pi-1").await.unwrap().unwrap();
        assert!(fetched.resolved);
    }

    #[tokio::test]
    async fn test_event_log() {
        let repo = Repository::new_in_memory().await.unwrap();

        let session = Session {
            id: "s1".to_string(),
            chat_id: 100,
            agent: "claude-code".to_string(),
            prompt: "test".to_string(),
            repo_url: None,
            sandbox_id: None,
            sandbox_port: None,
            session_api_id: None,
            status: SessionStatus::Running,
            created_at: "2024-01-01T00:00:00".to_string(),
            updated_at: "2024-01-01T00:00:00".to_string(),
        };
        repo.create_session(&session).await.unwrap();

        repo.log_event("s1", "session.started", None).await.unwrap();
        repo.log_event("s1", "item.delta", Some(r#"{"text":"hello"}"#))
            .await
            .unwrap();

        let events = repo.get_events_for_session("s1").await.unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].event_type, "session.started");
        assert_eq!(events[1].event_type, "item.delta");
    }
}
