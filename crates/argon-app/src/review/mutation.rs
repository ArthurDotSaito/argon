use std::future::Future;

use gpui_kit::component::WindowExt as _;
use gpui_kit::component::notification::Notification;
use gpui_kit::{AnyWindowHandle, Context};

pub fn run_detached<T: 'static, R: 'static>(
    window: AnyWindowHandle,
    cx: &mut Context<T>,
    job: impl Future<Output = R> + 'static,
    apply: impl FnOnce(&mut T, R, &mut Context<T>) + 'static,
    orphan: impl FnOnce(R) -> Notification + 'static,
) {
    cx.spawn(async move |this, cx| {
        let result = job.await;
        match this.upgrade() {
            Some(entity) => {
                entity.update(cx, |this, cx| apply(this, result, cx));
            }
            None => {
                let note = orphan(result);
                _ = window.update(cx, |_, window, cx| window.push_notification(note, cx));
            }
        }
    })
    .detach();
}
