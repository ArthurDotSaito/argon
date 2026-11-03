use std::path::PathBuf;

use argon_core::{AgentKind, Finding, PrKey, Severity, Side};
use chrono::{DateTime, TimeZone, Utc};
use rusqlite::types::Type;
use rusqlite::{OptionalExtension, Row, params};
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::Store;
use crate::error::Result;

const SESSION_COLUMNS: &str = "id, host, pr_id, agent, model, template, status, worktree_path, \
     acp_session_id, nonce, allowed_files, created_at, updated_at";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SessionStatus {
    Running,
    AwaitingUser,
    Completed,
    Failed,
}

impl SessionStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::AwaitingUser => "awaiting_user",
            Self::Completed => "completed",
            Self::Failed => "failed",
        }
    }

    pub fn parse(value: &str) -> Option<SessionStatus> {
        match value {
            "running" => Some(Self::Running),
            "awaiting_user" => Some(Self::AwaitingUser),
            "completed" => Some(Self::Completed),
            "failed" => Some(Self::Failed),
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
    pub nonce: String,
    pub allowed_files: Vec<String>,
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredFinding {
    pub id: i64,
    pub session_id: String,
    pub finding: Finding,
    pub state: FindingState,
}

fn invalid(index: usize, kind: &str, value: &str) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        index,
        Type::Text,
        format!("unknown {kind} {value:?}").into(),
    )
}

fn agent_key(kind: AgentKind) -> Result<String> {
    Ok(serde_json::to_value(kind)?
        .as_str()
        .unwrap_or_default()
        .to_owned())
}

fn millis(index: usize, value: i64) -> rusqlite::Result<DateTime<Utc>> {
    Utc.timestamp_millis_opt(value)
        .single()
        .ok_or_else(|| invalid(index, "timestamp", &value.to_string()))
}

fn session_from_row(row: &Row<'_>) -> rusqlite::Result<SessionRow> {
    let agent: String = row.get(3)?;
    let status: String = row.get(6)?;
    let worktree: Option<String> = row.get(7)?;
    let allowed: String = row.get(10)?;
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
    })
}

fn finding_from_row(row: &Row<'_>) -> rusqlite::Result<StoredFinding> {
    let severity: String = row.get(2)?;
    let side: String = row.get(5)?;
    let state: String = row.get(9)?;
    Ok(StoredFinding {
        id: row.get(0)?,
        session_id: row.get(1)?,
        finding: Finding {
            severity: Severity::parse(&severity)
                .ok_or_else(|| invalid(2, "severity", &severity))?,
            file: row.get(3)?,
            line: row.get(4)?,
            side: Side::from_graphql(&side).ok_or_else(|| invalid(5, "side", &side))?,
            title: row.get(6)?,
            body: row.get(7)?,
            suggestion: row.get(8)?,
        },
        state: FindingState::parse(&state).ok_or_else(|| invalid(9, "state", &state))?,
    })
}

impl Store {
    pub fn create_session(&mut self, session: &NewAgentSession) -> Result<()> {
        let now = Utc::now().timestamp_millis();
        self.conn.execute(
            "INSERT INTO agent_sessions
                (id, host, pr_id, agent, model, template, status, worktree_path, acp_session_id,
                 nonce, allowed_files, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?12)",
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
            ],
        )?;
        Ok(())
    }

    pub fn set_session_status(&mut self, id: &str, status: SessionStatus) -> Result<()> {
        self.conn.execute(
            "UPDATE agent_sessions SET status = ?1, updated_at = ?2 WHERE id = ?3",
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

    pub fn append_event<E: Serialize>(&mut self, id: &str, event: &E) -> Result<i64> {
        let data = serde_json::to_string(event)?;
        let tx = self.conn.transaction()?;
        let seq: i64 = tx.query_row(
            "SELECT COALESCE(MAX(seq), 0) + 1 FROM agent_events WHERE session_id = ?1",
            params![id],
            |row| row.get(0),
        )?;
        tx.execute(
            "INSERT INTO agent_events (session_id, seq, data) VALUES (?1, ?2, ?3)",
            params![id, seq, data],
        )?;
        tx.execute(
            "UPDATE agent_sessions SET updated_at = ?1 WHERE id = ?2",
            params![Utc::now().timestamp_millis(), id],
        )?;
        tx.commit()?;
        Ok(seq)
    }

    pub fn events<E: DeserializeOwned>(&self, id: &str) -> Result<Vec<E>> {
        let mut stmt = self
            .conn
            .prepare("SELECT data FROM agent_events WHERE session_id = ?1 ORDER BY seq")?;
        let rows = stmt
            .query_map(params![id], |row| row.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let events = rows
            .iter()
            .map(|data| serde_json::from_str(data))
            .collect::<std::result::Result<Vec<E>, _>>()?;
        Ok(events)
    }

    pub fn save_findings(&mut self, id: &str, findings: &[Finding]) -> Result<Vec<i64>> {
        let tx = self.conn.transaction()?;
        let mut ids = Vec::with_capacity(findings.len());
        {
            let mut stmt = tx.prepare(
                "INSERT INTO findings
                    (session_id, severity, path, line, side, title, body, suggestion, state)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            )?;
            for finding in findings {
                stmt.execute(params![
                    id,
                    finding.severity.as_str(),
                    finding.file,
                    finding.line,
                    finding.side.as_graphql(),
                    finding.title,
                    finding.body,
                    finding.suggestion,
                    FindingState::New.as_str(),
                ])?;
                ids.push(tx.last_insert_rowid());
            }
        }
        tx.commit()?;
        Ok(ids)
    }

    pub fn findings(&self, session_id: &str) -> Result<Vec<StoredFinding>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, session_id, severity, path, line, side, title, body, suggestion, state
             FROM findings WHERE session_id = ?1 ORDER BY id",
        )?;
        let rows = stmt
            .query_map(params![session_id], finding_from_row)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn set_finding_state(&mut self, finding_id: i64, state: FindingState) -> Result<()> {
        self.conn.execute(
            "UPDATE findings SET state = ?1 WHERE id = ?2",
            params![state.as_str(), finding_id],
        )?;
        Ok(())
    }
}

#[cfg(test)]
#[path = "agents_tests.rs"]
mod tests;
