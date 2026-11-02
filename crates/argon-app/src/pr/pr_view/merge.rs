use argon_core::is_ready;
use argon_github::{GithubError, MergeMethod};
use gpui_kit::{Context, Window};

use super::{MergeState, PrView, app_services, request_sync};
use crate::pr::labels;
use crate::pr::merge_dialog::{self, MergeTarget};
use crate::services::ServiceError;

const HEAD_MISMATCH_MARKERS: [&str; 3] = ["head branch was modified", "expected head", "head sha"];

fn is_head_mismatch(error: &ServiceError) -> bool {
    let ServiceError::Github(GithubError::GraphQl(messages)) = error else {
        return false;
    };
    messages.iter().any(|message| {
        let message = message.to_lowercase();
        HEAD_MISMATCH_MARKERS
            .iter()
            .any(|marker| message.contains(marker))
    })
}

impl PrView {
    pub(super) fn can_merge(&self) -> bool {
        let Some(pr) = self.pr() else {
            return false;
        };
        self.detail.is_some()
            && !self.merge_state.is_busy()
            && is_ready(pr)
            && self
                .viewer_login
                .as_deref()
                .is_some_and(|login| login.eq_ignore_ascii_case(&pr.author))
    }

    pub fn merge_method(&self) -> MergeMethod {
        self.merge_method
    }

    pub fn set_merge_method(&mut self, method: MergeMethod, cx: &mut Context<Self>) {
        self.merge_method = method;
        cx.notify();
    }

    pub fn open_merge_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.can_merge() {
            return;
        }
        let Some(pr) = self.pr() else {
            return;
        };
        let target = MergeTarget {
            number: pr.number,
            head: pr.head.clone(),
            base: pr.base.clone(),
        };
        merge_dialog::open(cx.weak_entity(), target, window, cx);
    }

    pub fn merge(&mut self, cx: &mut Context<Self>) {
        if !self.can_merge() {
            return;
        }
        let Some(head_oid) = self.detail.as_ref().map(|detail| detail.head_oid.clone()) else {
            return;
        };
        let Some(services) = app_services(cx) else {
            self.merge_state =
                MergeState::Failed(labels::merge_failed(labels::SERVICES_UNAVAILABLE).into());
            cx.notify();
            return;
        };
        self.merge_state = MergeState::Merging;
        let id = self.key.id.clone();
        let method = self.merge_method;
        let task = services.spawn_github(cx, self.key.host.clone(), move |client| async move {
            client.merge(&id, method, &head_oid).await
        });
        self.merge_task = Some(cx.spawn(async move |this, cx| {
            let result = task.await;
            _ = this.update(cx, |this, cx| {
                match result {
                    Ok(()) => {
                        tracing::info!("pull request merged");
                        this.merge_state = MergeState::Merged;
                        this.reload(cx);
                        request_sync(cx);
                    }
                    Err(error) if is_head_mismatch(&error) => {
                        tracing::warn!(%error, "merge refused: head moved since load");
                        this.merge_state = MergeState::Failed(labels::MERGE_HEAD_MOVED.into());
                        this.reload(cx);
                    }
                    Err(error) => {
                        tracing::warn!(%error, "failed to merge pull request");
                        this.merge_state =
                            MergeState::Failed(labels::merge_failed(&error.to_string()).into());
                    }
                }
                cx.notify();
            });
        }));
        cx.notify();
    }
}
