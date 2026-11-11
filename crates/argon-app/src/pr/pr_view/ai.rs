use argon_core::PrState;
use gpui_kit::component::WindowExt as _;
use gpui_kit::component::notification::Notification;
use gpui_kit::{Context, Window};

use super::{Load, MergeState, PrView};
use crate::agents::{PR_CLOSED, ReviewTarget, open_start_dialog};

const LOAD_FAILED: &str = "Não consegui carregar o PR para revisar com IA";

impl PrView {
    pub(super) fn settle_pending_ai_review(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.pending_ai_review {
            return;
        }
        if self.pr().is_some() {
            self.pending_ai_review = false;
            cx.defer_in(window, |this, window, cx| this.open_ai_review(window, cx));
        } else if matches!(self.load, Load::Failed(_)) {
            self.pending_ai_review = false;
            cx.defer_in(window, |_, window, cx| {
                window.push_notification(Notification::error(LOAD_FAILED), cx)
            });
        }
    }

    pub fn can_review_with_ai(&self) -> bool {
        !matches!(self.merge_state, MergeState::Merged)
            && self.pr().is_some_and(|pr| pr.state == PrState::Open)
    }

    pub fn open_ai_review(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(pr) = self.pr().cloned() else {
            self.pending_ai_review = true;
            cx.notify();
            return;
        };
        if !self.can_review_with_ai() {
            window.push_notification(Notification::warning(PR_CLOSED), cx);
            return;
        }
        let target = ReviewTarget {
            pr,
            detail: self.detail.as_deref().cloned(),
        };
        open_start_dialog(target, window, cx);
    }
}
