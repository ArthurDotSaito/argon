mod logging;
mod orgs;
mod shared;
mod store_handle;
mod sync;

use std::sync::Arc;

use argon_core::{Config, Paths};
use argon_github::Account;
use futures::channel::mpsc::{self, UnboundedReceiver, UnboundedSender};
use gpui_kit::Global;

pub use logging::init as init_logging;
pub use store_handle::StoreHandle;
pub use sync::SyncEvent;

use shared::Shared;
use sync::{SyncRequest, SyncWorker};

#[derive(Clone)]
pub struct Services {
    requests: UnboundedSender<SyncRequest>,
    store: StoreHandle,
    shared: Arc<Shared>,
}

impl Services {
    pub fn start(paths: Paths, config: Config) -> (Services, UnboundedReceiver<SyncEvent>) {
        let (events, receiver) = mpsc::unbounded();
        let (requests, queue) = mpsc::unbounded();
        let (store, store_startup) = StoreHandle::spawn(paths.data_dir.clone(), paths.db_file());
        let shared = Arc::new(Shared::new(config));
        SyncWorker {
            paths,
            shared: shared.clone(),
            store: store.clone(),
            store_startup,
            events,
            requests: queue,
        }
        .spawn();
        let services = Services {
            requests,
            store,
            shared,
        };
        (services, receiver)
    }

    pub fn sync_now(&self) {
        _ = self.requests.unbounded_send(SyncRequest);
    }

    pub fn store(&self) -> StoreHandle {
        self.store.clone()
    }

    pub fn config(&self) -> Config {
        self.shared.config()
    }

    pub fn accounts(&self) -> Vec<Account> {
        self.shared.accounts()
    }

    pub fn admin_orgs(&self) -> Vec<String> {
        self.shared.admin_orgs()
    }
}

pub struct AppServices(pub Services);

impl Global for AppServices {}
