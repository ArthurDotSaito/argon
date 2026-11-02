mod logging;
mod store_handle;
mod sync;

use std::sync::Arc;

use argon_core::{Config, Paths};
use argon_github::Account;
use argon_store::Store;
use futures::channel::mpsc::{self, UnboundedReceiver, UnboundedSender};
use gpui_kit::Global;

pub use logging::init as init_logging;
pub use store_handle::StoreHandle;
pub use sync::SyncEvent;

use sync::{Shared, SyncRequest, SyncWorker};

const LOCAL_STORE_HOST: &str = "argon.db";

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
        let store = StoreHandle::spawn(open_store(&paths, &events));
        let shared = Arc::new(Shared::new(config));
        SyncWorker {
            paths,
            shared: shared.clone(),
            store: store.clone(),
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

    #[expect(dead_code, reason = "consumed by the inbox model")]
    pub fn store(&self) -> StoreHandle {
        self.store.clone()
    }

    #[expect(dead_code, reason = "consumed by the inbox model")]
    pub fn config(&self) -> Config {
        self.shared.config()
    }

    pub fn accounts(&self) -> Vec<Account> {
        self.shared.accounts()
    }
}

pub struct AppServices(pub Services);

impl Global for AppServices {}

fn open_store(paths: &Paths, events: &UnboundedSender<SyncEvent>) -> Store {
    let path = paths.db_file();
    let opened = std::fs::create_dir_all(&paths.data_dir)
        .map_err(|error| error.to_string())
        .and_then(|()| Store::open(&path).map_err(|error| error.to_string()));
    match opened {
        Ok(store) => store,
        Err(error) => {
            tracing::error!(path = %path.display(), %error, "failed to open store, using in-memory database");
            _ = events.unbounded_send(SyncEvent::Failed {
                host: LOCAL_STORE_HOST.into(),
                error: format!("falha ao abrir o banco local: {error}"),
            });
            Store::open_in_memory().expect("in-memory store must open")
        }
    }
}
