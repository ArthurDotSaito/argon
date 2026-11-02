use std::sync::{PoisonError, RwLock};

use argon_core::{Config, Paths};
use argon_github::Account;
use tracing::{info, warn};

use super::orgs::merge_tracked;

pub(super) struct Shared {
    config: RwLock<Config>,
    accounts: RwLock<Vec<Account>>,
}

impl Shared {
    pub(super) fn new(config: Config) -> Self {
        Self {
            config: RwLock::new(config),
            accounts: RwLock::default(),
        }
    }

    pub(super) fn config(&self) -> Config {
        self.config
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    pub(super) fn accounts(&self) -> Vec<Account> {
        self.accounts
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    pub(super) fn set_accounts(&self, accounts: Vec<Account>) {
        *self
            .accounts
            .write()
            .unwrap_or_else(PoisonError::into_inner) = accounts;
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
