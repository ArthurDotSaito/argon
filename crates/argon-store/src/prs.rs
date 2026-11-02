use argon_core::PullRequest;
use chrono::Utc;
use rusqlite::params;

use crate::Store;
use crate::error::Result;
use crate::local::LocalState;

impl Store {
    pub fn replace_prs(&mut self, host: &str, prs: &[PullRequest]) -> Result<()> {
        let synced_at = Utc::now().timestamp_millis();
        let kept_ids = serde_json::to_string(&prs.iter().map(|pr| &pr.id.0).collect::<Vec<_>>())?;
        let tx = self.conn.transaction()?;
        {
            let mut upsert = tx.prepare(
                "INSERT INTO pull_requests (id, host, repo, number, data, updated_at, synced_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(id) DO UPDATE SET
                    host = excluded.host,
                    repo = excluded.repo,
                    number = excluded.number,
                    data = excluded.data,
                    updated_at = excluded.updated_at,
                    synced_at = excluded.synced_at",
            )?;
            for pr in prs {
                let number = i64::try_from(pr.number)
                    .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
                upsert.execute(params![
                    pr.id.0,
                    host,
                    pr.repo,
                    number,
                    serde_json::to_string(pr)?,
                    pr.updated_at.timestamp_millis(),
                    synced_at,
                ])?;
            }
        }
        tx.execute(
            "DELETE FROM pull_requests
             WHERE host = ?1 AND id NOT IN (SELECT value FROM json_each(?2))",
            params![host, kept_ids],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn list_prs(&self) -> Result<Vec<(PullRequest, LocalState)>> {
        let mut stmt = self.conn.prepare(
            "SELECT p.data, p.updated_at, l.last_seen_updated_at, l.archived_at
             FROM pull_requests p
             LEFT JOIN pr_local l ON l.pr_id = p.id
             ORDER BY p.updated_at DESC, p.id ASC",
        )?;
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
                let pr = serde_json::from_str(&data)?;
                Ok((pr, LocalState::derive(updated_at, last_seen, archived_at)))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use crate::Store;
    use crate::fixtures::{days_ago, pr};

    fn ids(store: &Store) -> Vec<String> {
        store
            .list_prs()
            .unwrap()
            .into_iter()
            .map(|(p, _)| p.id.0)
            .collect()
    }

    #[test]
    fn upsert_and_list_orders_by_updated_at_desc() {
        let mut store = Store::open_in_memory().unwrap();
        let old = pr("old", "github.com", days_ago(5));
        let newest = pr("newest", "github.com", days_ago(1));
        let mid = pr("mid", "github.com", days_ago(3));
        store
            .replace_prs("github.com", &[old, newest.clone(), mid])
            .unwrap();
        assert_eq!(ids(&store), ["newest", "mid", "old"]);

        let mut updated = newest;
        updated.title = "Renamed".into();
        store
            .replace_prs("github.com", std::slice::from_ref(&updated))
            .unwrap();
        let listed = store.list_prs().unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].0, updated);
    }

    #[test]
    fn replace_prs_removes_prs_that_left_the_window() {
        let mut store = Store::open_in_memory().unwrap();
        store
            .replace_prs(
                "github.com",
                &[
                    pr("a", "github.com", days_ago(2)),
                    pr("b", "github.com", days_ago(1)),
                ],
            )
            .unwrap();
        store
            .replace_prs("github.com", &[pr("b", "github.com", days_ago(1))])
            .unwrap();
        assert_eq!(ids(&store), ["b"]);
    }

    #[test]
    fn replace_prs_with_empty_slice_clears_only_that_host() {
        let mut store = Store::open_in_memory().unwrap();
        store
            .replace_prs("github.com", &[pr("a", "github.com", days_ago(2))])
            .unwrap();
        store
            .replace_prs("ghe.acme.io", &[pr("b", "ghe.acme.io", days_ago(1))])
            .unwrap();
        store.replace_prs("github.com", &[]).unwrap();
        assert_eq!(ids(&store), ["b"]);
    }

    #[test]
    fn replace_prs_does_not_affect_other_hosts() {
        let mut store = Store::open_in_memory().unwrap();
        store
            .replace_prs("github.com", &[pr("a", "github.com", days_ago(2))])
            .unwrap();
        store
            .replace_prs("ghe.acme.io", &[pr("b", "ghe.acme.io", days_ago(1))])
            .unwrap();
        store
            .replace_prs("github.com", &[pr("c", "github.com", days_ago(3))])
            .unwrap();
        assert_eq!(ids(&store), ["b", "c"]);
    }

    #[test]
    fn replace_prs_keeps_local_state_of_removed_prs() {
        let mut store = Store::open_in_memory().unwrap();
        let first = pr("a", "github.com", days_ago(2));
        store
            .replace_prs("github.com", std::slice::from_ref(&first))
            .unwrap();
        store.mark_seen(&first.id, first.updated_at).unwrap();
        store.replace_prs("github.com", &[]).unwrap();
        store.replace_prs("github.com", &[first]).unwrap();
        assert!(!store.list_prs().unwrap()[0].1.unread);
    }
}
