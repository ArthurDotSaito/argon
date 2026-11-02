use std::rc::Rc;

use argon_core::{PrState, PullRequest};
use argon_github::PrDetail;
use gpui_kit::Context;

use super::{Load, MergeState, PrView, app_services, viewer_login};
use crate::pr::labels;

impl PrView {
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        let Some(services) = app_services(cx) else {
            self.fail(labels::load_failed(labels::SERVICES_UNAVAILABLE), cx);
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
            client.pr_detail(&id).await
        });
        self.load_task = Some(cx.spawn(async move |this, cx| {
            let result = task.await;
            _ = this.update(cx, |this, cx| match result {
                Ok(detail) => this.apply(epoch, detail, cx),
                Err(error) => {
                    tracing::warn!(%error, "failed to load pr detail");
                    if epoch > this.applied_epoch {
                        this.applied_epoch = epoch;
                        this.fail(labels::load_failed(&error.to_string()), cx);
                    }
                }
            });
        }));
        cx.notify();
    }

    fn apply(&mut self, epoch: u64, detail: PrDetail, cx: &mut Context<Self>) {
        if epoch <= self.applied_epoch {
            return;
        }
        self.applied_epoch = epoch;
        self.refreshing = false;
        self.load = Load::Ready;
        self.seen_updated_at = self.seen_updated_at.max(Some(detail.pr.updated_at));
        if matches!(self.merge_state, MergeState::Merged) && detail.pr.state != PrState::Open {
            self.merge_state = MergeState::Idle;
        }
        let detail = Rc::new(detail);
        self.conversation
            .update(cx, |tab, cx| tab.set_detail(detail.clone(), cx));
        self.commits
            .update(cx, |tab, cx| tab.set_detail(detail.clone(), cx));
        self.actions
            .update(cx, |tab, cx| tab.set_detail(detail.clone(), cx));
        self.review
            .update(cx, |tab, cx| tab.set_detail(detail.clone(), cx));
        self.detail = Some(detail);
        cx.notify();
    }

    fn fail(&mut self, message: String, cx: &mut Context<Self>) {
        self.refreshing = false;
        self.load = Load::Failed(message.into());
        cx.notify();
    }

    pub(super) fn on_inbox_changed(&mut self, cx: &mut Context<Self>) {
        let viewer_login = viewer_login(&self.key.host, cx);
        let changed_login = viewer_login != self.viewer_login;
        if changed_login {
            let login = viewer_login.clone();
            self.review
                .update(cx, |tab, cx| tab.set_viewer_login(login, cx));
        }
        self.viewer_login = viewer_login;
        let current = self.store_pr.as_ref().map(|pr| pr.updated_at);
        let latest = self.inbox.read(cx).pr(&self.key).map(|pr| pr.updated_at);
        let changed_pr = latest != current;
        if changed_pr {
            self.store_pr = self.inbox.read(cx).pr(&self.key).cloned().map(Rc::new);
        }
        if latest.is_some() && latest > self.seen_updated_at {
            self.seen_updated_at = latest;
            self.reload(cx);
        } else if changed_login || (changed_pr && self.detail.is_none()) {
            cx.notify();
        }
    }

    pub(super) fn pr(&self) -> Option<&PullRequest> {
        self.detail
            .as_ref()
            .map(|detail| &detail.pr)
            .or(self.store_pr.as_deref())
    }
}
