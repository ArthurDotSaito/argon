mod error;
mod logging;
mod orgs;
mod shared;
mod store_handle;
mod sync;
mod tasks;

use std::future::Future;
use std::sync::Arc;

use argon_core::{Config, Paths};
use argon_github::{Account, GithubClient, token_for};
use futures::channel::mpsc::{self, UnboundedReceiver, UnboundedSender};
use gpui_kit::{App, Global, Task};
use tokio::runtime::Handle;

pub use error::ServiceError;
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
    runtime: Option<Handle>,
}

impl Services {
    pub fn start(paths: Paths, config: Config) -> (Services, UnboundedReceiver<SyncEvent>) {
        let (events, receiver) = mpsc::unbounded();
        let (requests, queue) = mpsc::unbounded();
        let (store, store_startup) = StoreHandle::spawn(paths.data_dir.clone(), paths.db_file());
        let shared = Arc::new(Shared::new(config));
        load_cached_viewers(&store, shared.clone());
        let runtime = SyncWorker {
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
            runtime,
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

#[expect(
    dead_code,
    reason = "bridge for the PR detail, comment and merge actions of Tasks 9 and 10"
)]
impl Services {
    pub async fn github(&self, host: &str) -> Result<Arc<GithubClient>, ServiceError> {
        if let Some(client) = self.shared.client(host) {
            return Ok(client);
        }
        let token = token_for(host).await?;
        let client = Arc::new(GithubClient::new(host, token));
        tracing::info!(host, "github client connected on demand");
        Ok(self.shared.register_client(host, client))
    }

    pub fn spawn<F, R>(&self, cx: &App, fut: F) -> Task<Result<R, ServiceError>>
    where
        F: Future<Output = R> + Send + 'static,
        R: Send + 'static,
    {
        tasks::spawn_on(self.runtime.as_ref(), cx, fut)
    }
}

fn load_cached_viewers(store: &StoreHandle, shared: Arc<Shared>) {
    store.submit(move |store| match store.viewers() {
        Ok(viewers) => shared.load_cached(&viewers),
        Err(error) => tracing::warn!(%error, "failed to load cached gh accounts"),
    });
}

pub struct AppServices(pub Services);

impl Global for AppServices {}
