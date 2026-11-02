use std::future::Future;
use std::sync::Arc;

use argon_github::{GithubClient, GithubError};
use gpui_kit::component::WindowExt as _;
use gpui_kit::component::notification::Notification;
use gpui_kit::{Context, Task, Window};

use super::{ReviewScope, ReviewTab, ReviewTabEvent};
use crate::review::drafts::{DraftsEvent, EventOptions};
use crate::review::finish_popover::{self, FinishEvent};
use crate::review::labels;
use crate::services::{AppServices, ServiceError};

impl ReviewTab {
    pub(super) fn github<F, Fut, R>(
        &self,
        cx: &mut Context<Self>,
        f: F,
    ) -> Option<Task<Result<R, ServiceError>>>
    where
        F: FnOnce(Arc<GithubClient>) -> Fut + Send + 'static,
        Fut: Future<Output = Result<R, GithubError>> + Send,
        R: Send + 'static,
    {
        let host = self.detail.as_ref()?.pr.host.clone();
        let services = cx.try_global::<AppServices>()?.0.clone();
        Some(services.spawn_github(cx, host, f))
    }

    pub(super) fn set_thread_resolved(
        &mut self,
        id: String,
        resolved: bool,
        cx: &mut Context<Self>,
    ) {
        let thread = id.clone();
        let Some(job) = self.github(cx, move |client| async move {
            client.set_thread_resolved(&thread, resolved).await
        }) else {
            let error = labels::resolve_failed(labels::SERVICES_UNAVAILABLE).into();
            self.thread_ui
                .update(cx, |ui, cx| ui.resolved_done(&id, Some(error), cx));
            return;
        };
        let key = id.clone();
        let task = cx.spawn(async move |this, cx| {
            let result = job.await;
            _ = this.update(cx, |this, cx| {
                let error = result.err().map(|error| {
                    tracing::warn!(%error, "failed to update review thread resolution");
                    labels::resolve_failed(&error.to_string()).into()
                });
                let reload = error.is_none();
                this.thread_ui
                    .update(cx, |ui, cx| ui.resolved_done(&id, error, cx));
                if reload {
                    cx.emit(ReviewTabEvent::Reload);
                }
            });
        });
        self.thread_tasks.insert(key, task);
    }

    pub fn open_finish(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self
            .finish
            .as_ref()
            .is_some_and(|(finish, _)| finish.upgrade().is_some())
        {
            return;
        }
        let Some(author) = self.detail.as_ref().map(|detail| detail.pr.author.clone()) else {
            return;
        };
        let options = EventOptions::for_viewer(&author, self.viewer_login.as_deref());
        let finish = finish_popover::open(self.drafts.clone(), options, window, cx);
        let subscription = cx.subscribe(&finish, |this, _, event, cx| match event {
            FinishEvent::Reveal { path, line, side } => {
                if this.scope != ReviewScope::All {
                    this.set_scope(ReviewScope::All, cx);
                }
                this.reveal(path, *line, *side, cx);
            }
        });
        self.finish = Some((finish.downgrade(), subscription));
    }

    pub(super) fn on_drafts_event(
        &mut self,
        event: &DraftsEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            DraftsEvent::Submitted => {
                let open = self
                    .finish
                    .take()
                    .is_some_and(|(finish, _)| finish.upgrade().is_some());
                if open && window.has_active_dialog(cx) {
                    window.close_dialog(cx);
                }
                window.push_notification(Notification::success(labels::REVIEW_SENT), cx);
                cx.emit(ReviewTabEvent::Reload);
            }
            DraftsEvent::Failed(error) => {
                self.error = Some(error.clone());
                cx.notify();
            }
        }
    }
}
