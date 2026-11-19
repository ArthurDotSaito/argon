use rusqlite::Connection;
use rusqlite_migration::{M, Migrations};

use crate::error::Result;

const INITIAL: &str = "
CREATE TABLE pull_requests (
    id TEXT PRIMARY KEY NOT NULL,
    host TEXT NOT NULL,
    repo TEXT NOT NULL,
    number INTEGER NOT NULL,
    data TEXT NOT NULL,
    updated_at INTEGER NOT NULL,
    synced_at INTEGER NOT NULL
);
CREATE INDEX idx_pull_requests_host ON pull_requests(host);

CREATE TABLE pr_local (
    pr_id TEXT PRIMARY KEY NOT NULL,
    last_seen_updated_at INTEGER,
    archived_at INTEGER
);

CREATE TABLE draft_comments (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    pr_id TEXT NOT NULL,
    path TEXT NOT NULL,
    line INTEGER NOT NULL,
    side TEXT NOT NULL,
    start_line INTEGER,
    start_side TEXT,
    body TEXT NOT NULL,
    source TEXT NOT NULL CHECK (source IN ('user', 'agent')),
    session_id TEXT,
    created_at INTEGER NOT NULL
);
CREATE INDEX idx_draft_comments_pr_id ON draft_comments(pr_id);

CREATE TABLE draft_reviews (
    pr_id TEXT PRIMARY KEY NOT NULL,
    body TEXT NOT NULL,
    event TEXT NOT NULL
);

CREATE TABLE agent_sessions (
    id TEXT PRIMARY KEY NOT NULL,
    pr_id TEXT NOT NULL,
    agent TEXT NOT NULL,
    model TEXT NOT NULL,
    template TEXT NOT NULL,
    status TEXT NOT NULL,
    worktree_path TEXT,
    acp_session_id TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE TABLE agent_events (
    session_id TEXT NOT NULL REFERENCES agent_sessions(id) ON DELETE CASCADE,
    seq INTEGER NOT NULL,
    data TEXT NOT NULL,
    PRIMARY KEY (session_id, seq)
);
CREATE INDEX idx_agent_events_session_seq ON agent_events(session_id, seq);

CREATE TABLE findings (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL REFERENCES agent_sessions(id) ON DELETE CASCADE,
    severity TEXT NOT NULL,
    path TEXT NOT NULL,
    line INTEGER NOT NULL,
    side TEXT NOT NULL,
    title TEXT NOT NULL,
    body TEXT NOT NULL,
    suggestion TEXT,
    state TEXT NOT NULL DEFAULT 'new' CHECK (state IN ('new', 'added', 'ignored'))
);
CREATE INDEX idx_findings_session_id ON findings(session_id);
";

const COMPOSITE_PR_KEY: &str = "
CREATE TABLE pull_requests_v2 (
    host TEXT NOT NULL,
    id TEXT NOT NULL,
    repo TEXT NOT NULL,
    number INTEGER NOT NULL,
    data TEXT NOT NULL,
    updated_at INTEGER NOT NULL,
    synced_at INTEGER NOT NULL,
    PRIMARY KEY (host, id)
);
INSERT INTO pull_requests_v2 (host, id, repo, number, data, updated_at, synced_at)
    SELECT host, id, repo, number, data, updated_at, synced_at FROM pull_requests;

CREATE TABLE pr_local_v2 (
    host TEXT NOT NULL,
    pr_id TEXT NOT NULL,
    last_seen_updated_at INTEGER,
    archived_at INTEGER,
    PRIMARY KEY (host, pr_id)
);
INSERT INTO pr_local_v2 (host, pr_id, last_seen_updated_at, archived_at)
    SELECT COALESCE((SELECT p.host FROM pull_requests p WHERE p.id = l.pr_id), 'github.com'),
           l.pr_id, l.last_seen_updated_at, l.archived_at
    FROM pr_local l;

CREATE TABLE draft_comments_v2 (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    host TEXT NOT NULL,
    pr_id TEXT NOT NULL,
    path TEXT NOT NULL,
    line INTEGER NOT NULL,
    side TEXT NOT NULL,
    start_line INTEGER,
    start_side TEXT,
    body TEXT NOT NULL,
    source TEXT NOT NULL CHECK (source IN ('user', 'agent')),
    session_id TEXT,
    created_at INTEGER NOT NULL
);
INSERT INTO draft_comments_v2
    (id, host, pr_id, path, line, side, start_line, start_side, body, source, session_id, created_at)
    SELECT d.id, COALESCE((SELECT p.host FROM pull_requests p WHERE p.id = d.pr_id), 'github.com'),
           d.pr_id, d.path, d.line, d.side, d.start_line, d.start_side, d.body, d.source,
           d.session_id, d.created_at
    FROM draft_comments d;

CREATE TABLE draft_reviews_v2 (
    host TEXT NOT NULL,
    pr_id TEXT NOT NULL,
    body TEXT NOT NULL,
    event TEXT NOT NULL,
    PRIMARY KEY (host, pr_id)
);
INSERT INTO draft_reviews_v2 (host, pr_id, body, event)
    SELECT COALESCE((SELECT p.host FROM pull_requests p WHERE p.id = r.pr_id), 'github.com'),
           r.pr_id, r.body, r.event
    FROM draft_reviews r;

ALTER TABLE agent_sessions ADD COLUMN host TEXT NOT NULL DEFAULT 'github.com';
UPDATE agent_sessions
    SET host = COALESCE((SELECT p.host FROM pull_requests p WHERE p.id = agent_sessions.pr_id), 'github.com');

DROP TABLE pull_requests;
ALTER TABLE pull_requests_v2 RENAME TO pull_requests;
DROP TABLE pr_local;
ALTER TABLE pr_local_v2 RENAME TO pr_local;
DROP TABLE draft_comments;
ALTER TABLE draft_comments_v2 RENAME TO draft_comments;
CREATE INDEX idx_draft_comments_pr ON draft_comments(host, pr_id);
DROP TABLE draft_reviews;
ALTER TABLE draft_reviews_v2 RENAME TO draft_reviews;
CREATE INDEX idx_agent_sessions_pr ON agent_sessions(host, pr_id);

DROP INDEX idx_agent_events_session_seq;

CREATE TABLE viewers (
    host TEXT PRIMARY KEY NOT NULL,
    login TEXT NOT NULL,
    orgs TEXT NOT NULL,
    updated_at INTEGER NOT NULL
);
";

const DRAFT_HEAD_OID: &str = "ALTER TABLE draft_comments ADD COLUMN head_oid TEXT;";

const AGENT_SESSION_CONTEXT: &str = "
ALTER TABLE agent_sessions ADD COLUMN nonce TEXT NOT NULL DEFAULT '';
ALTER TABLE agent_sessions ADD COLUMN allowed_files TEXT NOT NULL DEFAULT '[]';
CREATE INDEX idx_agent_sessions_updated ON agent_sessions(updated_at);
";

const SESSION_LIFECYCLE: &str = "
ALTER TABLE agent_sessions ADD COLUMN model_label TEXT;
ALTER TABLE agent_sessions ADD COLUMN failure_reason TEXT;
ALTER TABLE draft_comments ADD COLUMN finding_id INTEGER;
";

const SESSION_TRANSPORT: &str =
    "ALTER TABLE agent_sessions ADD COLUMN transport TEXT NOT NULL DEFAULT 'acp';";

const SESSION_PERMISSION_MODE: &str =
    "ALTER TABLE agent_sessions ADD COLUMN permission_mode TEXT NOT NULL DEFAULT 'supervised';";

const ISSUE_TABLES: &str = "
CREATE TABLE issues (
    host TEXT NOT NULL,
    id TEXT NOT NULL,
    repo TEXT NOT NULL,
    number INTEGER NOT NULL,
    data TEXT NOT NULL,
    updated_at INTEGER NOT NULL,
    synced_at INTEGER NOT NULL,
    PRIMARY KEY (host, id)
);
CREATE TABLE sync_state (
    host TEXT NOT NULL,
    key TEXT NOT NULL,
    value TEXT NOT NULL,
    PRIMARY KEY (host, key)
);
";

const SESSION_KIND_AND_PLAN_DRAFTS: &str = "
ALTER TABLE agent_sessions ADD COLUMN kind TEXT NOT NULL DEFAULT 'review';
ALTER TABLE agent_sessions ADD COLUMN base_sha TEXT NULL;
CREATE TABLE plan_drafts (
    id INTEGER PRIMARY KEY,
    host TEXT NOT NULL,
    issue_id TEXT NOT NULL,
    session_id TEXT NOT NULL,
    body TEXT NOT NULL,
    base_sha TEXT NOT NULL,
    agent_body TEXT NULL,
    edited INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    published_at INTEGER NULL,
    comment_id TEXT NULL,
    comment_url TEXT NULL
);
CREATE INDEX idx_plan_drafts_issue ON plan_drafts(host, issue_id);
CREATE INDEX idx_plan_drafts_session ON plan_drafts(session_id);
";

const PR_SUMMARIES: &str = "
CREATE TABLE pr_summaries (
    id INTEGER PRIMARY KEY,
    host TEXT NOT NULL,
    pr_id TEXT NOT NULL,
    session_id TEXT NOT NULL,
    body TEXT NOT NULL,
    head_sha TEXT NOT NULL,
    touched TEXT NULL,
    created_at INTEGER NOT NULL
);
CREATE INDEX idx_pr_summaries_pr ON pr_summaries(host, pr_id);
CREATE INDEX idx_pr_summaries_session ON pr_summaries(session_id);
CREATE TABLE project_overviews (
    host TEXT NOT NULL,
    repo TEXT NOT NULL,
    body TEXT NOT NULL,
    diagram TEXT NULL,
    source_sha TEXT NOT NULL,
    source_pr INTEGER NOT NULL,
    session_id TEXT NOT NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (host, repo)
);
ALTER TABLE agent_sessions ADD COLUMN related_worktrees TEXT NULL;
";

const MIGRATIONS: &[M<'static>] = &[
    M::up(INITIAL),
    M::up(COMPOSITE_PR_KEY),
    M::up(DRAFT_HEAD_OID),
    M::up(AGENT_SESSION_CONTEXT),
    M::up(SESSION_LIFECYCLE),
    M::up(SESSION_TRANSPORT),
    M::up(SESSION_PERMISSION_MODE),
    M::up(ISSUE_TABLES),
    M::up(SESSION_KIND_AND_PLAN_DRAFTS),
    M::up(PR_SUMMARIES),
];

fn migrations() -> Migrations<'static> {
    Migrations::from_slice(MIGRATIONS)
}

pub(crate) fn migrate(conn: &mut Connection) -> Result<()> {
    migrations().to_latest(conn)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use argon_core::fixtures::{days_ago, issue_on, pr_on};
    use argon_core::{DEFAULT_HOST, PrKey};
    use rusqlite::params;

    use super::*;
    use crate::Store;

    const GHE: &str = "ghe.acme.io";

    #[test]
    fn migrations_are_valid() {
        migrations().validate().unwrap();
    }

    fn version_one() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", true).unwrap();
        migrations().to_version(&mut conn, 1).unwrap();
        conn
    }

    fn insert_v1_pr(conn: &Connection, host: &str, id: &str, days: i64) {
        let pr = pr_on(host, id, days_ago(days));
        conn.execute(
            "INSERT INTO pull_requests (id, host, repo, number, data, updated_at, synced_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0)",
            params![
                id,
                host,
                pr.repo,
                482,
                serde_json::to_string(&pr).unwrap(),
                pr.updated_at.timestamp_millis()
            ],
        )
        .unwrap();
    }

    fn names(conn: &Connection, kind: &str) -> Vec<String> {
        let mut stmt = conn
            .prepare("SELECT name FROM sqlite_master WHERE type = ?1 ORDER BY name")
            .unwrap();
        stmt.query_map([kind], |row| row.get(0))
            .unwrap()
            .collect::<std::result::Result<_, _>>()
            .unwrap()
    }

    #[test]
    fn migration_two_preserves_prs_and_local_state() {
        let mut conn = version_one();
        insert_v1_pr(&conn, DEFAULT_HOST, "seen", 3);
        insert_v1_pr(&conn, GHE, "archived", 4);
        insert_v1_pr(&conn, DEFAULT_HOST, "fresh", 1);
        conn.execute_batch(&format!(
            "INSERT INTO pr_local (pr_id, last_seen_updated_at, archived_at) VALUES
                ('seen', {seen}, NULL),
                ('archived', NULL, {archived}),
                ('orphan', 1, NULL);
             INSERT INTO draft_reviews (pr_id, body, event) VALUES ('archived', 'lgtm', 'APPROVE');",
            seen = days_ago(3).timestamp_millis(),
            archived = days_ago(2).timestamp_millis(),
        ))
        .unwrap();

        migrate(&mut conn).unwrap();

        let hosts: Vec<(String, String)> = conn
            .prepare("SELECT host, pr_id FROM pr_local ORDER BY pr_id")
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .collect::<std::result::Result<_, _>>()
            .unwrap();
        assert_eq!(
            hosts,
            [
                (GHE.to_string(), "archived".to_string()),
                (DEFAULT_HOST.to_string(), "orphan".to_string()),
                (DEFAULT_HOST.to_string(), "seen".to_string()),
            ]
        );
        let review_host: String = conn
            .query_row("SELECT host FROM draft_reviews", [], |row| row.get(0))
            .unwrap();
        assert_eq!(review_host, GHE);
        assert!(!names(&conn, "index").contains(&"idx_agent_events_session_seq".to_string()));
        assert!(names(&conn, "table").contains(&"viewers".to_string()));

        let store = Store { conn };
        let listed: Vec<(PrKey, bool, bool)> = store
            .list_prs()
            .unwrap()
            .into_iter()
            .map(|(pr, state)| (pr.key(), state.unread, state.archived))
            .collect();
        assert_eq!(
            listed,
            [
                (PrKey::new(DEFAULT_HOST, "fresh"), true, false),
                (PrKey::new(DEFAULT_HOST, "seen"), false, false),
                (PrKey::new(GHE, "archived"), true, true),
            ]
        );
    }

    #[test]
    fn migration_three_adds_head_oid_and_keeps_drafts() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", true).unwrap();
        migrations().to_version(&mut conn, 2).unwrap();
        conn.execute_batch(
            "INSERT INTO draft_comments
                (host, pr_id, path, line, side, start_line, start_side, body, source, session_id, created_at)
             VALUES ('github.com', 'PR_1', 'src/a.ts', 12, 'RIGHT', 10, 'RIGHT', 'keep me', 'user', NULL, 1);
             INSERT INTO draft_reviews (host, pr_id, body, event)
             VALUES ('github.com', 'PR_1', 'general', 'COMMENT');",
        )
        .unwrap();

        migrate(&mut conn).unwrap();

        let store = Store { conn };
        let pr = PrKey::new(DEFAULT_HOST, "PR_1");
        let drafts = store.drafts(&pr).unwrap();
        assert_eq!(drafts.len(), 1);
        assert_eq!(drafts[0].body, "keep me");
        assert_eq!(drafts[0].start_line, Some(10));
        assert_eq!(drafts[0].head_oid, None);
        assert_eq!(store.draft_review(&pr).unwrap().unwrap().body, "general");
    }

    #[test]
    fn migration_four_adds_session_context_with_defaults() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", true).unwrap();
        migrations().to_version(&mut conn, 3).unwrap();
        conn.execute_batch(
            "INSERT INTO agent_sessions
                (id, host, pr_id, agent, model, template, status, created_at, updated_at)
             VALUES ('s1', 'github.com', 'PR_1', 'codex', 'm', 'review-padrao', 'completed', 1, 2);",
        )
        .unwrap();

        migrate(&mut conn).unwrap();

        let store = Store { conn };
        let session = store.session("s1").unwrap().unwrap();
        assert_eq!(session.nonce, "");
        assert!(session.allowed_files.is_empty());
        assert_eq!(session.agent, argon_core::AgentKind::Codex);
    }

    #[test]
    fn migration_five_adds_lifecycle_columns_and_keeps_rows() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", true).unwrap();
        migrations().to_version(&mut conn, 4).unwrap();
        conn.execute_batch(
            "INSERT INTO agent_sessions
                (id, host, pr_id, agent, model, template, status, created_at, updated_at)
             VALUES ('s1', 'github.com', 'PR_1', 'codex', 'sonnet', 't', 'running', 1, 2);
             INSERT INTO draft_comments
                (host, pr_id, path, line, side, body, source, created_at)
             VALUES ('github.com', 'PR_1', 'a.rs', 3, 'RIGHT', 'keep', 'agent', 1);",
        )
        .unwrap();

        migrate(&mut conn).unwrap();

        let store = Store { conn };
        let session = store.session("s1").unwrap().unwrap();
        assert_eq!(session.model_label, None);
        assert_eq!(session.failure_reason, None);
        let drafts = store.drafts(&PrKey::new(DEFAULT_HOST, "PR_1")).unwrap();
        assert_eq!(drafts[0].body, "keep");
    }

    #[test]
    fn migration_six_defaults_existing_sessions_to_acp() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", true).unwrap();
        migrations().to_version(&mut conn, 5).unwrap();
        conn.execute_batch(
            "INSERT INTO agent_sessions
                (id, host, pr_id, agent, model, template, status, acp_session_id, created_at, updated_at)
             VALUES ('s1', 'github.com', 'PR_1', 'codex', 'm', 't', 'completed', 'ses_1', 1, 2);",
        )
        .unwrap();

        migrate(&mut conn).unwrap();

        let store = Store { conn };
        let session = store.session("s1").unwrap().unwrap();
        assert_eq!(session.transport, "acp");
        assert_eq!(session.acp_session_id.as_deref(), Some("ses_1"));
    }

    #[test]
    fn migration_seven_defaults_existing_sessions_to_supervised() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", true).unwrap();
        migrations().to_version(&mut conn, 6).unwrap();
        conn.execute_batch(
            "INSERT INTO agent_sessions
                (id, host, pr_id, agent, model, template, status, acp_session_id, created_at, updated_at)
             VALUES ('s1', 'github.com', 'PR_1', 'codex', 'm', 't', 'completed', 'ses_1', 1, 2);",
        )
        .unwrap();

        migrate(&mut conn).unwrap();

        let store = Store { conn };
        let session = store.session("s1").unwrap().unwrap();
        assert_eq!(session.permission_mode, "supervised");
        assert_eq!(session.transport, "acp");
    }

    fn pr_json_before_issues(pr: &argon_core::PullRequest) -> String {
        let mut json = serde_json::to_value(pr).unwrap();
        let object = json.as_object_mut().unwrap();
        object.remove("closing_issues");
        object.insert(
            "involvement".into(),
            serde_json::json!({
                "requested": true,
                "assigned": false,
                "authored": false,
                "reviewed": false,
            }),
        );
        json.to_string()
    }

    #[test]
    fn migration_eight_adds_issue_tables_and_keeps_pull_requests() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", true).unwrap();
        migrations().to_version(&mut conn, 7).unwrap();
        let tables = names(&conn, "table");
        assert!(!tables.contains(&"issues".to_string()));
        assert!(!tables.contains(&"sync_state".to_string()));

        let pr = pr_on(DEFAULT_HOST, "PR_old", days_ago(2));
        conn.execute(
            "INSERT INTO pull_requests (host, id, repo, number, data, updated_at, synced_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0)",
            params![
                DEFAULT_HOST,
                "PR_old",
                pr.repo,
                482,
                pr_json_before_issues(&pr),
                pr.updated_at.timestamp_millis()
            ],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO pr_local (host, pr_id, last_seen_updated_at) VALUES (?1, 'PR_old', ?2)",
            params![DEFAULT_HOST, days_ago(3).timestamp_millis()],
        )
        .unwrap();

        migrate(&mut conn).unwrap();

        let tables = names(&conn, "table");
        assert!(tables.contains(&"issues".to_string()));
        assert!(tables.contains(&"sync_state".to_string()));
        let mut store = Store { conn };
        assert!(store.list_issues().unwrap().is_empty());
        assert!(!store.issue_baseline_done(DEFAULT_HOST).unwrap());

        let listed = store.list_prs().unwrap();
        assert_eq!(listed.len(), 1);
        let mut expected = pr;
        expected.involvement.requested = true;
        assert_eq!(listed[0].0, expected);
        assert_eq!(listed[0].1.last_seen, Some(days_ago(3)));
        assert!(listed[0].1.unread);

        let issue = issue_on(DEFAULT_HOST, "ISSUE_1", days_ago(1));
        store
            .upsert_issues(DEFAULT_HOST, std::slice::from_ref(&issue))
            .unwrap();
        assert_eq!(store.list_issues().unwrap()[0].0, issue);
        assert_eq!(store.list_prs().unwrap().len(), 1);
    }

    #[test]
    fn migration_nine_defaults_existing_sessions_to_review_and_adds_plan_drafts() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", true).unwrap();
        migrations().to_version(&mut conn, 8).unwrap();
        assert!(!names(&conn, "table").contains(&"plan_drafts".to_string()));
        conn.execute_batch(
            "INSERT INTO agent_sessions
                (id, host, pr_id, agent, model, template, status, acp_session_id, created_at, updated_at)
             VALUES ('s1', 'github.com', 'PR_1', 'codex', 'm', 't', 'completed', 'ses_1', 1, 2);",
        )
        .unwrap();

        migrate(&mut conn).unwrap();

        assert!(names(&conn, "table").contains(&"plan_drafts".to_string()));
        let indexes = names(&conn, "index");
        assert!(indexes.contains(&"idx_plan_drafts_issue".to_string()));
        assert!(indexes.contains(&"idx_plan_drafts_session".to_string()));
        let kind: String = conn
            .query_row(
                "SELECT kind FROM agent_sessions WHERE id = 's1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(kind, "review");
        let store = Store { conn };
        let session = store.session("s1").unwrap().unwrap();
        assert_eq!(session.kind, argon_core::SessionKind::Review);
        assert_eq!(session.base_sha, None);
        assert_eq!(session.acp_session_id.as_deref(), Some("ses_1"));
    }

    #[test]
    fn migration_ten_adds_summary_tables_and_related_worktrees() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", true).unwrap();
        migrations().to_version(&mut conn, 9).unwrap();
        let tables = names(&conn, "table");
        assert!(!tables.contains(&"pr_summaries".to_string()));
        assert!(!tables.contains(&"project_overviews".to_string()));
        conn.execute_batch(
            "INSERT INTO agent_sessions
                (id, host, pr_id, agent, model, template, status, acp_session_id, created_at, updated_at)
             VALUES ('s1', 'github.com', 'PR_1', 'codex', 'm', 't', 'completed', 'ses_1', 1, 2);",
        )
        .unwrap();

        migrate(&mut conn).unwrap();

        let tables = names(&conn, "table");
        assert!(tables.contains(&"pr_summaries".to_string()));
        assert!(tables.contains(&"project_overviews".to_string()));
        let indexes = names(&conn, "index");
        assert!(indexes.contains(&"idx_pr_summaries_pr".to_string()));
        assert!(indexes.contains(&"idx_pr_summaries_session".to_string()));
        let store = Store { conn };
        let session = store.session("s1").unwrap().unwrap();
        assert!(session.related_worktrees.is_empty());
        assert_eq!(session.acp_session_id.as_deref(), Some("ses_1"));
    }
}
