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

fn migrations() -> Migrations<'static> {
    Migrations::new(vec![M::up(INITIAL)])
}

pub(crate) fn migrate(conn: &mut Connection) -> Result<()> {
    migrations().to_latest(conn)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_are_valid() {
        migrations().validate().unwrap();
    }
}
