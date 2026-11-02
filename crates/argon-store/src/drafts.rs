use std::collections::HashMap;

use argon_core::{DraftComment, DraftReview, DraftSource, PrKey, ReviewEvent, Side};
use chrono::Utc;
use rusqlite::types::Type;
use rusqlite::{Row, params};

use crate::Store;
use crate::error::Result;

const DRAFT_COLUMNS: &str =
    "id, host, pr_id, path, line, side, start_line, start_side, body, source, session_id";

fn invalid_column(index: usize, kind: &str, value: &str) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        index,
        Type::Text,
        format!("unknown {kind} {value:?}").into(),
    )
}

fn parse_side(index: usize, value: &str) -> rusqlite::Result<Side> {
    Side::from_graphql(value).ok_or_else(|| invalid_column(index, "side", value))
}

fn draft_from_row(row: &Row<'_>) -> rusqlite::Result<DraftComment> {
    let side: String = row.get(5)?;
    let start_side: Option<String> = row.get(7)?;
    let source: String = row.get(9)?;
    Ok(DraftComment {
        id: row.get(0)?,
        pr: PrKey::new(row.get::<_, String>(1)?, row.get::<_, String>(2)?),
        path: row.get(3)?,
        line: row.get(4)?,
        side: parse_side(5, &side)?,
        start_line: row.get(6)?,
        start_side: start_side.map(|value| parse_side(7, &value)).transpose()?,
        body: row.get(8)?,
        source: DraftSource::parse(&source).ok_or_else(|| invalid_column(9, "source", &source))?,
        session_id: row.get(10)?,
    })
}

impl Store {
    pub fn add_draft(&mut self, draft: &DraftComment) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO draft_comments
                (host, pr_id, path, line, side, start_line, start_side, body, source, session_id, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                draft.pr.host,
                draft.pr.id.0,
                draft.path,
                draft.line,
                draft.side.as_graphql(),
                draft.start_line,
                draft.start_side.map(Side::as_graphql),
                draft.body,
                draft.source.as_str(),
                draft.session_id,
                Utc::now().timestamp_millis(),
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn drafts(&self, pr: &PrKey) -> Result<Vec<DraftComment>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {DRAFT_COLUMNS} FROM draft_comments
             WHERE host = ?1 AND pr_id = ?2 ORDER BY id"
        ))?;
        let drafts = stmt
            .query_map(params![pr.host, pr.id.0], draft_from_row)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(drafts)
    }

    pub fn update_draft(&mut self, id: i64, body: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE draft_comments SET body = ?1 WHERE id = ?2",
            params![body, id],
        )?;
        Ok(())
    }

    pub fn delete_draft(&mut self, id: i64) -> Result<()> {
        self.conn
            .execute("DELETE FROM draft_comments WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn clear_drafts(&mut self, pr: &PrKey) -> Result<()> {
        self.conn.execute(
            "DELETE FROM draft_comments WHERE host = ?1 AND pr_id = ?2",
            params![pr.host, pr.id.0],
        )?;
        Ok(())
    }

    pub fn clear_review(&mut self, pr: &PrKey) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute(
            "DELETE FROM draft_comments WHERE host = ?1 AND pr_id = ?2",
            params![pr.host, pr.id.0],
        )?;
        tx.execute(
            "DELETE FROM draft_reviews WHERE host = ?1 AND pr_id = ?2",
            params![pr.host, pr.id.0],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn count_drafts_by_pr(&self) -> Result<HashMap<PrKey, usize>> {
        let mut stmt = self
            .conn
            .prepare("SELECT host, pr_id, COUNT(*) FROM draft_comments GROUP BY host, pr_id")?;
        let counts = stmt
            .query_map([], |row| {
                Ok((
                    PrKey::new(row.get::<_, String>(0)?, row.get::<_, String>(1)?),
                    usize::try_from(row.get::<_, i64>(2)?).unwrap_or_default(),
                ))
            })?
            .collect::<std::result::Result<HashMap<_, _>, _>>()?;
        Ok(counts)
    }

    pub fn draft_review(&self, pr: &PrKey) -> Result<Option<DraftReview>> {
        let mut stmt = self
            .conn
            .prepare("SELECT body, event FROM draft_reviews WHERE host = ?1 AND pr_id = ?2")?;
        let mut rows = stmt.query_map(params![pr.host, pr.id.0], |row| {
            let event: String = row.get(1)?;
            Ok(DraftReview {
                body: row.get(0)?,
                event: ReviewEvent::from_graphql(&event)
                    .ok_or_else(|| invalid_column(1, "event", &event))?,
            })
        })?;
        Ok(rows.next().transpose()?)
    }

    pub fn save_draft_review(&mut self, pr: &PrKey, body: &str, event: ReviewEvent) -> Result<()> {
        self.conn.execute(
            "INSERT INTO draft_reviews (host, pr_id, body, event) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(host, pr_id) DO UPDATE SET body = excluded.body, event = excluded.event",
            params![pr.host, pr.id.0, body, event.as_graphql()],
        )?;
        Ok(())
    }

    pub fn clear_draft_review(&mut self, pr: &PrKey) -> Result<()> {
        self.conn.execute(
            "DELETE FROM draft_reviews WHERE host = ?1 AND pr_id = ?2",
            params![pr.host, pr.id.0],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use argon_core::{DEFAULT_HOST, DraftComment, DraftSource, PrKey, ReviewEvent, Side};

    use crate::Store;

    const GHE: &str = "ghe.acme.io";

    fn key(host: &str, id: &str) -> PrKey {
        PrKey::new(host, id)
    }

    fn draft(pr: &PrKey, path: &str, line: u32) -> DraftComment {
        DraftComment {
            id: 0,
            pr: pr.clone(),
            path: path.to_string(),
            line,
            side: Side::Right,
            start_line: None,
            start_side: None,
            body: format!("note on {path}:{line}"),
            source: DraftSource::User,
            session_id: None,
        }
    }

    #[test]
    fn add_then_list_round_trips_every_field_in_insertion_order() {
        let mut store = Store::open_in_memory().unwrap();
        let pr = key(DEFAULT_HOST, "PR_1");
        let plain = draft(&pr, "src/a.ts", 4);
        let ranged = DraftComment {
            side: Side::Left,
            start_line: Some(2),
            start_side: Some(Side::Left),
            source: DraftSource::Agent,
            session_id: Some("sess-1".to_string()),
            ..draft(&pr, "src/b.ts", 9)
        };
        let first = store.add_draft(&plain).unwrap();
        let second = store.add_draft(&ranged).unwrap();
        assert_ne!(first, second);

        let listed = store.drafts(&pr).unwrap();
        assert_eq!(
            listed,
            [
                DraftComment { id: first, ..plain },
                DraftComment {
                    id: second,
                    ..ranged
                },
            ]
        );
    }

    #[test]
    fn update_changes_only_the_body_of_that_draft() {
        let mut store = Store::open_in_memory().unwrap();
        let pr = key(DEFAULT_HOST, "PR_1");
        let first = store.add_draft(&draft(&pr, "a.ts", 1)).unwrap();
        let second = store.add_draft(&draft(&pr, "b.ts", 2)).unwrap();

        store.update_draft(first, "rewritten").unwrap();

        let listed = store.drafts(&pr).unwrap();
        assert_eq!(listed[0].body, "rewritten");
        assert_eq!(listed[0].path, "a.ts");
        assert_eq!(listed[1].id, second);
        assert_eq!(listed[1].body, "note on b.ts:2");
    }

    #[test]
    fn delete_removes_one_draft_and_ignores_unknown_ids() {
        let mut store = Store::open_in_memory().unwrap();
        let pr = key(DEFAULT_HOST, "PR_1");
        let first = store.add_draft(&draft(&pr, "a.ts", 1)).unwrap();
        let second = store.add_draft(&draft(&pr, "b.ts", 2)).unwrap();

        store.delete_draft(first).unwrap();
        store.delete_draft(9999).unwrap();
        store.update_draft(9999, "ghost").unwrap();

        let ids: Vec<i64> = store.drafts(&pr).unwrap().iter().map(|d| d.id).collect();
        assert_eq!(ids, [second]);
    }

    #[test]
    fn same_pr_id_on_different_hosts_is_isolated() {
        let mut store = Store::open_in_memory().unwrap();
        let public = key(DEFAULT_HOST, "PR_1");
        let enterprise = key(GHE, "PR_1");
        store.add_draft(&draft(&public, "a.ts", 1)).unwrap();
        store.add_draft(&draft(&public, "a.ts", 2)).unwrap();
        store.add_draft(&draft(&enterprise, "z.ts", 3)).unwrap();

        assert_eq!(store.drafts(&public).unwrap().len(), 2);
        let enterprise_drafts = store.drafts(&enterprise).unwrap();
        assert_eq!(enterprise_drafts.len(), 1);
        assert_eq!(enterprise_drafts[0].pr, enterprise);

        store.clear_drafts(&public).unwrap();
        assert!(store.drafts(&public).unwrap().is_empty());
        assert_eq!(store.drafts(&enterprise).unwrap().len(), 1);
    }

    #[test]
    fn count_drafts_by_pr_groups_by_host_and_id() {
        let mut store = Store::open_in_memory().unwrap();
        assert!(store.count_drafts_by_pr().unwrap().is_empty());
        let public = key(DEFAULT_HOST, "PR_1");
        let enterprise = key(GHE, "PR_1");
        let other = key(DEFAULT_HOST, "PR_2");
        for (pr, times) in [(&public, 3), (&enterprise, 1), (&other, 2)] {
            for line in 0..times {
                store.add_draft(&draft(pr, "a.ts", line + 1)).unwrap();
            }
        }

        let counts = store.count_drafts_by_pr().unwrap();
        assert_eq!(counts.len(), 3);
        assert_eq!(counts[&public], 3);
        assert_eq!(counts[&enterprise], 1);
        assert_eq!(counts[&other], 2);

        store.clear_drafts(&public).unwrap();
        assert!(!store.count_drafts_by_pr().unwrap().contains_key(&public));
    }

    #[test]
    fn draft_review_saves_overwrites_and_clears_per_pr() {
        let mut store = Store::open_in_memory().unwrap();
        let public = key(DEFAULT_HOST, "PR_1");
        let enterprise = key(GHE, "PR_1");
        assert_eq!(store.draft_review(&public).unwrap(), None);

        store
            .save_draft_review(&public, "first thoughts", ReviewEvent::Comment)
            .unwrap();
        store
            .save_draft_review(&public, "ship it", ReviewEvent::Approve)
            .unwrap();
        store
            .save_draft_review(&enterprise, "needs work", ReviewEvent::RequestChanges)
            .unwrap();

        let saved = store.draft_review(&public).unwrap().unwrap();
        assert_eq!(saved.body, "ship it");
        assert_eq!(saved.event, ReviewEvent::Approve);
        assert_eq!(
            store.draft_review(&enterprise).unwrap().unwrap().event,
            ReviewEvent::RequestChanges
        );

        store.clear_draft_review(&public).unwrap();
        assert_eq!(store.draft_review(&public).unwrap(), None);
        assert!(store.draft_review(&enterprise).unwrap().is_some());
        store.clear_draft_review(&public).unwrap();
    }

    #[test]
    fn clearing_drafts_keeps_the_draft_review_body() {
        let mut store = Store::open_in_memory().unwrap();
        let pr = key(DEFAULT_HOST, "PR_1");
        store.add_draft(&draft(&pr, "a.ts", 1)).unwrap();
        store
            .save_draft_review(&pr, "body", ReviewEvent::Comment)
            .unwrap();

        store.clear_drafts(&pr).unwrap();

        assert!(store.drafts(&pr).unwrap().is_empty());
        assert!(store.draft_review(&pr).unwrap().is_some());
    }

    #[test]
    fn clear_review_removes_drafts_and_draft_review_of_that_pr_only() {
        let mut store = Store::open_in_memory().unwrap();
        let public = key(DEFAULT_HOST, "PR_1");
        let enterprise = key(GHE, "PR_1");
        for pr in [&public, &enterprise] {
            store.add_draft(&draft(pr, "a.ts", 1)).unwrap();
            store
                .save_draft_review(pr, "body", ReviewEvent::Comment)
                .unwrap();
        }

        store.clear_review(&public).unwrap();
        store.clear_review(&public).unwrap();

        assert!(store.drafts(&public).unwrap().is_empty());
        assert_eq!(store.draft_review(&public).unwrap(), None);
        assert_eq!(store.drafts(&enterprise).unwrap().len(), 1);
        assert!(store.draft_review(&enterprise).unwrap().is_some());
    }

    #[test]
    fn unknown_stored_values_surface_as_errors() {
        let mut store = Store::open_in_memory().unwrap();
        let pr = key(DEFAULT_HOST, "PR_1");
        store.add_draft(&draft(&pr, "a.ts", 1)).unwrap();
        store
            .conn
            .execute("UPDATE draft_comments SET side = 'MIDDLE'", [])
            .unwrap();
        assert!(store.drafts(&pr).is_err());

        store
            .conn
            .execute(
                "INSERT INTO draft_reviews (host, pr_id, body, event) VALUES (?1, 'PR_1', '', 'PENDING')",
                [DEFAULT_HOST],
            )
            .unwrap();
        assert!(store.draft_review(&pr).is_err());
    }
}
