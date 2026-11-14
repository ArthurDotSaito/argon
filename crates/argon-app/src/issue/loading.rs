use std::rc::Rc;

use argon_core::Issue;
use argon_github::IssueDetail;
use gpui_kit::Context;

use super::issue_view::{IssueView, Load};
use super::labels;
use super::timeline::Timeline;
use crate::pr::labels as pr_labels;
use crate::services::AppServices;

impl IssueView {
    pub(super) fn reload(&mut self, cx: &mut Context<Self>) {
        let Some(services) = cx
            .try_global::<AppServices>()
            .map(|global| global.0.clone())
        else {
            self.fail(labels::load_failed(pr_labels::SERVICES_UNAVAILABLE), cx);
            return;
        };
        self.epoch += 1;
        let epoch = self.epoch;
        self.refreshing = true;
        if self.detail.is_none() {
            self.load = Load::Loading;
        }
        let id = self.key.id.clone();
        let task = services.spawn_github(cx, self.key.host.clone(), move |client| async move {
            client.issue_detail(&id).await
        });
        self.load_task = Some(cx.spawn(async move |this, cx| {
            let result = task.await;
            _ = this.update(cx, |this, cx| match result {
                Ok(detail) => this.apply(epoch, detail, cx),
                Err(error) => {
                    tracing::warn!(%error, "failed to load issue detail");
                    if this.read_after_epoch.is_some_and(|after| epoch >= after) {
                        this.read_after_epoch = None;
                    }
                    this.action.reloaded(epoch);
                    if epoch > this.applied_epoch {
                        this.applied_epoch = epoch;
                        this.fail(labels::load_failed(&error.to_string()), cx);
                    }
                }
            });
        }));
        cx.notify();
    }

    fn apply(&mut self, epoch: u64, detail: IssueDetail, cx: &mut Context<Self>) {
        if epoch <= self.applied_epoch {
            return;
        }
        self.applied_epoch = epoch;
        self.refreshing = false;
        self.load = Load::Ready;
        self.seen_updated_at = self.seen_updated_at.max(Some(detail.issue.updated_at));
        self.timeline = Timeline::new(&detail, self.inbox.read(cx));
        self.composer.update(cx, |composer, cx| {
            composer.set_timeline(&detail.timeline, cx)
        });
        self.detail = Some(Rc::new(detail));
        self.settle_action(epoch, cx);
        let opened_now = std::mem::take(&mut self.read_on_load);
        let read_requested = self.read_after_epoch.is_some_and(|after| epoch >= after);
        if read_requested {
            self.read_after_epoch = None;
        }
        if opened_now || read_requested {
            self.mark_read(cx);
        }
        cx.notify();
    }

    pub(crate) fn reload_and_mark_read(&mut self, cx: &mut Context<Self>) {
        self.reload(cx);
        self.read_after_epoch = Some(self.epoch);
    }

    pub(super) fn mark_read(&mut self, cx: &mut Context<Self>) {
        let Some(updated_at) = self.detail.as_ref().map(|detail| detail.issue.updated_at) else {
            return;
        };
        let key = self.key.clone();
        self.inbox
            .update(cx, |inbox, cx| inbox.mark_seen(&key, updated_at, cx));
        if let Some(services) = cx.try_global::<AppServices>() {
            services.0.sync_now();
        }
    }

    fn fail(&mut self, message: String, cx: &mut Context<Self>) {
        self.refreshing = false;
        self.load = Load::Failed(message.into());
        cx.notify();
    }

    pub(super) fn on_inbox_changed(&mut self, cx: &mut Context<Self>) {
        let current = self.store_issue.as_ref().map(|issue| issue.updated_at);
        let latest = self
            .inbox
            .read(cx)
            .issue(&self.key)
            .map(|issue| issue.updated_at);
        let changed = latest != current;
        if changed {
            self.store_issue = self.inbox.read(cx).issue(&self.key).cloned().map(Rc::new);
        }
        if latest.is_some() && latest > self.seen_updated_at {
            self.seen_updated_at = latest;
            self.reload(cx);
        } else if changed && self.detail.is_none() {
            cx.notify();
        }
    }

    pub(super) fn issue(&self) -> Option<&Issue> {
        self.detail
            .as_ref()
            .map(|detail| &detail.issue)
            .or(self.store_issue.as_deref())
    }
}
