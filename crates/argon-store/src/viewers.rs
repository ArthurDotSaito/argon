use chrono::{DateTime, Utc};
use rusqlite::params;
use serde::{Deserialize, Serialize};

use crate::Store;
use crate::error::Result;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CachedOrg {
    pub login: String,
    pub can_admin: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CachedViewer {
    pub host: String,
    pub login: String,
    pub orgs: Vec<CachedOrg>,
    pub updated_at: DateTime<Utc>,
}

impl Store {
    pub fn save_viewer(&mut self, host: &str, login: &str, orgs: &[CachedOrg]) -> Result<()> {
        self.conn.execute(
            "INSERT INTO viewers (host, login, orgs, updated_at) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(host) DO UPDATE SET
                login = excluded.login,
                orgs = excluded.orgs,
                updated_at = excluded.updated_at",
            params![
                host,
                login,
                serde_json::to_string(orgs)?,
                Utc::now().timestamp_millis()
            ],
        )?;
        Ok(())
    }

    pub fn viewers(&self) -> Result<Vec<CachedViewer>> {
        let mut stmt = self
            .conn
            .prepare("SELECT host, login, orgs, updated_at FROM viewers ORDER BY host")?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        rows.into_iter()
            .map(|(host, login, orgs, updated_at)| {
                Ok(CachedViewer {
                    host,
                    login,
                    orgs: serde_json::from_str(&orgs)?,
                    updated_at: DateTime::from_timestamp_millis(updated_at).unwrap_or_default(),
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use argon_core::DEFAULT_HOST;

    use super::CachedOrg;
    use crate::Store;

    fn org(login: &str, can_admin: bool) -> CachedOrg {
        CachedOrg {
            login: login.into(),
            can_admin,
        }
    }

    #[test]
    fn save_viewer_replaces_previous_snapshot_per_host() {
        let mut store = Store::open_in_memory().unwrap();
        store
            .save_viewer(DEFAULT_HOST, "old", &[org("acme", false)])
            .unwrap();
        store
            .save_viewer("ghe.acme.io", "corp", &[org("platform", true)])
            .unwrap();
        store
            .save_viewer(DEFAULT_HOST, "me", &[org("acme", true), org("oss", false)])
            .unwrap();

        let viewers = store.viewers().unwrap();
        let summary: Vec<(&str, &str, &[CachedOrg])> = viewers
            .iter()
            .map(|viewer| {
                (
                    viewer.host.as_str(),
                    viewer.login.as_str(),
                    viewer.orgs.as_slice(),
                )
            })
            .collect();
        assert_eq!(
            summary,
            [
                ("ghe.acme.io", "corp", &[org("platform", true)][..]),
                (
                    DEFAULT_HOST,
                    "me",
                    &[org("acme", true), org("oss", false)][..]
                ),
            ]
        );
    }
}
