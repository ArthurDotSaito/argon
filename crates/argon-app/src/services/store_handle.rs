use std::fmt::Display;
use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;

use argon_store::Store;
use futures::channel::oneshot;

type Job = Box<dyn FnOnce(&mut Store) + Send>;

const THREAD_NAME: &str = "argon-store";

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum StoreHandleError {
    #[error("store job panicked")]
    Panicked,
    #[error("store thread is not running")]
    Stopped,
}

pub type StoreStartup = oneshot::Receiver<Option<String>>;

#[derive(Clone)]
pub struct StoreHandle {
    jobs: mpsc::Sender<Job>,
}

impl StoreHandle {
    pub fn spawn(data_dir: PathBuf, db_file: PathBuf) -> (StoreHandle, StoreStartup) {
        let (jobs, queue) = mpsc::channel::<Job>();
        let (opened, startup) = oneshot::channel();
        let spawned = thread::Builder::new()
            .name(THREAD_NAME.into())
            .spawn(move || {
                let (store, error) = open(&data_dir, &db_file);
                _ = opened.send(error);
                let Some(mut store) = store else {
                    return;
                };
                for job in queue {
                    job(&mut store);
                }
            });
        if let Err(error) = spawned {
            tracing::error!(%error, "failed to start store thread");
        }
        (StoreHandle { jobs }, startup)
    }

    pub fn submit(&self, f: impl FnOnce(&mut Store) + Send + 'static) {
        let job: Job = Box::new(move |store| {
            if panic::catch_unwind(AssertUnwindSafe(|| f(store))).is_err() {
                tracing::error!("store job panicked");
            }
        });
        if self.jobs.send(job).is_err() {
            tracing::warn!("store thread is not running");
        }
    }

    pub async fn call<R: Send + 'static>(
        &self,
        f: impl FnOnce(&mut Store) -> R + Send + 'static,
    ) -> Result<R, StoreHandleError> {
        let (reply, response) = oneshot::channel();
        let job: Job = Box::new(move |store| {
            let result = panic::catch_unwind(AssertUnwindSafe(|| f(store)));
            _ = reply.send(result.ok());
        });
        self.jobs.send(job).map_err(|_| StoreHandleError::Stopped)?;
        match response.await {
            Ok(Some(value)) => Ok(value),
            Ok(None) => Err(StoreHandleError::Panicked),
            Err(_) => Err(StoreHandleError::Stopped),
        }
    }
}

pub fn flatten<T, E: Display, F: Display>(result: Result<Result<T, E>, F>) -> Result<T, String> {
    result
        .map_err(|error| error.to_string())
        .and_then(|inner| inner.map_err(|error| error.to_string()))
}

fn open(data_dir: &Path, db_file: &Path) -> (Option<Store>, Option<String>) {
    let opened = std::fs::create_dir_all(data_dir)
        .map_err(|error| error.to_string())
        .and_then(|()| Store::open(db_file).map_err(|error| error.to_string()));
    let error = match opened {
        Ok(store) => {
            tracing::info!(path = %db_file.display(), "store opened");
            return (Some(store), None);
        }
        Err(error) => error,
    };
    tracing::error!(path = %db_file.display(), %error, "failed to open store, using in-memory database");
    let message = format!("falha ao abrir o banco local: {error}");
    match Store::open_in_memory() {
        Ok(store) => (Some(store), Some(message)),
        Err(fallback) => {
            tracing::error!(error = %fallback, "failed to open in-memory store");
            (None, Some(message))
        }
    }
}
