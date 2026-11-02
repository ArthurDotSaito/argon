use argon_core::PrId;
use chrono::{DateTime, Utc};
use rusqlite::params;

use crate::Store;
use crate::error::Result;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalState {
    pub unread: bool,
    pub archived: bool,
}

impl LocalState {
    pub(crate) fn derive(
        updated_at: i64,
        last_seen_updated_at: Option<i64>,
        archived_at: Option<i64>,
    ) -> LocalState {
        LocalState {
            unread: last_seen_updated_at.is_none_or(|seen| updated_at > seen),
            archived: archived_at.is_some_and(|at| updated_at <= at),
        }
    }
}

impl Store {
    pub fn mark_seen(&mut self, id: &PrId, updated_at: DateTime<Utc>) -> Result<()> {
        self.conn.execute(
            "INSERT INTO pr_local (pr_id, last_seen_updated_at) VALUES (?1, ?2)
             ON CONFLICT(pr_id) DO UPDATE SET last_seen_updated_at = excluded.last_seen_updated_at",
            params![id.0, updated_at.timestamp_millis()],
        )?;
        Ok(())
    }

    pub fn mark_unread(&mut self, id: &PrId) -> Result<()> {
        self.conn.execute(
            "INSERT INTO pr_local (pr_id, last_seen_updated_at) VALUES (?1, NULL)
             ON CONFLICT(pr_id) DO UPDATE SET last_seen_updated_at = NULL",
            params![id.0],
        )?;
        Ok(())
    }

    pub fn archive(&mut self, id: &PrId, at: DateTime<Utc>) -> Result<()> {
        self.conn.execute(
            "INSERT INTO pr_local (pr_id, archived_at) VALUES (?1, ?2)
             ON CONFLICT(pr_id) DO UPDATE SET archived_at = excluded.archived_at",
            params![id.0, at.timestamp_millis()],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use argon_core::PrId;
    use chrono::{DateTime, Utc};

    use crate::fixtures::{days_ago, pr};
    use crate::{LocalState, Store};

    fn state_of(store: &Store, id: &str) -> LocalState {
        store
            .list_prs()
            .unwrap()
            .into_iter()
            .find(|(p, _)| p.id.0 == id)
            .unwrap()
            .1
    }

    fn store_with(id: &str, updated_at: DateTime<Utc>) -> Store {
        let mut store = Store::open_in_memory().unwrap();
        store
            .replace_prs("github.com", &[pr(id, "github.com", updated_at)])
            .unwrap();
        store
    }

    #[test]
    fn never_seen_pr_is_unread() {
        let store = store_with("a", days_ago(1));
        let state = state_of(&store, "a");
        assert!(state.unread);
        assert!(!state.archived);
    }

    #[test]
    fn update_after_seen_makes_pr_unread_again() {
        let mut store = store_with("a", days_ago(3));
        store.mark_seen(&PrId("a".into()), days_ago(3)).unwrap();
        assert!(!state_of(&store, "a").unread);

        store
            .replace_prs("github.com", &[pr("a", "github.com", days_ago(1))])
            .unwrap();
        assert!(state_of(&store, "a").unread);
    }

    #[test]
    fn mark_unread_clears_last_seen() {
        let mut store = store_with("a", days_ago(1));
        let id = PrId("a".into());
        store.mark_seen(&id, days_ago(1)).unwrap();
        assert!(!state_of(&store, "a").unread);

        store.mark_unread(&id).unwrap();
        assert!(state_of(&store, "a").unread);
    }

    #[test]
    fn archived_pr_returns_with_new_activity() {
        let mut store = store_with("a", days_ago(3));
        store.archive(&PrId("a".into()), days_ago(2)).unwrap();
        assert!(state_of(&store, "a").archived);

        store
            .replace_prs("github.com", &[pr("a", "github.com", days_ago(1))])
            .unwrap();
        assert!(!state_of(&store, "a").archived);
    }

    #[test]
    fn archive_keeps_seen_state_independent() {
        let mut store = store_with("a", days_ago(3));
        let id = PrId("a".into());
        store.mark_seen(&id, days_ago(3)).unwrap();
        store.archive(&id, days_ago(2)).unwrap();
        let state = state_of(&store, "a");
        assert!(state.archived);
        assert!(!state.unread);
    }
}
