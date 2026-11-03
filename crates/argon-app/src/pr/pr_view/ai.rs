use argon_core::PrState;
use gpui_kit::{Context, Window};

use super::{MergeState, PrView};
use crate::agents::{ReviewTarget, open_start_dialog};

impl PrView {
    pub fn can_review_with_ai(&self) -> bool {
        !matches!(self.merge_state, MergeState::Merged)
            && self.pr().is_some_and(|pr| pr.state == PrState::Open)
    }

    pub fn open_ai_review(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.can_review_with_ai() {
            return;
        }
        let Some(pr) = self.pr().cloned() else {
            return;
        };
        let target = ReviewTarget {
            pr,
            detail: self.detail.as_deref().cloned(),
        };
        open_start_dialog(target, window, cx);
    }
}
