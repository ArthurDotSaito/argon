use std::collections::HashSet;

use argon_core::PullRequest;
use chrono::Utc;
use rusqlite::params;

use crate::Store;
use crate::error::Result;
use crate::local::LocalState;

impl Store {
    pub fn upsert_prs(&mut self, host: &str, prs: &[PullRequest]) -> Result<()> {
        let synced_at = Utc::now().timestamp_millis();
        let tx = self.conn.transaction()?;
        {
            let mut upsert = tx.prepare(
                "INSERT INTO pull_requests (host, id, repo, number, data, updated_at, synced_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(host, id) DO UPDATE SET
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
                    host,
                    pr.id.0,
                    pr.repo,
                    number,
                    serde_json::to_string(pr)?,
                    pr.updated_at.timestamp_millis(),
                    synced_at,
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn prune_prs(&mut self, host: &str, kept: &HashSet<String>) -> Result<()> {
        self.conn.execute(
            "DELETE FROM pull_requests
             WHERE host = ?1 AND id NOT IN (SELECT value FROM json_each(?2))",
            params![host, serde_json::to_string(kept)?],
        )?;
        Ok(())
    }

    pub fn list_prs(&self) -> Result<Vec<(PullRequest, LocalState)>> {
        self.list_with_local("pull_requests")
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use argon_core::fixtures::{days_ago, pr_on};
    use argon_core::{DEFAULT_HOST, PrKey, PullRequest};

    use crate::Store;

    const GHE: &str = "ghe.acme.io";

    fn ids(store: &Store) -> Vec<String> {
        store
            .list_prs()
            .unwrap()
            .into_iter()
            .map(|(p, _)| p.id.0)
            .collect()
    }

    fn keys(store: &Store) -> Vec<PrKey> {
        store
            .list_prs()
            .unwrap()
            .into_iter()
            .map(|(p, _)| p.key())
            .collect()
    }

    fn kept(ids: &[&str]) -> HashSet<String> {
        ids.iter().map(|id| id.to_string()).collect()
    }

    fn sync(store: &mut Store, host: &str, prs: &[PullRequest]) {
        store.upsert_prs(host, prs).unwrap();
        let ids = prs.iter().map(|pr| pr.id.0.clone()).collect();
        store.prune_prs(host, &ids).unwrap();
    }

    #[test]
    fn upsert_and_list_orders_by_updated_at_desc() {
        let mut store = Store::open_in_memory().unwrap();
        let old = pr_on(DEFAULT_HOST, "old", days_ago(5));
        let newest = pr_on(DEFAULT_HOST, "newest", days_ago(1));
        let mid = pr_on(DEFAULT_HOST, "mid", days_ago(3));
        store
            .upsert_prs(DEFAULT_HOST, &[old, newest.clone(), mid])
            .unwrap();
        assert_eq!(ids(&store), ["newest", "mid", "old"]);

        let mut updated = newest;
        updated.title = "Renamed".into();
        store
            .upsert_prs(DEFAULT_HOST, std::slice::from_ref(&updated))
            .unwrap();
        let listed = store.list_prs().unwrap();
        assert_eq!(listed.len(), 3);
        assert_eq!(listed[0].0, updated);
    }

    #[test]
    fn upsert_never_deletes_rows_missing_from_the_batch() {
        let mut store = Store::open_in_memory().unwrap();
        store
            .upsert_prs(
                DEFAULT_HOST,
                &[
                    pr_on(DEFAULT_HOST, "a", days_ago(2)),
                    pr_on(DEFAULT_HOST, "b", days_ago(1)),
                ],
            )
            .unwrap();
        store.upsert_prs(DEFAULT_HOST, &[]).unwrap();
        store
            .upsert_prs(DEFAULT_HOST, &[pr_on(DEFAULT_HOST, "c", days_ago(3))])
            .unwrap();
        assert_eq!(ids(&store), ["b", "a", "c"]);
    }

    #[test]
    fn prune_deletes_only_rows_of_that_host_outside_the_kept_set() {
        let mut store = Store::open_in_memory().unwrap();
        store
            .upsert_prs(
                DEFAULT_HOST,
                &[
                    pr_on(DEFAULT_HOST, "a", days_ago(3)),
                    pr_on(DEFAULT_HOST, "b", days_ago(2)),
                    pr_on(DEFAULT_HOST, "c", days_ago(1)),
                ],
            )
            .unwrap();
        store
            .upsert_prs(GHE, &[pr_on(GHE, "a", days_ago(4))])
            .unwrap();
        store.prune_prs(DEFAULT_HOST, &kept(&["b", "z"])).unwrap();
        assert_eq!(
            keys(&store),
            [PrKey::new(DEFAULT_HOST, "b"), PrKey::new(GHE, "a")]
        );
    }

    #[test]
    fn prune_with_empty_set_clears_only_that_host() {
        let mut store = Store::open_in_memory().unwrap();
        sync(
            &mut store,
            DEFAULT_HOST,
            &[pr_on(DEFAULT_HOST, "a", days_ago(2))],
        );
        sync(&mut store, GHE, &[pr_on(GHE, "b", days_ago(1))]);
        store.prune_prs(DEFAULT_HOST, &HashSet::new()).unwrap();
        assert_eq!(ids(&store), ["b"]);
    }

    #[test]
    fn sync_removes_prs_that_left_the_window() {
        let mut store = Store::open_in_memory().unwrap();
        sync(
            &mut store,
            DEFAULT_HOST,
            &[
                pr_on(DEFAULT_HOST, "a", days_ago(2)),
                pr_on(DEFAULT_HOST, "b", days_ago(1)),
            ],
        );
        sync(
            &mut store,
            DEFAULT_HOST,
            &[pr_on(DEFAULT_HOST, "b", days_ago(1))],
        );
        assert_eq!(ids(&store), ["b"]);
    }

    #[test]
    fn sync_does_not_affect_other_hosts() {
        let mut store = Store::open_in_memory().unwrap();
        sync(
            &mut store,
            DEFAULT_HOST,
            &[pr_on(DEFAULT_HOST, "a", days_ago(2))],
        );
        sync(&mut store, GHE, &[pr_on(GHE, "b", days_ago(1))]);
        sync(
            &mut store,
            DEFAULT_HOST,
            &[pr_on(DEFAULT_HOST, "c", days_ago(3))],
        );
        assert_eq!(ids(&store), ["b", "c"]);
    }

    #[test]
    fn prune_keeps_local_state_of_removed_prs() {
        let mut store = Store::open_in_memory().unwrap();
        let first = pr_on(DEFAULT_HOST, "a", days_ago(2));
        sync(&mut store, DEFAULT_HOST, std::slice::from_ref(&first));
        store.mark_seen(&first.key(), first.updated_at).unwrap();
        store.prune_prs(DEFAULT_HOST, &HashSet::new()).unwrap();
        sync(&mut store, DEFAULT_HOST, &[first]);
        assert!(!store.list_prs().unwrap()[0].1.unread);
    }

    #[test]
    fn same_id_on_two_hosts_does_not_collide() {
        let mut store = Store::open_in_memory().unwrap();
        let on_github = pr_on(DEFAULT_HOST, "same", days_ago(2));
        let on_ghe = pr_on(GHE, "same", days_ago(1));
        sync(&mut store, DEFAULT_HOST, std::slice::from_ref(&on_github));
        sync(&mut store, GHE, std::slice::from_ref(&on_ghe));
        store
            .mark_seen(&on_github.key(), on_github.updated_at)
            .unwrap();
        store.archive(&on_ghe.key(), on_ghe.updated_at).unwrap();

        let listed: Vec<(PrKey, bool, bool)> = store
            .list_prs()
            .unwrap()
            .into_iter()
            .map(|(pr, state)| (pr.key(), state.unread, state.archived))
            .collect();
        assert_eq!(
            listed,
            [
                (PrKey::new(GHE, "same"), true, true),
                (PrKey::new(DEFAULT_HOST, "same"), false, false),
            ]
        );

        store.prune_prs(GHE, &HashSet::new()).unwrap();
        assert_eq!(ids(&store), ["same"]);
        assert_eq!(store.list_prs().unwrap()[0].0, on_github);
    }
}
