use argon_core::ArchGraph;
use chrono::{DateTime, Utc};
use rusqlite::{OptionalExtension, Row, TransactionBehavior, params};

use crate::Store;
use crate::agents::millis;
use crate::error::Result;

const SUMMARY_COLUMNS: &str = "id, host, pr_id, session_id, body, head_sha, touched, created_at";
const OVERVIEW_COLUMNS: &str =
    "host, repo, body, diagram, source_sha, source_pr, session_id, updated_at";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrSummary {
    pub id: i64,
    pub host: String,
    pub pr_id: String,
    pub session_id: String,
    pub body: String,
    pub head_sha: String,
    pub touched: Vec<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProjectOverview {
    pub host: String,
    pub repo: String,
    pub body: String,
    pub diagram: Option<ArchGraph>,
    pub source_sha: String,
    pub source_pr: u64,
    pub session_id: String,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NewProject<'a> {
    pub body: &'a str,
    pub diagram: Option<&'a ArchGraph>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NewSummary<'a> {
    pub session_id: &'a str,
    pub host: &'a str,
    pub pr_id: &'a str,
    pub repo: &'a str,
    pub pr_number: u64,
    pub body: &'a str,
    pub head_sha: &'a str,
    pub touched: &'a [String],
    pub project: Option<NewProject<'a>>,
}

fn summary_from_row(row: &Row<'_>) -> rusqlite::Result<PrSummary> {
    let touched: Option<String> = row.get(6)?;
    Ok(PrSummary {
        id: row.get(0)?,
        host: row.get(1)?,
        pr_id: row.get(2)?,
        session_id: row.get(3)?,
        body: row.get(4)?,
        head_sha: row.get(5)?,
        touched: touched
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default(),
        created_at: millis(7, row.get(7)?)?,
    })
}

fn overview_from_row(row: &Row<'_>) -> rusqlite::Result<ProjectOverview> {
    let diagram: Option<String> = row.get(3)?;
    Ok(ProjectOverview {
        host: row.get(0)?,
        repo: row.get(1)?,
        body: row.get(2)?,
        diagram: diagram.and_then(|json| serde_json::from_str(&json).ok()),
        source_sha: row.get(4)?,
        source_pr: row.get::<_, i64>(5)?.try_into().unwrap_or_default(),
        session_id: row.get(6)?,
        updated_at: millis(7, row.get(7)?)?,
    })
}

impl Store {
    pub fn record_summary(&mut self, new: NewSummary<'_>) -> Result<PrSummary> {
        let now = Utc::now().timestamp_millis();
        let touched = serde_json::to_string(new.touched)?;
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "INSERT INTO pr_summaries
                (host, pr_id, session_id, body, head_sha, touched, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                new.host,
                new.pr_id,
                new.session_id,
                new.body,
                new.head_sha,
                touched,
                now
            ],
        )?;
        let id = tx.last_insert_rowid();
        if let Some(project) = new.project {
            let diagram = project.diagram.map(serde_json::to_string).transpose()?;
            tx.execute(
                "INSERT INTO project_overviews
                    (host, repo, body, diagram, source_sha, source_pr, session_id, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                 ON CONFLICT(host, repo) DO UPDATE SET
                    body = excluded.body,
                    diagram = excluded.diagram,
                    source_sha = excluded.source_sha,
                    source_pr = excluded.source_pr,
                    session_id = excluded.session_id,
                    updated_at = excluded.updated_at",
                params![
                    new.host,
                    new.repo,
                    project.body,
                    diagram,
                    new.head_sha,
                    i64::try_from(new.pr_number).unwrap_or(i64::MAX),
                    new.session_id,
                    now
                ],
            )?;
        }
        let summary = tx.query_row(
            &format!("SELECT {SUMMARY_COLUMNS} FROM pr_summaries WHERE id = ?1"),
            params![id],
            summary_from_row,
        )?;
        tx.commit()?;
        Ok(summary)
    }

    pub fn latest_summary_for_pr(&self, host: &str, pr_id: &str) -> Result<Option<PrSummary>> {
        let row = self
            .conn
            .query_row(
                &format!(
                    "SELECT {SUMMARY_COLUMNS} FROM pr_summaries
                     WHERE host = ?1 AND pr_id = ?2
                     ORDER BY id DESC LIMIT 1"
                ),
                params![host, pr_id],
                summary_from_row,
            )
            .optional()?;
        Ok(row)
    }

    pub fn summaries_for_session(&self, session_id: &str) -> Result<Vec<PrSummary>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {SUMMARY_COLUMNS} FROM pr_summaries
             WHERE session_id = ?1
             ORDER BY id"
        ))?;
        let rows = stmt
            .query_map(params![session_id], summary_from_row)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn project_overview(&self, host: &str, repo: &str) -> Result<Option<ProjectOverview>> {
        let row = self
            .conn
            .query_row(
                &format!(
                    "SELECT {OVERVIEW_COLUMNS} FROM project_overviews
                     WHERE host = ?1 AND repo = ?2"
                ),
                params![host, repo],
                overview_from_row,
            )
            .optional()?;
        Ok(row)
    }

    pub fn project_keys(&self) -> Result<Vec<(String, String)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT host, repo FROM project_overviews ORDER BY repo, host")?;
        let rows = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn project_overviews(&self) -> Result<Vec<ProjectOverview>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {OVERVIEW_COLUMNS} FROM project_overviews ORDER BY repo, host"
        ))?;
        let rows = stmt
            .query_map([], overview_from_row)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }
}

#[cfg(test)]
#[path = "summaries_tests.rs"]
mod tests;
