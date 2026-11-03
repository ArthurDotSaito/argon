use futures::channel::mpsc::UnboundedSender;
use tauri_winrt_notification::Toast;

use super::toast::{ToastSpec, ToastTarget};

const TOAST_TITLE: &str = "Argon";

pub(super) fn show(spec: &ToastSpec, clicks: &UnboundedSender<ToastTarget>) {
    let target = spec.target.clone();
    let clicks = clicks.clone();
    let shown = Toast::new(Toast::POWERSHELL_APP_ID)
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
