use std::collections::{HashMap, HashSet};

use argon_core::Issue;
use chrono::{DateTime, Utc};
use rusqlite::params;

use crate::Store;
use crate::error::Result;
use crate::local::LocalState;

const ISSUE_BASELINE: &str = "issue_baseline";

impl Store {
    pub fn upsert_issues(&mut self, host: &str, issues: &[Issue]) -> Result<()> {
        let synced_at = Utc::now().timestamp_millis();
        let tx = self.conn.transaction()?;
        {
            let mut upsert = tx.prepare(
                "INSERT INTO issues (host, id, repo, number, data, updated_at, synced_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(host, id) DO UPDATE SET
                    repo = excluded.repo,
                    number = excluded.number,
                    data = excluded.data,
                    updated_at = excluded.updated_at,
                    synced_at = excluded.synced_at",
            )?;
            for issue in issues {
                let number = i64::try_from(issue.number)
                    .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
                upsert.execute(params![
                    host,
                    issue.id.0,
                    issue.repo,
                    number,
                    serde_json::to_string(issue)?,
                    issue.updated_at.timestamp_millis(),
                    synced_at,
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn prune_issues(&mut self, host: &str, kept: &HashSet<String>) -> Result<()> {
        self.conn.execute(
            "DELETE FROM issues
             WHERE host = ?1 AND id NOT IN (SELECT value FROM json_each(?2))",
            params![host, serde_json::to_string(kept)?],
        )?;
        Ok(())
    }

    pub fn list_issues(&self) -> Result<Vec<(Issue, LocalState)>> {
        self.list_with_local("issues")
    }

    pub fn has_issues(&self, host: &str) -> Result<bool> {
        Ok(self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM issues WHERE host = ?1)",
            [host],
            |row| row.get(0),
        )?)
    }

    pub fn issue_baseline_done(&self, host: &str) -> Result<bool> {
        Ok(self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM sync_state WHERE host = ?1 AND key = ?2)",
            params![host, ISSUE_BASELINE],
            |row| row.get(0),
        )?)
    }

    pub fn mark_issue_baseline_done(&mut self, host: &str, at: DateTime<Utc>) -> Result<()> {
        self.conn.execute(
            "INSERT INTO sync_state (host, key, value) VALUES (?1, ?2, ?3)
             ON CONFLICT(host, key) DO UPDATE SET value = excluded.value",
            params![host, ISSUE_BASELINE, at.to_rfc3339()],
        )?;
        Ok(())
    }

    pub fn stored_last_mentions(&self, host: &str) -> Result<HashMap<String, DateTime<Utc>>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, data FROM issues WHERE host = ?1")?;
        let rows = stmt
            .query_map([host], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows
            .into_iter()
            .filter_map(|(id, data)| {
                let issue: Issue = serde_json::from_str(&data).ok()?;
                Some((id, issue.last_mention_at?))
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};

    use argon_core::fixtures::{days_ago, issue_on, pr_on};
    use argon_core::{DEFAULT_HOST, PrKey};
    use rusqlite::params;

    use crate::Store;

    const GHE: &str = "ghe.acme.io";

    fn issue_ids(store: &Store) -> Vec<String> {
        store
            .list_issues()
            .unwrap()
            .into_iter()
            .map(|(i, _)| i.id.0)
            .collect()
    }

    fn pr_ids(store: &Store) -> Vec<String> {
        store
            .list_prs()
            .unwrap()
            .into_iter()
            .map(|(p, _)| p.id.0)
            .collect()
    }

    fn kept(ids: &[&str]) -> HashSet<String> {
        ids.iter().map(|id| id.to_string()).collect()
    }

    #[test]
    fn upsert_and_list_orders_by_updated_at_desc() {
        let mut store = Store::open_in_memory().unwrap();
        let old = issue_on(DEFAULT_HOST, "old", days_ago(5));
        let newest = issue_on(DEFAULT_HOST, "newest", days_ago(1));
        let mid = issue_on(DEFAULT_HOST, "mid", days_ago(3));
        store
            .upsert_issues(DEFAULT_HOST, &[old, newest.clone(), mid])
            .unwrap();
        assert_eq!(issue_ids(&store), ["newest", "mid", "old"]);

        let mut updated = newest;
        updated.title = "Renamed".into();
        store
            .upsert_issues(DEFAULT_HOST, std::slice::from_ref(&updated))
            .unwrap();
        let listed = store.list_issues().unwrap();
        assert_eq!(listed.len(), 3);
        assert_eq!(listed[0].0, updated);
    }

    #[test]
    fn has_issues_is_scoped_to_the_host() {
        let mut store = Store::open_in_memory().unwrap();
        assert!(!store.has_issues(DEFAULT_HOST).unwrap());
        store
            .upsert_issues(DEFAULT_HOST, &[issue_on(DEFAULT_HOST, "a", days_ago(1))])
            .unwrap();
        assert!(store.has_issues(DEFAULT_HOST).unwrap());
        assert!(!store.has_issues(GHE).unwrap());
    }

    #[test]
    fn prune_issues_deletes_only_that_host_outside_the_kept_set() {
        let mut store = Store::open_in_memory().unwrap();
        store
            .upsert_issues(
                DEFAULT_HOST,
                &[
                    issue_on(DEFAULT_HOST, "a", days_ago(3)),
                    issue_on(DEFAULT_HOST, "b", days_ago(2)),
                ],
            )
            .unwrap();
        store
            .upsert_issues(GHE, &[issue_on(GHE, "a", days_ago(4))])
            .unwrap();
        store
            .prune_issues(DEFAULT_HOST, &kept(&["b", "z"]))
            .unwrap();
        let keys: Vec<PrKey> = store
            .list_issues()
            .unwrap()
            .iter()
            .map(|(i, _)| i.key())
            .collect();
        assert_eq!(keys, [PrKey::new(DEFAULT_HOST, "b"), PrKey::new(GHE, "a")]);
    }

    #[test]
    fn prune_issues_does_not_delete_prs_and_prune_prs_does_not_delete_issues() {
        let mut store = Store::open_in_memory().unwrap();
        store
            .upsert_prs(DEFAULT_HOST, &[pr_on(DEFAULT_HOST, "pr", days_ago(2))])
            .unwrap();
        store
            .upsert_issues(
                DEFAULT_HOST,
                &[issue_on(DEFAULT_HOST, "issue", days_ago(1))],
            )
            .unwrap();

        store.prune_issues(DEFAULT_HOST, &HashSet::new()).unwrap();
        assert_eq!(pr_ids(&store), ["pr"]);
        assert!(issue_ids(&store).is_empty());

        store
            .upsert_issues(
                DEFAULT_HOST,
                &[issue_on(DEFAULT_HOST, "issue", days_ago(1))],
            )
            .unwrap();
        store.prune_prs(DEFAULT_HOST, &HashSet::new()).unwrap();
        assert!(pr_ids(&store).is_empty());
        assert_eq!(issue_ids(&store), ["issue"]);
    }

    #[test]
    fn list_issues_reports_last_seen() {
        let mut store = Store::open_in_memory().unwrap();
        let issue = issue_on(DEFAULT_HOST, "a", days_ago(3));
        store
            .upsert_issues(DEFAULT_HOST, std::slice::from_ref(&issue))
            .unwrap();

        let state = store.list_issues().unwrap()[0].1;
        assert_eq!(state.last_seen, None);
        assert!(state.unread);

        store.mark_seen(&issue.key(), issue.updated_at).unwrap();
        let state = store.list_issues().unwrap()[0].1;
        assert_eq!(state.last_seen, Some(issue.updated_at));
        assert!(!state.unread);
    }

    #[test]
    fn list_prs_reports_last_seen() {
        let mut store = Store::open_in_memory().unwrap();
        let pr = pr_on(DEFAULT_HOST, "a", days_ago(3));
        store
            .upsert_prs(DEFAULT_HOST, std::slice::from_ref(&pr))
            .unwrap();
        assert_eq!(store.list_prs().unwrap()[0].1.last_seen, None);

        store.mark_seen(&pr.key(), days_ago(4)).unwrap();
        let state = store.list_prs().unwrap()[0].1;
        assert_eq!(state.last_seen, Some(days_ago(4)));
        assert!(state.unread);
    }

    #[test]
    fn mark_seen_all_feeds_last_seen_map_for_one_host() {
        let mut store = Store::open_in_memory().unwrap();
        let first = issue_on(DEFAULT_HOST, "a", days_ago(3));
        let second = issue_on(DEFAULT_HOST, "b", days_ago(2));
        let other_host = issue_on(GHE, "c", days_ago(1));
        store
            .upsert_issues(DEFAULT_HOST, &[first.clone(), second.clone()])
            .unwrap();
        store
            .upsert_issues(GHE, std::slice::from_ref(&other_host))
            .unwrap();
        store.mark_seen(&other_host.key(), days_ago(1)).unwrap();
        assert!(store.last_seen_map(DEFAULT_HOST).unwrap().is_empty());

        store
            .mark_seen_all(
                DEFAULT_HOST,
                &[
                    ("a".to_string(), first.updated_at),
                    ("b".to_string(), second.updated_at),
                ],
            )
            .unwrap();

        let map = store.last_seen_map(DEFAULT_HOST).unwrap();
        assert_eq!(map.len(), 2);
        assert_eq!(map["a"], first.updated_at);
        assert_eq!(map["b"], second.updated_at);
        assert!(
            store
                .list_issues()
                .unwrap()
                .iter()
                .filter(|(i, _)| i.host == DEFAULT_HOST)
                .all(|(_, state)| !state.unread)
        );
    }

    #[test]
    fn issue_baseline_is_tracked_per_host() {
        let mut store = Store::open_in_memory().unwrap();
        assert!(!store.issue_baseline_done(DEFAULT_HOST).unwrap());

        store
            .mark_issue_baseline_done(DEFAULT_HOST, days_ago(1))
            .unwrap();
        assert!(store.issue_baseline_done(DEFAULT_HOST).unwrap());
        assert!(!store.issue_baseline_done(GHE).unwrap());

        store
            .mark_issue_baseline_done(DEFAULT_HOST, days_ago(0))
            .unwrap();
        assert!(store.issue_baseline_done(DEFAULT_HOST).unwrap());
    }

    #[test]
    fn issue_baseline_survives_an_empty_issue_table() {
        let mut store = Store::open_in_memory().unwrap();
        store
            .mark_issue_baseline_done(DEFAULT_HOST, days_ago(0))
            .unwrap();
        store.prune_issues(DEFAULT_HOST, &HashSet::new()).unwrap();
        assert!(!store.has_issues(DEFAULT_HOST).unwrap());
        assert!(store.issue_baseline_done(DEFAULT_HOST).unwrap());
    }

    #[test]
    fn stored_last_mentions_reads_mentioned_issues_of_one_host() {
        let mut store = Store::open_in_memory().unwrap();
        let mut mentioned = issue_on(DEFAULT_HOST, "mentioned", days_ago(1));
        mentioned.last_mention_at = Some(days_ago(2));
        let quiet = issue_on(DEFAULT_HOST, "quiet", days_ago(1));
        let mut other_host = issue_on(GHE, "other", days_ago(1));
        other_host.last_mention_at = Some(days_ago(3));
        store
            .upsert_issues(DEFAULT_HOST, &[mentioned, quiet])
            .unwrap();
        store.upsert_issues(GHE, &[other_host]).unwrap();
        store
            .conn
            .execute(
                "INSERT INTO issues (host, id, repo, number, data, updated_at, synced_at)
                 VALUES (?1, 'broken', 'acme/api', 1, '{not json', 0, 0)",
                params![DEFAULT_HOST],
            )
            .unwrap();

        assert_eq!(
            store.stored_last_mentions(DEFAULT_HOST).unwrap(),
            HashMap::from([("mentioned".to_string(), days_ago(2))])
        );
    }

    #[test]
    fn last_seen_map_skips_rows_marked_unread() {
        let mut store = Store::open_in_memory().unwrap();
        let issue = issue_on(DEFAULT_HOST, "a", days_ago(1));
        store.mark_seen(&issue.key(), issue.updated_at).unwrap();
        store.mark_unread(&issue.key()).unwrap();
        store.archive(&issue.key(), issue.updated_at).unwrap();
        assert!(store.last_seen_map(DEFAULT_HOST).unwrap().is_empty());
    }
}
