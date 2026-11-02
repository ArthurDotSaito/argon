use argon_core::PrKey;
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
    pub fn mark_seen(&mut self, key: &PrKey, updated_at: DateTime<Utc>) -> Result<()> {
        self.conn.execute(
            "INSERT INTO pr_local (host, pr_id, last_seen_updated_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(host, pr_id) DO UPDATE
             SET last_seen_updated_at = excluded.last_seen_updated_at",
            params![key.host, key.id.0, updated_at.timestamp_millis()],
        )?;
        Ok(())
    }

    pub fn mark_unread(&mut self, key: &PrKey) -> Result<()> {
        self.conn.execute(
            "INSERT INTO pr_local (host, pr_id, last_seen_updated_at) VALUES (?1, ?2, NULL)
             ON CONFLICT(host, pr_id) DO UPDATE SET last_seen_updated_at = NULL",
            params![key.host, key.id.0],
        )?;
        Ok(())
    }

    pub fn archive(&mut self, key: &PrKey, at: DateTime<Utc>) -> Result<()> {
        self.conn.execute(
            "INSERT INTO pr_local (host, pr_id, archived_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(host, pr_id) DO UPDATE SET archived_at = excluded.archived_at",
            params![key.host, key.id.0, at.timestamp_millis()],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use argon_core::fixtures::{days_ago, pr_on};
    use argon_core::{DEFAULT_HOST, PrKey};
    use chrono::{DateTime, Utc};

    use crate::{LocalState, Store};

    fn key(id: &str) -> PrKey {
        PrKey::new(DEFAULT_HOST, id)
    }

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
            .replace_prs(DEFAULT_HOST, &[pr_on(DEFAULT_HOST, id, updated_at)])
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
        store.mark_seen(&key("a"), days_ago(3)).unwrap();
        assert!(!state_of(&store, "a").unread);

        store
            .replace_prs(DEFAULT_HOST, &[pr_on(DEFAULT_HOST, "a", days_ago(1))])
            .unwrap();
        assert!(state_of(&store, "a").unread);
    }

    #[test]
    fn mark_unread_clears_last_seen() {
        let mut store = store_with("a", days_ago(1));
        let id = key("a");
        store.mark_seen(&id, days_ago(1)).unwrap();
        assert!(!state_of(&store, "a").unread);

        store.mark_unread(&id).unwrap();
        assert!(state_of(&store, "a").unread);
    }

    #[test]
    fn archived_pr_returns_with_new_activity() {
        let mut store = store_with("a", days_ago(3));
        store.archive(&key("a"), days_ago(2)).unwrap();
        assert!(state_of(&store, "a").archived);

        store
            .replace_prs(DEFAULT_HOST, &[pr_on(DEFAULT_HOST, "a", days_ago(1))])
            .unwrap();
        assert!(!state_of(&store, "a").archived);
    }

    #[test]
    fn archive_keeps_seen_state_independent() {
        let mut store = store_with("a", days_ago(3));
        let id = key("a");
        store.mark_seen(&id, days_ago(3)).unwrap();
        store.archive(&id, days_ago(2)).unwrap();
        let state = state_of(&store, "a");
        assert!(state.archived);
        assert!(!state.unread);
    }
}
