use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, PoisonError, RwLock};

use argon_core::{Config, Paths};
use argon_github::{Account, GithubClient, Viewer};
use argon_store::CachedViewer;
use tracing::{info, warn};

use super::orgs::{admin_orgs, merge_tracked, viewer_from_cache};

pub(super) struct Shared {
    config: RwLock<Config>,
    config_version: AtomicU64,
    accounts: RwLock<Vec<Account>>,
    admin_orgs: RwLock<Vec<String>>,
    clients: RwLock<HashMap<String, Arc<GithubClient>>>,
}

impl Shared {
    pub(super) fn new(config: Config) -> Self {
        Self {
            config: RwLock::new(config),
            config_version: AtomicU64::new(0),
            accounts: RwLock::default(),
            admin_orgs: RwLock::default(),
            clients: RwLock::default(),
        }
    }

    pub(super) fn config(&self) -> Config {
        self.config
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    pub(super) fn update_config(&self, f: impl FnOnce(&mut Config)) -> (u64, Config) {
        let mut config = self.config.write().unwrap_or_else(PoisonError::into_inner);
        f(&mut config);
        let version = self.config_version.fetch_add(1, Ordering::SeqCst) + 1;
        (version, config.clone())
    }

    pub(super) fn accounts(&self) -> Vec<Account> {
        self.accounts
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    pub(super) fn admin_orgs(&self) -> Vec<String> {
        self.admin_orgs
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    pub(super) fn client(&self, host: &str) -> Option<Arc<GithubClient>> {
        self.clients
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&host_key(host))
            .cloned()
    }

    pub(super) fn register_client(
        &self,
        host: &str,
        client: Arc<GithubClient>,
    ) -> Arc<GithubClient> {
        self.clients
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .entry(host_key(host))
            .or_insert(client)
            .clone()
    }

    pub(super) fn connect(&self, host: &str, viewer: &Viewer) {
        self.upsert_account(Account {
            host: host.to_string(),
            login: viewer.login.clone(),
        });
        self.add_admin_orgs(admin_orgs(&[(host.to_string(), viewer.clone())]));
    }

    pub(super) fn load_cached(&self, cached: &[CachedViewer]) {
        let known = self.accounts();
        for entry in cached {
            if known
                .iter()
                .any(|account| account.host.eq_ignore_ascii_case(&entry.host))
            {
                continue;
            }
            self.connect(&entry.host, &viewer_from_cache(entry));
        }
        info!(viewers = cached.len(), "loaded cached gh accounts");
    }

    pub(super) fn drop_client(&self, host: &str) {
        self.clients
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&host_key(host));
    }

    pub(super) fn forget(&self, host: &str) {
        self.drop_client(host);
        self.accounts
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .retain(|account| !account.host.eq_ignore_ascii_case(host));
    }

    fn upsert_account(&self, account: Account) {
        let mut accounts = self
            .accounts
            .write()
            .unwrap_or_else(PoisonError::into_inner);
        match accounts
            .iter_mut()
            .find(|existing| existing.host.eq_ignore_ascii_case(&account.host))
        {
            Some(existing) => *existing = account,
            None => accounts.push(account),
        }
    }

    fn add_admin_orgs(&self, orgs: Vec<String>) {
        let mut admin_orgs = self
            .admin_orgs
            .write()
            .unwrap_or_else(PoisonError::into_inner);
        merge_tracked(&mut admin_orgs, orgs);
    }

    pub(super) fn seed_tracked(&self, admin_orgs: Vec<String>, paths: &Paths) {
        let snapshot = {
            let mut config = self.config.write().unwrap_or_else(PoisonError::into_inner);
            merge_tracked(&mut config.orgs.tracked, admin_orgs);
            config.orgs.initialized = true;
            config.clone()
        };
        match snapshot.save(&paths.config_file()) {
            Ok(()) => info!(
                tracked = snapshot.orgs.tracked.len(),
                "initialized tracked organizations"
            ),
            Err(error) => warn!(%error, "failed to save initialized config"),
        }
    }
}

fn host_key(host: &str) -> String {
    host.to_ascii_lowercase()
}
