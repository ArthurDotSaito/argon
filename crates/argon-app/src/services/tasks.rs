use std::future::Future;
use std::panic::AssertUnwindSafe;

use futures::FutureExt as _;
use futures::channel::oneshot;
use gpui_kit::{App, AppContext as _, Task};
use tokio::runtime::Handle;
use tokio::task::AbortHandle;

use super::error::ServiceError;

struct AbortOnDrop(Option<AbortHandle>);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        if let Some(handle) = &self.0 {
            handle.abort();
        }
    }
}

pub(super) fn run_on<F, R>(
    runtime: Option<&Handle>,
    cx: &App,
    fut: F,
) -> Task<Result<R, ServiceError>>
where
    F: Future<Output = Result<R, ServiceError>> + Send + 'static,
    R: Send + 'static,
{
    let (sender, receiver) = oneshot::channel();
    let abort = runtime.map(|runtime| {
        runtime
            .spawn(async move {
                let result = AssertUnwindSafe(fut).catch_unwind().await;
                _ = sender.send(result.ok());
            })
            .abort_handle()
    });
    let guard = AbortOnDrop(abort);
    cx.background_spawn(async move {
        let _guard = guard;
        match receiver.await {
            Ok(Some(result)) => result,
            Ok(None) => Err(ServiceError::Panicked),
            Err(_) => Err(ServiceError::Stopped),
        }
    })
}
