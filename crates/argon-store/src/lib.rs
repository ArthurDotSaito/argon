mod agent_log;
mod agents;
mod drafts;
mod error;
mod issues;
mod lifecycle;
mod local;
mod plans;
mod prs;
mod schema;
mod viewers;

use std::path::Path;

use rusqlite::Connection;

pub use agent_log::SessionWrite;
pub use agents::{FindingState, NewAgentSession, SessionRow, SessionStatus, StoredFinding};
pub use error::{Result, StoreError};
pub use lifecycle::{ARCHIVE_PENDING, StaleWorktree};
pub use local::LocalState;
pub use plans::PlanDraft;
pub use viewers::{CachedOrg, CachedViewer};

pub struct Store {
    conn: Connection,
}

impl Store {
    pub fn open(path: &Path) -> Result<Store> {
        let conn = Connection::open(path)?;
        conn.pragma_update_and_check(None, "journal_mode", "WAL", |row| row.get::<_, String>(0))?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Store> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(mut conn: Connection) -> Result<Store> {
        conn.pragma_update(None, "foreign_keys", true)?;
        schema::migrate(&mut conn)?;
        Ok(Store { conn })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_send<T: Send>() {}

    #[test]
    fn store_is_send() {
        assert_send::<Store>();
    }

    #[test]
    fn open_creates_file_database_in_wal_mode() {
        let dir = std::env::temp_dir().join(format!("argon-store-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("argon.db");
        let store = Store::open(&path).unwrap();
        let mode: String = store
            .conn
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))
            .unwrap();
        drop(store);
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(mode, "wal");
    }
}
