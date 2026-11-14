use std::collections::HashMap;

use argon_core::PrKey;
use chrono::{DateTime, Utc};
use rusqlite::params;
use serde::de::DeserializeOwned;

use crate::Store;
use crate::error::Result;

const MARK_SEEN: &str =
    "INSERT INTO pr_local (host, pr_id, last_seen_updated_at) VALUES (?1, ?2, ?3)
     ON CONFLICT(host, pr_id) DO UPDATE
     SET last_seen_updated_at = excluded.last_seen_updated_at";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalState {
    pub unread: bool,
    pub archived: bool,
    pub last_seen: Option<DateTime<Utc>>,
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
            last_seen: last_seen_updated_at.and_then(DateTime::from_timestamp_millis),
        }
    }
}

impl Store {
    pub(crate) fn list_with_local<T: DeserializeOwned>(
        &self,
        table: &'static str,
    ) -> Result<Vec<(T, LocalState)>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT p.data, p.updated_at, l.last_seen_updated_at, l.archived_at
             FROM {table} p
             LEFT JOIN pr_local l ON l.host = p.host AND l.pr_id = p.id
             ORDER BY p.updated_at DESC, p.host ASC, p.id ASC"
        ))?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, Option<i64>>(2)?,
                    row.get::<_, Option<i64>>(3)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        rows.into_iter()
            .map(|(data, updated_at, last_seen, archived_at)| {
                let item = serde_json::from_str(&data)?;
                Ok((item, LocalState::derive(updated_at, last_seen, archived_at)))
            })
            .collect()
    }

    pub fn mark_seen(&mut self, key: &PrKey, updated_at: DateTime<Utc>) -> Result<()> {
        self.conn.execute(
            MARK_SEEN,
            params![key.host, key.id.0, updated_at.timestamp_millis()],
        )?;
        Ok(())
    }

    pub fn mark_seen_all(&mut self, host: &str, seen: &[(String, DateTime<Utc>)]) -> Result<()> {
        let tx = self.conn.transaction()?;
        {
            let mut stmt = tx.prepare(MARK_SEEN)?;
            for (id, updated_at) in seen {
                stmt.execute(params![host, id, updated_at.timestamp_millis()])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn last_seen_map(&self, host: &str) -> Result<HashMap<String, DateTime<Utc>>> {
        let mut stmt = self.conn.prepare(
            "SELECT pr_id, last_seen_updated_at FROM pr_local
             WHERE host = ?1 AND last_seen_updated_at IS NOT NULL",
        )?;
        let rows = stmt
            .query_map([host], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows
            .into_iter()
            .filter_map(|(id, ms)| Some((id, DateTime::from_timestamp_millis(ms)?)))
            .collect())
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
            .upsert_prs(DEFAULT_HOST, &[pr_on(DEFAULT_HOST, id, updated_at)])
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
            .upsert_prs(DEFAULT_HOST, &[pr_on(DEFAULT_HOST, "a", days_ago(1))])
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
            .upsert_prs(DEFAULT_HOST, &[pr_on(DEFAULT_HOST, "a", days_ago(1))])
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
