use chrono::{DateTime, Utc};
use rusqlite::{OptionalExtension, Row, TransactionBehavior, params};

use crate::Store;
use crate::agents::millis;
use crate::error::{Result, StoreError};

const PLAN_COLUMNS: &str = "id, host, issue_id, session_id, body, base_sha, agent_body, edited, \
     created_at, updated_at, published_at, comment_id, comment_url";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanDraft {
    pub id: i64,
    pub host: String,
    pub issue_id: String,
    pub session_id: String,
    pub body: String,
    pub base_sha: String,
    pub agent_body: Option<String>,
    pub edited: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub published_at: Option<DateTime<Utc>>,
    pub comment_id: Option<String>,
    pub comment_url: Option<String>,
}

fn plan_from_row(row: &Row<'_>) -> rusqlite::Result<PlanDraft> {
    let published_at: Option<i64> = row.get(10)?;
    Ok(PlanDraft {
        id: row.get(0)?,
        host: row.get(1)?,
        issue_id: row.get(2)?,
        session_id: row.get(3)?,
        body: row.get(4)?,
        base_sha: row.get(5)?,
        agent_body: row.get(6)?,
        edited: row.get::<_, i64>(7)? != 0,
        created_at: millis(8, row.get(8)?)?,
        updated_at: millis(9, row.get(9)?)?,
        published_at: published_at.map(|value| millis(10, value)).transpose()?,
        comment_id: row.get(11)?,
        comment_url: row.get(12)?,
    })
}

fn plan_by_id(conn: &rusqlite::Connection, id: i64) -> rusqlite::Result<PlanDraft> {
    conn.query_row(
        &format!("SELECT {PLAN_COLUMNS} FROM plan_drafts WHERE id = ?1"),
        params![id],
        plan_from_row,
    )
}

fn open_plan_id(conn: &rusqlite::Connection, session_id: &str) -> rusqlite::Result<Option<i64>> {
    conn.query_row(
        "SELECT id FROM plan_drafts
         WHERE session_id = ?1 AND published_at IS NULL
         ORDER BY id DESC LIMIT 1",
        params![session_id],
        |row| row.get(0),
    )
    .optional()
}

impl Store {
    pub fn record_plan(
        &mut self,
        host: &str,
        issue_id: &str,
        session_id: &str,
        body: &str,
        base_sha: &str,
    ) -> Result<PlanDraft> {
        let now = Utc::now().timestamp_millis();
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let id = match open_plan_id(&tx, session_id)? {
            Some(id) => {
                let changed = tx.execute(
                    "UPDATE plan_drafts SET
                         body = CASE WHEN edited = 0 THEN ?1 ELSE body END,
                         base_sha = CASE WHEN edited = 0 THEN ?2 ELSE base_sha END,
                         agent_body = CASE WHEN edited = 0 THEN agent_body ELSE ?1 END,
                         updated_at = ?3
                     WHERE id = ?4 AND published_at IS NULL",
                    params![body, base_sha, now, id],
                )?;
                if changed == 0 {
                    ensure_open(&tx, id)?;
                }
                id
            }
            None => {
                tx.execute(
                    "INSERT INTO plan_drafts
                        (host, issue_id, session_id, body, base_sha, created_at, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
                    params![host, issue_id, session_id, body, base_sha, now],
                )?;
                tx.last_insert_rowid()
            }
        };
        let plan = plan_by_id(&tx, id)?;
        tx.commit()?;
        Ok(plan)
    }

    pub fn edit_plan(&mut self, id: i64, body: &str) -> Result<()> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let changed = tx.execute(
            "UPDATE plan_drafts SET body = ?1, edited = 1, updated_at = ?2
             WHERE id = ?3 AND published_at IS NULL",
            params![body, Utc::now().timestamp_millis(), id],
        )?;
        if changed == 0 {
            ensure_open(&tx, id)?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn resolve_agent_version(&mut self, id: i64, use_agent: bool) -> Result<()> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let now = Utc::now().timestamp_millis();
        let changed = if use_agent {
            tx.execute(
                "UPDATE plan_drafts
                 SET body = agent_body, edited = 0, agent_body = NULL, updated_at = ?1
                 WHERE id = ?2 AND published_at IS NULL AND agent_body IS NOT NULL",
                params![now, id],
            )?
        } else {
            tx.execute(
                "UPDATE plan_drafts SET agent_body = NULL, updated_at = ?1
                 WHERE id = ?2 AND published_at IS NULL",
                params![now, id],
            )?
        };
        if changed == 0 {
            ensure_open(&tx, id)?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn reserve_plan_publish(
        &mut self,
        id: i64,
        body_sent: &str,
        at: DateTime<Utc>,
    ) -> Result<bool> {
        let changed = self.conn.execute(
            "UPDATE plan_drafts SET published_at = ?1
             WHERE id = ?2 AND published_at IS NULL AND body = ?3 AND agent_body IS NULL",
            params![at.timestamp_millis(), id, body_sent],
        )?;
        Ok(changed == 1)
    }

    pub fn complete_plan_publish(
        &mut self,
        id: i64,
        comment_id: &str,
        comment_url: &str,
    ) -> Result<()> {
        let changed = self.conn.execute(
            "UPDATE plan_drafts SET comment_id = ?1, comment_url = ?2
             WHERE id = ?3 AND published_at IS NOT NULL",
            params![comment_id, comment_url, id],
        )?;
        if changed == 0 {
            plan_by_id(&self.conn, id)?;
            return Err(StoreError::PlanNotReserved(id));
        }
        Ok(())
    }

    pub fn release_plan_publish(&mut self, id: i64) -> Result<()> {
        self.conn.execute(
            "UPDATE plan_drafts SET published_at = NULL
             WHERE id = ?1 AND comment_id IS NULL",
            params![id],
        )?;
        Ok(())
    }

    pub fn plans_for_issue(&self, host: &str, issue_id: &str) -> Result<Vec<PlanDraft>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {PLAN_COLUMNS} FROM plan_drafts
             WHERE host = ?1 AND issue_id = ?2
             ORDER BY id DESC"
        ))?;
        let rows = stmt
            .query_map(params![host, issue_id], plan_from_row)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn plans_for_session(&self, session_id: &str) -> Result<Vec<PlanDraft>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {PLAN_COLUMNS} FROM plan_drafts
             WHERE session_id = ?1
             ORDER BY id DESC"
        ))?;
        let rows = stmt
            .query_map(params![session_id], plan_from_row)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }
}

fn ensure_open(conn: &rusqlite::Connection, id: i64) -> Result<()> {
    if plan_by_id(conn, id)?.published_at.is_some() {
        return Err(StoreError::PlanPublished(id));
    }
    Ok(())
}

#[cfg(test)]
#[path = "plans_tests.rs"]
mod tests;
