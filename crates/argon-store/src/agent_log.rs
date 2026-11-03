use std::time::Duration;

use argon_core::{Finding, Severity, Side};
use chrono::Utc;
use rusqlite::{OptionalExtension, Row, Transaction, params};
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::Store;
use crate::agents::{FindingState, SessionStatus, StoredFinding, invalid};
use crate::error::Result;

const BUSY_TIMEOUT: Duration = Duration::from_secs(5);
const FINDING_COLUMNS: &str =
    "id, session_id, severity, path, line, side, title, body, suggestion, state";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionWrite {
    pub session_id: String,
    pub events: Vec<(i64, String)>,
    pub status: Option<SessionStatus>,
    pub model: Option<String>,
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

fn apply_write(tx: &Transaction<'_>, write: &SessionWrite, now: i64) -> Result<()> {
    {
        let mut insert = tx.prepare_cached(
            "INSERT INTO agent_events (session_id, seq, data) VALUES (?1, ?2, ?3)",
        )?;
        for (seq, data) in &write.events {
            insert.execute(params![write.session_id, seq, data])?;
        }
    }
    if let Some(status) = write.status {
        tx.execute(
            "UPDATE agent_sessions SET status = ?1 WHERE id = ?2",
            params![status.as_str(), write.session_id],
        )?;
    }
    if let Some(model) = &write.model {
        tx.execute(
            "UPDATE agent_sessions SET model = ?1 WHERE id = ?2",
            params![model, write.session_id],
        )?;
    }
    tx.execute(
        "UPDATE agent_sessions SET updated_at = ?1 WHERE id = ?2",
        params![now, write.session_id],
    )?;
    Ok(())
}

impl Store {
    pub fn tune_for_background_writes(&self) -> Result<()> {
        self.conn.pragma_update(None, "synchronous", "NORMAL")?;
        self.conn.busy_timeout(BUSY_TIMEOUT)?;
        Ok(())
    }

    pub fn append_batch(&mut self, writes: &[SessionWrite]) -> Result<()> {
        let now = Utc::now().timestamp_millis();
        let tx = self.conn.transaction()?;
        for write in writes {
            apply_write(&tx, write, now)?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn append_event<E: Serialize>(&mut self, id: &str, event: &E) -> Result<i64> {
        let seq = self.last_event_seq(id)? + 1;
        self.append_batch(&[SessionWrite {
            session_id: id.to_owned(),
            events: vec![(seq, serde_json::to_string(event)?)],
            ..SessionWrite::default()
        }])?;
        Ok(seq)
    }

    pub fn last_event_seq(&self, id: &str) -> Result<i64> {
        Ok(self.conn.query_row(
            "SELECT COALESCE(MAX(seq), 0) FROM agent_events WHERE session_id = ?1",
            params![id],
            |row| row.get(0),
        )?)
    }

    pub fn events<E: DeserializeOwned>(&self, id: &str) -> Result<Vec<E>> {
        Ok(self
            .events_since(id, 0)?
            .into_iter()
            .map(|(_, event)| event)
            .collect())
    }

    pub fn events_since<E: DeserializeOwned>(&self, id: &str, after: i64) -> Result<Vec<(i64, E)>> {
        let mut stmt = self.conn.prepare(
            "SELECT seq, data FROM agent_events WHERE session_id = ?1 AND seq > ?2 ORDER BY seq",
        )?;
        let rows = stmt
            .query_map(params![id, after], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        rows.into_iter()
            .map(|(seq, data)| Ok((seq, serde_json::from_str(&data)?)))
            .collect()
    }

    pub fn save_findings(&mut self, id: &str, findings: &[Finding]) -> Result<Vec<StoredFinding>> {
        let tx = self.conn.transaction()?;
        let mut saved = Vec::with_capacity(findings.len());
        for finding in findings {
            let existing = tx
                .query_row(
                    &format!(
                        "SELECT {FINDING_COLUMNS} FROM findings
                         WHERE session_id = ?1 AND path = ?2 AND line = ?3 AND title = ?4"
                    ),
                    params![id, finding.file, finding.line, finding.title],
                    finding_from_row,
                )
                .optional()?;
            if let Some(existing) = existing {
                saved.push(existing);
                continue;
            }
            tx.execute(
                "INSERT INTO findings
                    (session_id, severity, path, line, side, title, body, suggestion, state)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    id,
                    finding.severity.as_str(),
                    finding.file,
                    finding.line,
                    finding.side.as_graphql(),
                    finding.title,
                    finding.body,
                    finding.suggestion,
                    FindingState::New.as_str(),
                ],
            )?;
            saved.push(StoredFinding {
                id: tx.last_insert_rowid(),
                session_id: id.to_owned(),
                finding: finding.clone(),
                state: FindingState::New,
            });
        }
        tx.commit()?;
        Ok(saved)
    }

    pub fn findings(&self, session_id: &str) -> Result<Vec<StoredFinding>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {FINDING_COLUMNS} FROM findings WHERE session_id = ?1 ORDER BY id"
        ))?;
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
#[path = "agent_log_tests.rs"]
mod tests;
