use std::collections::{HashMap, HashSet};

use argon_core::{Involvement, PullRequest, SubjectKind};
use chrono::{DateTime, Utc};
use rusqlite::params;

use crate::Store;
use crate::error::Result;
use crate::local::LocalState;

const VOLATILE_PR: &str = "CASE WHEN json_valid(data)
        THEN json_extract(data, '$.state') = 'Open'
            AND (json_extract(data, '$.checks') = 'Pending'
                OR json_extract(data, '$.mergeable') = 'Unknown')
        ELSE 0 END";

fn table_of(kind: SubjectKind) -> &'static str {
    match kind {
        SubjectKind::Pr => "pull_requests",
        SubjectKind::Issue => "issues",
    }
}

impl Store {
    pub fn updated_at_map(
        &self,
        host: &str,
        kind: SubjectKind,
    ) -> Result<HashMap<String, DateTime<Utc>>> {
        let volatile = match kind {
            SubjectKind::Pr => VOLATILE_PR,
            SubjectKind::Issue => "0",
        };
        let mut stmt = self.conn.prepare(&format!(
            "SELECT id, updated_at FROM {} WHERE host = ?1 AND NOT ({volatile})",
            table_of(kind)
        ))?;
        let rows = stmt
            .query_map([host], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows
            .into_iter()
            .filter_map(|(id, at)| Some((id, DateTime::from_timestamp_millis(at)?)))
            .collect())
    }

    pub fn update_involvement(
        &mut self,
        host: &str,
        kind: SubjectKind,
        items: &[(String, Involvement)],
    ) -> Result<()> {
        let tx = self.conn.transaction()?;
        {
            let mut update = tx.prepare(&format!(
                "UPDATE {} SET data = CASE WHEN json_valid(data)
                    THEN json_set(data, '$.involvement', json(?3)) ELSE data END
                 WHERE host = ?1 AND id = ?2",
                table_of(kind)
            ))?;
            for (id, involvement) in items {
                update.execute(params![host, id, serde_json::to_string(involvement)?])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

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
    use std::collections::{HashMap, HashSet};

    use argon_core::fixtures::{days_ago, issue_on, pr_on, requested};
    use argon_core::{
        ChecksState, DEFAULT_HOST, Involvement, Mergeable, PrKey, PrState, PullRequest, SubjectKind,
    };

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

    #[test]
    fn updated_at_map_is_scoped_to_host_and_kind() {
        let mut store = Store::open_in_memory().unwrap();
        let pr = PullRequest {
            checks: Some(ChecksState::Success),
            mergeable: Mergeable::Mergeable,
            ..pr_on(DEFAULT_HOST, "pr", days_ago(2))
        };
        let issue = issue_on(DEFAULT_HOST, "issue", days_ago(1));
        store
            .upsert_prs(DEFAULT_HOST, std::slice::from_ref(&pr))
            .unwrap();
        store
            .upsert_prs(GHE, &[pr_on(GHE, "other", days_ago(3))])
            .unwrap();
        store
            .upsert_issues(DEFAULT_HOST, std::slice::from_ref(&issue))
            .unwrap();

        assert_eq!(
            store.updated_at_map(DEFAULT_HOST, SubjectKind::Pr).unwrap(),
            HashMap::from([("pr".to_string(), pr.updated_at)])
        );
        assert_eq!(
            store
                .updated_at_map(DEFAULT_HOST, SubjectKind::Issue)
                .unwrap(),
            HashMap::from([("issue".to_string(), issue.updated_at)])
        );
        assert!(
            store
                .updated_at_map("none.example", SubjectKind::Pr)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn updated_at_map_leaves_out_open_prs_whose_checks_or_mergeability_are_unsettled() {
        let mut store = Store::open_in_memory().unwrap();
        let settled = PullRequest {
            checks: Some(ChecksState::Success),
            mergeable: Mergeable::Mergeable,
            ..pr_on(DEFAULT_HOST, "settled", days_ago(1))
        };
        let pending = PullRequest {
            checks: Some(ChecksState::Pending),
            ..settled.clone()
        };
        let unknown = PullRequest {
            mergeable: Mergeable::Unknown,
            ..settled.clone()
        };
        let closed = PullRequest {
            state: PrState::Closed,
            checks: Some(ChecksState::Pending),
            mergeable: Mergeable::Unknown,
            ..settled.clone()
        };
        let rename = |pr: &PullRequest, id: &str| PullRequest {
            id: argon_core::PrId(id.into()),
            ..pr.clone()
        };
        sync(
            &mut store,
            DEFAULT_HOST,
            &[
                settled,
                rename(&pending, "pending"),
                rename(&unknown, "unknown"),
                rename(&closed, "closed"),
            ],
        );

        let mut ids: Vec<String> = store
            .updated_at_map(DEFAULT_HOST, SubjectKind::Pr)
            .unwrap()
            .into_keys()
            .collect();
        ids.sort();
        assert_eq!(ids, ["closed", "settled"]);
    }

    #[test]
    fn update_involvement_rewrites_only_the_involvement_of_the_row() {
        let mut store = Store::open_in_memory().unwrap();
        let pr = pr_on(DEFAULT_HOST, "pr", days_ago(2));
        let mut issue = issue_on(DEFAULT_HOST, "issue", days_ago(1));
        issue.last_mention_at = Some(days_ago(1));
        issue.involvement = requested();
        sync(&mut store, DEFAULT_HOST, std::slice::from_ref(&pr));
        store
            .upsert_issues(DEFAULT_HOST, std::slice::from_ref(&issue))
            .unwrap();

        store
            .update_involvement(
                DEFAULT_HOST,
                SubjectKind::Pr,
                &[
                    ("pr".to_string(), requested()),
                    ("ghost".to_string(), requested()),
                ],
            )
            .unwrap();
        store
            .update_involvement(
                DEFAULT_HOST,
                SubjectKind::Issue,
                &[("issue".to_string(), Involvement::default())],
            )
            .unwrap();

        let updated = store.list_prs().unwrap().remove(0).0;
        assert_eq!(updated.involvement, requested());
        assert_eq!(
            PullRequest {
                involvement: pr.involvement,
                ..updated
            },
            pr
        );
        assert_eq!(
            store.list_issues().unwrap()[0].0,
            argon_core::Issue {
                involvement: Involvement::default(),
                ..issue
            }
        );
        assert_eq!(ids(&store), ["pr"]);
    }
}
