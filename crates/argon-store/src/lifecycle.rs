use std::path::PathBuf;

use argon_core::DraftComment;
use chrono::{DateTime, Utc};
use rusqlite::params;

use crate::Store;
use crate::agents::{FindingState, SessionStatus};
use crate::drafts::insert_draft;
use crate::error::Result;

pub const ARCHIVE_PENDING: &str = "arquivamento pendente: o worktree não pôde ser removido";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaleWorktree {
    pub session_id: String,
    pub path: PathBuf,
    pub archive: bool,
}

impl Store {
    pub fn mark_archive_pending(&mut self, id: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE agent_sessions SET status = ?1, failure_reason = ?2, updated_at = ?3
             WHERE id = ?4 AND status <> ?5",
            params![
                SessionStatus::Failed.as_str(),
                ARCHIVE_PENDING,
                Utc::now().timestamp_millis(),
                id,
                SessionStatus::Archived.as_str()
            ],
        )?;
        Ok(())
    }

    pub fn worktree_paths(&self) -> Result<Vec<PathBuf>> {
        let mut stmt = self
            .conn
            .prepare("SELECT worktree_path FROM agent_sessions WHERE worktree_path IS NOT NULL")?;
        let rows = stmt
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows.into_iter().map(PathBuf::from).collect())
    }

    pub fn set_session_model_label(&mut self, id: &str, label: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE agent_sessions SET model_label = ?1 WHERE id = ?2",
            params![label, id],
        )?;
        Ok(())
    }

    pub fn fail_orphaned_sessions(&mut self, reason: &str) -> Result<Vec<String>> {
        let tx = self.conn.transaction()?;
        let ids = {
            let mut stmt = tx.prepare("SELECT id FROM agent_sessions WHERE status IN (?1, ?2)")?;
            stmt.query_map(
                params![
                    SessionStatus::Running.as_str(),
                    SessionStatus::AwaitingUser.as_str()
                ],
                |row| row.get::<_, String>(0),
            )?
            .collect::<std::result::Result<Vec<_>, _>>()?
        };
        {
            let mut update = tx.prepare(
                "UPDATE agent_sessions SET status = ?1, failure_reason = ?2, updated_at = ?3
                 WHERE id = ?4",
            )?;
            let now = Utc::now().timestamp_millis();
            for id in &ids {
                update.execute(params![SessionStatus::Failed.as_str(), reason, now, id])?;
            }
        }
        tx.commit()?;
        Ok(ids)
    }

    pub fn note_failure_reason(&mut self, ids: &[String], reason: &str) -> Result<()> {
        let tx = self.conn.transaction()?;
        {
            let mut update = tx.prepare(
                "UPDATE agent_sessions SET failure_reason = ?1
                 WHERE id = ?2 AND status = ?3 AND failure_reason IS NULL",
            )?;
            for id in ids {
                update.execute(params![reason, id, SessionStatus::Failed.as_str()])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn stale_worktrees(&self, before: DateTime<Utc>) -> Result<Vec<StaleWorktree>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, worktree_path, failure_reason IS ?4 FROM agent_sessions
             WHERE status IN (?1, ?2) AND worktree_path IS NOT NULL
               AND (updated_at < ?3 OR failure_reason IS ?4)
             ORDER BY updated_at",
        )?;
        let rows = stmt
            .query_map(
                params![
                    SessionStatus::Completed.as_str(),
                    SessionStatus::Failed.as_str(),
                    before.timestamp_millis(),
                    ARCHIVE_PENDING
                ],
                |row| {
                    Ok(StaleWorktree {
                        session_id: row.get(0)?,
                        path: PathBuf::from(row.get::<_, String>(1)?),
                        archive: row.get(2)?,
                    })
                },
            )?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn add_agent_draft(&mut self, draft: &DraftComment, finding_id: i64) -> Result<i64> {
        let tx = self.conn.transaction()?;
        let id = insert_draft(&tx, draft, Some(finding_id))?;
        tx.execute(
            "UPDATE findings SET state = ?1 WHERE id = ?2",
            params![FindingState::Added.as_str(), finding_id],
        )?;
        tx.commit()?;
        Ok(id)
    }
}

#[cfg(test)]
mod tests {
    use argon_core::{DraftSource, Finding, PrKey, Severity, Side};
    use chrono::TimeDelta;

    use super::*;
    use crate::agents::tests::new_session;

    fn finding() -> Finding {
        Finding {
            severity: Severity::Medium,
            file: "src/a.rs".into(),
            line: 4,
            side: Side::Right,
            title: "t".into(),
            body: "b".into(),
            suggestion: None,
        }
    }

    fn draft(pr: &PrKey) -> DraftComment {
        DraftComment {
            id: 0,
            pr: pr.clone(),
            path: "src/a.rs".into(),
            line: 4,
            side: Side::Right,
            start_line: None,
            start_side: None,
            body: "**t**".into(),
            source: DraftSource::Agent,
            session_id: Some("s1".into()),
            head_oid: None,
        }
    }

    fn state(store: &Store, id: i64) -> FindingState {
        store
            .findings("s1")
            .unwrap()
            .into_iter()
            .find(|stored| stored.id == id)
            .unwrap()
            .state
    }

    fn store_with_finding() -> (Store, PrKey, i64) {
        let mut store = Store::open_in_memory().unwrap();
        let session = new_session("s1");
        store.create_session(&session).unwrap();
        let id = store.save_findings("s1", &[finding()]).unwrap()[0].id;
        (store, session.pr, id)
    }

    #[test]
    fn deleting_an_agent_draft_reopens_its_finding() {
        let (mut store, pr, finding) = store_with_finding();
        let draft_id = store.add_agent_draft(&draft(&pr), finding).unwrap();
        assert_eq!(state(&store, finding), FindingState::Added);
        store.delete_draft(draft_id).unwrap();
        assert_eq!(state(&store, finding), FindingState::New);
        assert!(store.drafts(&pr).unwrap().is_empty());
    }

    #[test]
    fn submitted_drafts_keep_their_findings_added_but_discard_reopens() {
        let (mut store, pr, finding) = store_with_finding();
        let sent = store.add_agent_draft(&draft(&pr), finding).unwrap();
        store.delete_drafts(&pr, &[sent]).unwrap();
        assert_eq!(state(&store, finding), FindingState::Added);
        store.add_agent_draft(&draft(&pr), finding).unwrap();
        store.clear_review(&pr).unwrap();
        assert_eq!(state(&store, finding), FindingState::New);
    }

    #[test]
    fn orphaned_sessions_fail_with_reason_and_resume_clears_it() {
        let mut store = Store::open_in_memory().unwrap();
        for id in ["run", "wait", "done"] {
            store.create_session(&new_session(id)).unwrap();
        }
        store
            .set_session_status("wait", SessionStatus::AwaitingUser)
            .unwrap();
        store
            .set_session_status("done", SessionStatus::Completed)
            .unwrap();
        let mut failed = store.fail_orphaned_sessions("encerrado").unwrap();
        failed.sort();
        assert_eq!(failed, ["run", "wait"]);
        let row = store.session("run").unwrap().unwrap();
        assert_eq!(row.status, SessionStatus::Failed);
        assert_eq!(row.failure_reason.as_deref(), Some("encerrado"));
        assert_eq!(store.session("done").unwrap().unwrap().failure_reason, None);
        store
            .set_session_status("run", SessionStatus::Completed)
            .unwrap();
        assert_eq!(store.session("run").unwrap().unwrap().failure_reason, None);
    }

    #[test]
    fn failure_reason_is_noted_only_on_failed_sessions_without_one() {
        let mut store = Store::open_in_memory().unwrap();
        for id in ["failed", "done"] {
            store.create_session(&new_session(id)).unwrap();
        }
        store
            .set_session_status("failed", SessionStatus::Failed)
            .unwrap();
        store
            .set_session_status("done", SessionStatus::Completed)
            .unwrap();
        let ids = ["failed".to_owned(), "done".to_owned()];
        store.note_failure_reason(&ids, "fechado").unwrap();
        store.note_failure_reason(&ids, "outro").unwrap();
        let reason = |id| store.session(id).unwrap().unwrap().failure_reason;
        assert_eq!(reason("failed").as_deref(), Some("fechado"));
        assert_eq!(reason("done"), None);
    }

    #[test]
    fn stale_worktrees_only_lists_finished_sessions_before_the_cutoff() {
        let mut store = Store::open_in_memory().unwrap();
        for id in ["old", "running", "archived"] {
            store.create_session(&new_session(id)).unwrap();
        }
        store
            .set_session_status("old", SessionStatus::Completed)
            .unwrap();
        store
            .set_session_status("archived", SessionStatus::Archived)
            .unwrap();
        let cutoff = Utc::now() + TimeDelta::seconds(5);
        let stale = store.stale_worktrees(cutoff).unwrap();
        assert_eq!(stale.len(), 1);
        assert_eq!(stale[0].session_id, "old");
        assert!(!stale[0].archive);
        assert!(
            store
                .stale_worktrees(Utc::now() - TimeDelta::days(7))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn archived_is_terminal_for_late_status_writes() {
        let mut store = Store::open_in_memory().unwrap();
        store.create_session(&new_session("s1")).unwrap();
        store
            .set_session_status("s1", SessionStatus::Archived)
            .unwrap();
        store
            .set_session_status("s1", SessionStatus::Completed)
            .unwrap();
        store
            .append_batch(&[crate::SessionWrite {
                session_id: "s1".into(),
                status: Some(SessionStatus::Running),
                ..Default::default()
            }])
            .unwrap();
        store.mark_archive_pending("s1").unwrap();
        let row = store.session("s1").unwrap().unwrap();
        assert_eq!(row.status, SessionStatus::Archived);
        assert_eq!(row.failure_reason, None);
    }

    #[test]
    fn pending_archives_are_retried_regardless_of_age() {
        let mut store = Store::open_in_memory().unwrap();
        store.create_session(&new_session("s1")).unwrap();
        store.mark_archive_pending("s1").unwrap();
        let stale = store
            .stale_worktrees(Utc::now() - TimeDelta::days(7))
            .unwrap();
        assert_eq!(stale.len(), 1);
        assert!(stale[0].archive);
        assert_eq!(store.worktree_paths().unwrap().len(), 1);
    }

    #[test]
    fn model_label_is_stored() {
        let mut store = Store::open_in_memory().unwrap();
        store.create_session(&new_session("s1")).unwrap();
        store.set_session_model_label("s1", "Sonnet 5.5").unwrap();
        assert_eq!(
            store.session("s1").unwrap().unwrap().model_label.as_deref(),
            Some("Sonnet 5.5")
        );
    }
}
