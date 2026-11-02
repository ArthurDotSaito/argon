use std::future::Future;
use std::panic::AssertUnwindSafe;

use futures::FutureExt as _;
use futures::channel::oneshot;
use gpui_kit::{App, AppContext as _, Task};
use tokio::runtime::Handle;

use super::error::ServiceError;

pub(super) fn spawn_on<F, R>(
    runtime: Option<&Handle>,
    cx: &App,
    fut: F,
) -> Task<Result<R, ServiceError>>
where
    F: Future<Output = R> + Send + 'static,
    R: Send + 'static,
{
    let (sender, receiver) = oneshot::channel();
    if let Some(runtime) = runtime {
        runtime.spawn(async move {
            let result = AssertUnwindSafe(fut).catch_unwind().await;
            _ = sender.send(result.ok());
        });
    }
    cx.background_spawn(async move {
        match receiver.await {
            Ok(Some(value)) => Ok(value),
            Ok(None) => Err(ServiceError::Panicked),
            Err(_) => Err(ServiceError::Stopped),
        }
    })
}
