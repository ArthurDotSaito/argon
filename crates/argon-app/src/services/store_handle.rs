use std::panic::{self, AssertUnwindSafe};
use std::sync::mpsc;
use std::thread;

use argon_store::Store;
use futures::channel::oneshot;

type Job = Box<dyn FnOnce(&mut Store) + Send>;

const THREAD_NAME: &str = "argon-store";

#[derive(Clone)]
pub struct StoreHandle {
    jobs: mpsc::Sender<Job>,
}

impl StoreHandle {
    pub fn spawn(mut store: Store) -> StoreHandle {
        let (jobs, queue) = mpsc::channel::<Job>();
        let spawned = thread::Builder::new()
            .name(THREAD_NAME.into())
            .spawn(move || {
                for job in queue {
                    job(&mut store);
                }
            });
        if let Err(error) = spawned {
            tracing::error!(%error, "failed to start store thread");
        }
        StoreHandle { jobs }
    }

    pub async fn call<R: Send + 'static>(
        &self,
        f: impl FnOnce(&mut Store) -> R + Send + 'static,
    ) -> R {
        let (reply, response) = oneshot::channel();
        let job: Job = Box::new(move |store| {
            let result = panic::catch_unwind(AssertUnwindSafe(|| f(store)));
            _ = reply.send(result);
        });
        if self.jobs.send(job).is_err() {
            panic!("store thread is not running");
        }
        match response.await {
            Ok(Ok(value)) => value,
            Ok(Err(payload)) => panic::resume_unwind(payload),
            Err(_) => panic!("store thread dropped the request"),
        }
    }
}
