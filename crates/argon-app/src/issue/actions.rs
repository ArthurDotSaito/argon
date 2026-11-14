use argon_core::{Issue, IssueState, IssueStateReason};
use gpui_kit::{App, Context, SharedString};

use super::issue_view::IssueView;
use super::labels;
use crate::pr::labels as pr_labels;
use crate::services::AppServices;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum IssueAction {
    Assign,
    Close(IssueStateReason),
}

impl IssueAction {
    fn eligible(self, bar: ActionBar) -> bool {
        match self {
            Self::Assign => bar.assign,
            Self::Close(_) => bar.close,
        }
    }
}

pub(super) enum ActionState {
    Idle,
    Running(IssueAction),
    Reloading {
        action: IssueAction,
        after_epoch: u64,
    },
    Failed {
        action: IssueAction,
        message: SharedString,
    },
}

impl ActionState {
    pub(super) fn busy(&self) -> Option<IssueAction> {
        match self {
            Self::Running(action) | Self::Reloading { action, .. } => Some(*action),
            Self::Idle | Self::Failed { .. } => None,
        }
    }

    pub(super) fn reloaded(&mut self, epoch: u64) {
        if matches!(self, Self::Reloading { after_epoch, .. } if epoch >= *after_epoch) {
            *self = Self::Idle;
        }
    }

    fn clear_failure(&mut self) {
        if matches!(self, Self::Failed { .. }) {
            *self = Self::Idle;
        }
    }

    fn drop_stale_failure(&mut self, bar: ActionBar) {
        if matches!(self, Self::Failed { action, .. } if !action.eligible(bar)) {
            *self = Self::Idle;
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ActionBar {
    pub assign: bool,
    pub close: bool,
    pub plan: bool,
    pub running: Option<IssueAction>,
}

impl ActionBar {
    pub(super) fn of(
        issue: &Issue,
        viewer_login: Option<&str>,
        running: Option<IssueAction>,
    ) -> Self {
        let open = issue.state == IssueState::Open;
        let unassigned_viewer = viewer_login.is_some_and(|login| {
            !issue
                .assignees
                .iter()
                .any(|assignee| assignee.eq_ignore_ascii_case(login))
        });
        Self {
            assign: open && unassigned_viewer,
            close: open,
            plan: open,
            running,
        }
    }
}

impl IssueView {
    fn current_action_bar(&self, cx: &App) -> Option<ActionBar> {
        let issue = self.issue()?;
        let login = self.viewer_login(cx);
        Some(ActionBar::of(issue, login.as_deref(), None))
    }

    pub(super) fn settle_action(&mut self, epoch: u64, cx: &App) {
        self.action.reloaded(epoch);
        if let Some(bar) = self.current_action_bar(cx) {
            self.action.drop_stale_failure(bar);
        }
    }

    pub(super) fn run_action(&mut self, action: IssueAction, cx: &mut Context<Self>) {
        if self.action.busy().is_some() {
            return;
        }
        let eligible = self
            .current_action_bar(cx)
            .is_some_and(|bar| action.eligible(bar));
        if !eligible {
            self.action.clear_failure();
            cx.notify();
            return;
        }
        let Some(services) = cx
            .try_global::<AppServices>()
            .map(|global| global.0.clone())
        else {
            self.action = ActionState::Failed {
                action,
                message: labels::action_failed(action, pr_labels::SERVICES_UNAVAILABLE).into(),
            };
            cx.notify();
            return;
        };
        self.action = ActionState::Running(action);
        let id = self.key.id.clone();
        let cached_viewer = self.viewer_id.clone();
        let task = services.spawn_github(cx, self.key.host.clone(), move |client| async move {
            match action {
                IssueAction::Assign => {
                    let viewer = match cached_viewer {
                        Some(viewer) => viewer,
                        None => client.viewer_id().await?,
                    };
                    client
                        .add_assignees(&id, std::slice::from_ref(&viewer))
                        .await?;
                    Ok(Some(viewer))
                }
                IssueAction::Close(reason) => {
                    client.close_issue(&id, reason).await?;
                    Ok(None)
                }
            }
        });
        self.action_task = Some(cx.spawn(async move |this, cx| {
            let result = task.await;
            _ = this.update(cx, |this, cx| {
                match result {
                    Ok(viewer) => {
                        tracing::info!(?action, "issue action applied");
                        if viewer.is_some() {
                            this.viewer_id = viewer;
                        }
                        this.reload_and_mark_read(cx);
                        this.action = ActionState::Reloading {
                            action,
                            after_epoch: this.epoch,
                        };
                    }
                    Err(error) => {
                        tracing::warn!(%error, ?action, "issue action failed");
                        this.action = ActionState::Failed {
                            action,
                            message: labels::action_failed(action, &error.to_string()).into(),
                        };
                    }
                }
                cx.notify();
            });
        }));
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use argon_core::fixtures::issue;

    fn closed(mut issue: Issue) -> Issue {
        issue.state = IssueState::Closed;
        issue
    }

    #[test]
    fn open_issue_offers_assign_close_and_plan() {
        let bar = ActionBar::of(&issue(), Some("ana"), None);
        assert!(bar.assign);
        assert!(bar.close);
        assert!(bar.plan);
    }

    #[test]
    fn assigned_viewer_does_not_get_assign_whatever_the_case() {
        let mut issue = issue();
        issue.assignees = vec!["Bruno".into(), "Ana".into()];
        let bar = ActionBar::of(&issue, Some("ana"), None);
        assert!(!bar.assign);
        assert!(bar.close);
    }

    #[test]
    fn other_assignees_do_not_hide_assign() {
        let mut issue = issue();
        issue.assignees = vec!["bruno".into()];
        assert!(ActionBar::of(&issue, Some("ana"), None).assign);
    }

    #[test]
    fn closed_issue_offers_nothing() {
        let bar = ActionBar::of(&closed(issue()), Some("ana"), None);
        assert!(!bar.assign);
        assert!(!bar.close);
        assert!(!bar.plan);
    }

    #[test]
    fn unknown_viewer_hides_assign_only() {
        let bar = ActionBar::of(&issue(), None, None);
        assert!(!bar.assign);
        assert!(bar.close);
    }

    #[test]
    fn busy_covers_running_and_reloading_only() {
        let action = IssueAction::Assign;
        let failed = ActionState::Failed {
            action,
            message: "x".into(),
        };
        assert_eq!(ActionState::Running(action).busy(), Some(action));
        assert_eq!(
            ActionState::Reloading {
                action,
                after_epoch: 3
            }
            .busy(),
            Some(action)
        );
        assert_eq!(ActionState::Idle.busy(), None);
        assert_eq!(failed.busy(), None);
    }

    #[test]
    fn eligibility_follows_the_current_bar() {
        let reason = IssueStateReason::Completed;
        let open = ActionBar::of(&issue(), Some("ana"), None);
        assert!(IssueAction::Assign.eligible(open));
        assert!(IssueAction::Close(reason).eligible(open));
        let done = ActionBar::of(&closed(issue()), Some("ana"), None);
        assert!(!IssueAction::Assign.eligible(done));
        assert!(!IssueAction::Close(reason).eligible(done));
        let mut assigned = issue();
        assigned.assignees = vec!["ana".into()];
        let bar = ActionBar::of(&assigned, Some("ana"), None);
        assert!(!IssueAction::Assign.eligible(bar));
        assert!(IssueAction::Close(reason).eligible(bar));
    }

    #[test]
    fn reloading_settles_only_once_the_post_action_epoch_is_reached() {
        let mut state = ActionState::Reloading {
            action: IssueAction::Assign,
            after_epoch: 4,
        };
        state.reloaded(3);
        assert!(state.busy().is_some());
        state.reloaded(4);
        assert!(matches!(state, ActionState::Idle));
    }

    #[test]
    fn reloaded_leaves_other_states_alone() {
        let mut running = ActionState::Running(IssueAction::Assign);
        running.reloaded(9);
        assert!(matches!(running, ActionState::Running(_)));
    }

    #[test]
    fn failure_is_dropped_when_the_action_is_no_longer_eligible() {
        let mut state = ActionState::Failed {
            action: IssueAction::Close(IssueStateReason::Completed),
            message: "x".into(),
        };
        state.drop_stale_failure(ActionBar::of(&issue(), Some("ana"), None));
        assert!(matches!(state, ActionState::Failed { .. }));
        state.drop_stale_failure(ActionBar::of(&closed(issue()), Some("ana"), None));
        assert!(matches!(state, ActionState::Idle));
    }

    #[test]
    fn clear_failure_keeps_busy_states() {
        let mut busy = ActionState::Running(IssueAction::Assign);
        busy.clear_failure();
        assert!(busy.busy().is_some());
        let mut failed = ActionState::Failed {
            action: IssueAction::Assign,
            message: "x".into(),
        };
        failed.clear_failure();
        assert!(matches!(failed, ActionState::Idle));
    }
}
