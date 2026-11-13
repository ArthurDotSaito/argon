use std::collections::{HashMap, HashSet};

use argon_core::{Involvement, PullRequest, SubjectKind};
use chrono::{DateTime, Utc};
use rusqlite::{OptionalExtension, params};

use crate::Store;
use crate::error::Result;
use crate::local::LocalState;

const MAX_CACHE_AGE_MS: i64 = 15 * 60 * 1000;
const HYDRATION_VERSION_KEY: &str = "hydration_version";

const VOLATILE_PR: &str = "CASE WHEN json_valid(data)
        THEN json_extract(data, '$.state') IS 'Open'
            AND (json_extract(data, '$.checks') IS 'Pending'
                OR json_extract(data, '$.mergeable') IS 'Unknown')
        ELSE 1 END";
const VOLATILE_ISSUE: &str = "NOT json_valid(data)";

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
            SubjectKind::Issue => VOLATILE_ISSUE,
        };
        let oldest_fresh = Utc::now().timestamp_millis() - MAX_CACHE_AGE_MS;
        let mut stmt = self.conn.prepare(&format!(
            "SELECT id, updated_at FROM {}
             WHERE host = ?1 AND synced_at >= ?2 AND NOT ({volatile})",
            table_of(kind)
        ))?;
        let rows = stmt
            .query_map(params![host, oldest_fresh], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows
            .into_iter()
            .filter_map(|(id, at)| Some((id, DateTime::from_timestamp_millis(at)?)))
            .collect())
    }

    pub fn mark_stale(&mut self, host: &str, kind: SubjectKind, ids: &[String]) -> Result<()> {
        self.conn.execute(
            &format!(
                "UPDATE {} SET synced_at = 0
                 WHERE host = ?1 AND id IN (SELECT value FROM json_each(?2))",
                table_of(kind)
            ),
            params![host, serde_json::to_string(ids)?],
        )?;
        Ok(())
    }

    pub fn hydration_version(&self, host: &str) -> Result<Option<u32>> {
        let stored: Option<String> = self
            .conn
            .query_row(
                "SELECT value FROM sync_state WHERE host = ?1 AND key = ?2",
                params![host, HYDRATION_VERSION_KEY],
                |row| row.get(0),
            )
            .optional()?;
        Ok(stored.and_then(|value| value.parse().ok()))
    }

    pub fn set_hydration_version(&mut self, host: &str, version: u32) -> Result<()> {
        self.conn.execute(
            "INSERT INTO sync_state (host, key, value) VALUES (?1, ?2, ?3)
             ON CONFLICT(host, key) DO UPDATE SET value = excluded.value",
            params![host, HYDRATION_VERSION_KEY, version.to_string()],
        )?;
        Ok(())
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

    use chrono::Utc;
    use rusqlite::params;

    use super::MAX_CACHE_AGE_MS;
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

    fn set_synced_at(store: &Store, table: &str, id: &str, synced_at: i64) {
        store
            .conn
            .execute(
                &format!("UPDATE {table} SET synced_at = ?1 WHERE id = ?2"),
                params![synced_at, id],
            )
            .unwrap();
    }

    fn synced_at_of(store: &Store, table: &str, id: &str) -> i64 {
        store
            .conn
            .query_row(
                &format!("SELECT synced_at FROM {table} WHERE id = ?1"),
                [id],
                |row| row.get(0),
            )
            .unwrap()
    }

    fn mapped_ids(store: &Store, kind: SubjectKind) -> Vec<String> {
        let mut ids: Vec<String> = store
            .updated_at_map(DEFAULT_HOST, kind)
            .unwrap()
            .into_keys()
            .collect();
        ids.sort();
        ids
    }

    fn settled(id: &str) -> PullRequest {
        PullRequest {
            checks: Some(ChecksState::Success),
            mergeable: Mergeable::Mergeable,
            ..pr_on(DEFAULT_HOST, id, days_ago(1))
        }
    }

    #[test]
    fn updated_at_map_keeps_open_prs_without_checks_when_mergeable_is_known() {
        let mut store = Store::open_in_memory().unwrap();
        let no_ci = PullRequest {
            checks: None,
            ..settled("no_ci")
        };
        sync(&mut store, DEFAULT_HOST, &[no_ci]);
        assert_eq!(mapped_ids(&store, SubjectKind::Pr), ["no_ci"]);
    }

    #[test]
    fn updated_at_map_drops_rows_older_than_the_max_cache_age() {
        let mut store = Store::open_in_memory().unwrap();
        sync(
            &mut store,
            DEFAULT_HOST,
            &[settled("fresh"), settled("aging"), settled("old")],
        );
        store
            .upsert_issues(
                DEFAULT_HOST,
                &[
                    issue_on(DEFAULT_HOST, "issue_fresh", days_ago(1)),
                    issue_on(DEFAULT_HOST, "issue_old", days_ago(1)),
                ],
            )
            .unwrap();
        let now = Utc::now().timestamp_millis();
        set_synced_at(
            &store,
            "pull_requests",
            "aging",
            now - MAX_CACHE_AGE_MS + 60_000,
        );
        set_synced_at(
            &store,
            "pull_requests",
            "old",
            now - MAX_CACHE_AGE_MS - 60_000,
        );
        set_synced_at(
            &store,
            "issues",
            "issue_old",
            now - MAX_CACHE_AGE_MS - 60_000,
        );

        assert_eq!(mapped_ids(&store, SubjectKind::Pr), ["aging", "fresh"]);
        assert_eq!(mapped_ids(&store, SubjectKind::Issue), ["issue_fresh"]);
    }

    #[test]
    fn updated_at_map_drops_rows_with_invalid_json_so_they_heal_themselves() {
        let mut store = Store::open_in_memory().unwrap();
        sync(&mut store, DEFAULT_HOST, &[settled("ok")]);
        store
            .upsert_issues(DEFAULT_HOST, &[issue_on(DEFAULT_HOST, "ok", days_ago(1))])
            .unwrap();
        let now = Utc::now().timestamp_millis();
        for table in ["pull_requests", "issues"] {
            store
                .conn
                .execute(
                    &format!(
                        "INSERT INTO {table} (host, id, repo, number, data, updated_at, synced_at)
                         VALUES (?1, 'broken', 'acme/api', 1, '{{not json', 0, ?2)"
                    ),
                    params![DEFAULT_HOST, now],
                )
                .unwrap();
        }

        assert_eq!(mapped_ids(&store, SubjectKind::Pr), ["ok"]);
        assert_eq!(mapped_ids(&store, SubjectKind::Issue), ["ok"]);
    }

    #[test]
    fn mark_stale_takes_only_the_named_rows_of_that_host_and_kind_out_of_the_map() {
        let mut store = Store::open_in_memory().unwrap();
        sync(&mut store, DEFAULT_HOST, &[settled("a"), settled("b")]);
        store
            .upsert_issues(
                DEFAULT_HOST,
                &[
                    issue_on(DEFAULT_HOST, "a", days_ago(1)),
                    issue_on(DEFAULT_HOST, "b", days_ago(1)),
                ],
            )
            .unwrap();
        store
            .upsert_issues(GHE, &[issue_on(GHE, "a", days_ago(1))])
            .unwrap();
        let before = store.list_issues().unwrap();

        store
            .mark_stale(DEFAULT_HOST, SubjectKind::Issue, &["a".to_string()])
            .unwrap();

        assert_eq!(mapped_ids(&store, SubjectKind::Issue), ["b"]);
        assert_eq!(mapped_ids(&store, SubjectKind::Pr), ["a", "b"]);
        assert_eq!(
            store.updated_at_map(GHE, SubjectKind::Issue).unwrap().len(),
            1
        );
        assert_eq!(store.list_issues().unwrap(), before);
    }

    #[test]
    fn update_involvement_does_not_refresh_synced_at_but_upsert_does() {
        let mut store = Store::open_in_memory().unwrap();
        sync(&mut store, DEFAULT_HOST, &[settled("a")]);
        set_synced_at(&store, "pull_requests", "a", 1_000);

        store
            .update_involvement(
                DEFAULT_HOST,
                SubjectKind::Pr,
                &[("a".to_string(), requested())],
            )
            .unwrap();
        assert_eq!(synced_at_of(&store, "pull_requests", "a"), 1_000);
        assert!(mapped_ids(&store, SubjectKind::Pr).is_empty());

        sync(&mut store, DEFAULT_HOST, &[settled("a")]);
        assert!(synced_at_of(&store, "pull_requests", "a") > 1_000);
        assert_eq!(mapped_ids(&store, SubjectKind::Pr), ["a"]);
    }

    #[test]
    fn hydration_version_is_absent_until_set_and_tracked_per_host() {
        let mut store = Store::open_in_memory().unwrap();
        assert_eq!(store.hydration_version(DEFAULT_HOST).unwrap(), None);

        store.set_hydration_version(DEFAULT_HOST, 1).unwrap();
        assert_eq!(store.hydration_version(DEFAULT_HOST).unwrap(), Some(1));
        assert_eq!(store.hydration_version(GHE).unwrap(), None);

        store.set_hydration_version(DEFAULT_HOST, 2).unwrap();
        assert_eq!(store.hydration_version(DEFAULT_HOST).unwrap(), Some(2));
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
