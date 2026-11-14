use argon_core::IssueState;
use gpui_kit::component::WindowExt as _;
use gpui_kit::component::notification::Notification;
use gpui_kit::{Context, Window};

use super::issue_view::{IssueView, Load};
use super::labels;
use crate::agents::{ISSUE_CLOSED, PlanTarget, Target, open_start_dialog};

impl IssueView {
    pub(super) fn settle_pending_plan(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.pending_plan {
            return;
        }
        if self.issue().is_some() {
            self.pending_plan = false;
            cx.defer_in(window, |this, window, cx| this.open_agent_plan(window, cx));
        } else if matches!(self.load, Load::Failed(_)) {
            self.pending_plan = false;
            cx.defer_in(window, |_, window, cx| {
                window.push_notification(Notification::error(labels::PLAN_LOAD_FAILED), cx)
            });
        }
    }

    pub fn open_agent_plan(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(issue) = self.issue().cloned() else {
            self.pending_plan = true;
            cx.notify();
            return;
        };
        if issue.state != IssueState::Open {
            window.push_notification(Notification::warning(ISSUE_CLOSED), cx);
            return;
        }
        let target = PlanTarget {
            issue,
            detail: self.detail.as_deref().cloned(),
        };
        open_start_dialog(Target::Plan(target), window, cx);
    }
}
