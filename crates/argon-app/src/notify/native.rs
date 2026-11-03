use futures::channel::mpsc::UnboundedSender;
use tauri_winrt_notification::Toast;

use super::toast::{ToastSpec, ToastTarget};
use crate::app_id::APP_USER_MODEL_ID;

const TOAST_TITLE: &str = "Argon";

pub(super) fn show(spec: &ToastSpec, clicks: &UnboundedSender<ToastTarget>) {
    let target = spec.target.clone();
    let clicks = clicks.clone();
    let shown = Toast::new(APP_USER_MODEL_ID)
        .title(TOAST_TITLE)
        .text1(&spec.body)
        .on_activated(move |_| {
            clicks.unbounded_send(target.clone()).ok();
            Ok(())
        })
        .show();
    if let Err(error) = shown {
        tracing::warn!(%error, "failed to show toast");
    }
}
