use std::path::PathBuf;

use argon_core::{AgentKind, Finding, PrKey, SessionKind};
use chrono::{DateTime, TimeZone, Utc};
use rusqlite::types::Type;
use rusqlite::{OptionalExtension, Row, params};

use crate::Store;
use crate::error::Result;

const SESSION_COLUMNS: &str = "id, host, pr_id, agent, model, template, status, worktree_path, \
     acp_session_id, nonce, allowed_files, created_at, updated_at, model_label, failure_reason, \
     transport, permission_mode, kind, base_sha";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SessionStatus {
    Running,
    AwaitingUser,
    Completed,
    Failed,
    Archived,
}

impl SessionStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::AwaitingUser => "awaiting_user",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Archived => "archived",
        }
    }

    pub fn parse(value: &str) -> Option<SessionStatus> {
        match value {
            "running" => Some(Self::Running),
            "awaiting_user" => Some(Self::AwaitingUser),
            "completed" => Some(Self::Completed),
            "failed" => Some(Self::Failed),
            "archived" => Some(Self::Archived),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FindingState {
    New,
    Added,
    Ignored,
}

impl FindingState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::New => "new",
            Self::Added => "added",
            Self::Ignored => "ignored",
        }
    }

    pub fn parse(value: &str) -> Option<FindingState> {
        match value {
            "new" => Some(Self::New),
            "added" => Some(Self::Added),
            "ignored" => Some(Self::Ignored),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewAgentSession {
    pub id: String,
    pub pr: PrKey,
    pub agent: AgentKind,
    pub model: String,
    pub template: String,
    pub worktree_path: Option<PathBuf>,
    pub acp_session_id: Option<String>,
    pub transport: String,
    pub permission_mode: String,
    pub nonce: String,
    pub allowed_files: Vec<String>,
    pub kind: SessionKind,
    pub base_sha: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionRow {
    pub id: String,
    pub pr: PrKey,
    pub agent: AgentKind,
    pub model: String,
    pub template: String,
    pub status: SessionStatus,
    pub worktree_path: Option<PathBuf>,
    pub acp_session_id: Option<String>,
    pub nonce: String,
    pub allowed_files: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub model_label: Option<String>,
    pub failure_reason: Option<String>,
    pub transport: String,
    pub permission_mode: String,
    pub kind: SessionKind,
    pub base_sha: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredFinding {
    pub id: i64,
    pub session_id: String,
    pub finding: Finding,
    pub state: FindingState,
}

pub(crate) fn invalid(index: usize, kind: &str, value: &str) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        index,
        Type::Text,
        format!("unknown {kind} {value:?}").into(),
    )
}

fn session_kind(value: &str) -> Option<SessionKind> {
    match value {
        "review" => Some(SessionKind::Review),
        "plan" => Some(SessionKind::Plan),
        _ => None,
    }
}

fn agent_key(kind: AgentKind) -> Result<String> {
    Ok(serde_json::to_value(kind)?
        .as_str()
        .unwrap_or_default()
        .to_owned())
}

pub(crate) fn millis(index: usize, value: i64) -> rusqlite::Result<DateTime<Utc>> {
    Utc.timestamp_millis_opt(value)
        .single()
        .ok_or_else(|| invalid(index, "timestamp", &value.to_string()))
}

fn session_from_row(row: &Row<'_>) -> rusqlite::Result<SessionRow> {
    let agent: String = row.get(3)?;
    let status: String = row.get(6)?;
    let worktree: Option<String> = row.get(7)?;
    let allowed: String = row.get(10)?;
    let kind: String = row.get(17)?;
    Ok(SessionRow {
        id: row.get(0)?,
        pr: PrKey::new(row.get::<_, String>(1)?, row.get::<_, String>(2)?),
        agent: serde_json::from_value(serde_json::Value::String(agent.clone()))
            .map_err(|_| invalid(3, "agent", &agent))?,
        model: row.get(4)?,
        template: row.get(5)?,
        status: SessionStatus::parse(&status).ok_or_else(|| invalid(6, "status", &status))?,
        worktree_path: worktree.map(PathBuf::from),
        acp_session_id: row.get(8)?,
        nonce: row.get(9)?,
        allowed_files: serde_json::from_str(&allowed)
            .map_err(|_| invalid(10, "allowed_files", &allowed))?,
        created_at: millis(11, row.get(11)?)?,
        updated_at: millis(12, row.get(12)?)?,
        model_label: row.get(13)?,
        failure_reason: row.get(14)?,
        transport: row.get(15)?,
        permission_mode: row.get(16)?,
        kind: session_kind(&kind).ok_or_else(|| invalid(17, "kind", &kind))?,
        base_sha: row.get(18)?,
    })
}

impl Store {
    pub fn create_session(&mut self, session: &NewAgentSession) -> Result<()> {
        let now = Utc::now().timestamp_millis();
        self.conn.execute(
            "INSERT INTO agent_sessions
                (id, host, pr_id, agent, model, template, status, worktree_path, acp_session_id,
                 nonce, allowed_files, created_at, updated_at, transport, permission_mode, kind,
                 base_sha)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?12, ?13, ?14, ?15, ?16)",
            params![
                session.id,
                session.pr.host,
                session.pr.id.0,
                agent_key(session.agent)?,
                session.model,
                session.template,
                SessionStatus::Running.as_str(),
                session
                    .worktree_path
                    .as_ref()
                    .map(|path| path.to_string_lossy().into_owned()),
                session.acp_session_id,
                session.nonce,
                serde_json::to_string(&session.allowed_files)?,
                now,
                session.transport,
                session.permission_mode,
                session.kind.as_str(),
                session.base_sha,
            ],
        )?;
        Ok(())
    }

    pub fn set_session_status(&mut self, id: &str, status: SessionStatus) -> Result<()> {
        self.conn.execute(
            "UPDATE agent_sessions SET status = ?1, updated_at = ?2,
                 failure_reason = CASE WHEN ?1 = 'failed' THEN failure_reason END
             WHERE id = ?3 AND status <> 'archived'",
            params![status.as_str(), Utc::now().timestamp_millis(), id],
        )?;
        Ok(())
    }

    pub fn set_session_model(&mut self, id: &str, model: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE agent_sessions SET model = ?1, updated_at = ?2 WHERE id = ?3",
            params![model, Utc::now().timestamp_millis(), id],
        )?;
        Ok(())
    }

    pub fn set_session_permission_mode(&mut self, id: &str, mode: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE agent_sessions SET permission_mode = ?1, updated_at = ?2 WHERE id = ?3",
            params![mode, Utc::now().timestamp_millis(), id],
        )?;
        Ok(())
    }

    pub fn set_session_acp_id(&mut self, id: &str, acp_session_id: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE agent_sessions SET acp_session_id = ?1, updated_at = ?2 WHERE id = ?3",
            params![acp_session_id, Utc::now().timestamp_millis(), id],
        )?;
        Ok(())
    }

    pub fn session(&self, id: &str) -> Result<Option<SessionRow>> {
        let row = self
            .conn
            .query_row(
                &format!("SELECT {SESSION_COLUMNS} FROM agent_sessions WHERE id = ?1"),
                params![id],
                session_from_row,
            )
            .optional()?;
        Ok(row)
    }

    pub fn sessions(&self) -> Result<Vec<SessionRow>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {SESSION_COLUMNS} FROM agent_sessions ORDER BY created_at DESC, id"
        ))?;
        let rows = stmt
            .query_map([], session_from_row)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }
}

#[cfg(test)]
#[path = "agents_tests.rs"]
pub(crate) mod tests;
