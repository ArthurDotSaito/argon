use std::future::Future;
use std::sync::Arc;

use argon_github::{GithubClient, GithubError};
use gpui_kit::component::WindowExt as _;
use gpui_kit::component::notification::Notification;
use gpui_kit::{AppContext as _, Context, Window};

use super::{ReviewScope, ReviewTab, ReviewTabEvent};
use crate::review::composer::{ComposerEvent, ReviewComposer};
use crate::review::drafts::{DraftsEvent, EventOptions};
use crate::review::labels;
use crate::review::mutation::run_detached;
use crate::services::ServiceError;

impl ReviewTab {
    pub(super) fn github_detached<F, Fut, R>(
        &self,
        f: F,
    ) -> Option<impl Future<Output = Result<R, ServiceError>> + Send + 'static + use<F, Fut, R>>
    where
        F: FnOnce(Arc<GithubClient>) -> Fut + Send + 'static,
        Fut: Future<Output = Result<R, GithubError>> + Send,
        R: Send + 'static,
    {
        let host = self.detail.as_ref()?.pr.host.clone();
        Some(self.services.as_ref()?.spawn_github_detached(host, f))
    }

    pub(super) fn displayed_head(&self, cx: &Context<Self>) -> Option<String> {
        self.loaded_head
            .clone()
            .filter(|_| !self.drafts.read(cx).stale())
    }

    pub(super) fn reload_diff(&mut self, cx: &mut Context<Self>) {
        cx.emit(ReviewTabEvent::Reload);
        if self.scope == ReviewScope::All {
            self.load_files(true, cx);
        }
    }

    pub(super) fn set_thread_resolved(
        &mut self,
        id: String,
        resolved: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let thread = id.clone();
        let number = self.detail.as_ref().map_or(0, |detail| detail.pr.number);
        let Some(job) = self.github_detached(move |client| async move {
            client.set_thread_resolved(&thread, resolved).await
        }) else {
            let error = labels::resolve_failed(labels::SERVICES_UNAVAILABLE).into();
            self.thread_ui
                .update(cx, |ui, cx| ui.resolved_done(&id, Some(error), cx));
            return;
        };
        let job = async move { job.await.map_err(|error| error.to_string()) };
        run_detached(
            window.window_handle(),
            cx,
            job,
            move |this, result, cx| {
                let error = result.err().map(|error| {
                    tracing::warn!(%error, "failed to update review thread resolution");
                    labels::resolve_failed(&error).into()
                });
                let reload = error.is_none();
                this.thread_ui
                    .update(cx, |ui, cx| ui.resolved_done(&id, error, cx));
                if reload {
                    cx.emit(ReviewTabEvent::Reload);
                }
            },
            move |result| match result {
                Ok(()) => Notification::success(labels::thread_updated_in(number)),
                Err(error) => Notification::error(labels::mutation_failed_in(number, &error)),
            },
        );
    }

    pub fn open_finish(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open_composer(true, window, cx);
    }

    fn open_composer(&mut self, focus: bool, window: &mut Window, cx: &mut Context<Self>) {
        if focus {
            self.focus_pending = false;
        }
        if let Some((composer, _)) = &self.composer {
            if focus {
                composer.update(cx, |composer, cx| composer.focus(cx));
            }
            return;
        }
        let Some(author) = self.detail.as_ref().map(|detail| detail.pr.author.clone()) else {
            return;
        };
        let options = EventOptions::for_viewer(&author, self.viewer_login.as_deref());
        let drafts = self.drafts.clone();
        let composer = cx.new(|cx| {
            let mut composer = ReviewComposer::new(drafts, options, window, cx);
            if !focus {
                composer.without_focus();
            }
            composer
        });
        let subscription =
            cx.subscribe_in(
                &composer,
                window,
                |this, _, event, window, cx| match event {
                    ComposerEvent::Reload => this.reload_diff(cx),
                    ComposerEvent::Reveal { path, line, side } => {
                        if this.scope != ReviewScope::All {
                            this.set_scope(ReviewScope::All, cx);
                        }
                        this.reveal(path, *line, *side, cx);
                    }
                    ComposerEvent::Collapse => this.collapse_composer(Some(window), cx),
                },
            );
        self.composer = Some((composer, subscription));
        cx.notify();
    }

    pub(super) fn collapse_composer(&mut self, window: Option<&Window>, cx: &mut Context<Self>) {
        let Some((composer, _)) = self.composer.take() else {
            return;
        };
        if let Some(window) = window
            && composer.read(cx).contains_focus(window, cx)
        {
            self.focus_pending = true;
        }
        cx.notify();
    }

    pub(super) fn on_drafts_event(
        &mut self,
        event: &DraftsEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            DraftsEvent::Submitted => {
                self.collapse_composer(Some(window), cx);
                window.push_notification(Notification::success(labels::REVIEW_SENT), cx);
                cx.emit(ReviewTabEvent::Reload);
            }
            DraftsEvent::SubmitFailed => {
                if self.composer.is_none() && self.dock_visible(cx) {
                    self.open_composer(false, window, cx);
                }
            }
            DraftsEvent::Failed(error) => {
                self.error = Some(error.clone());
                cx.notify();
            }
        }
    }
}
