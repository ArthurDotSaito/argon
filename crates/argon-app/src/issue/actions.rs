use argon_core::{Issue, IssueState, IssueStateReason};
use gpui_kit::{Context, SharedString};

use super::issue_view::IssueView;
use super::labels;
use crate::pr::labels as pr_labels;
use crate::services::AppServices;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum IssueAction {
    Assign,
    Close(IssueStateReason),
}

pub(super) enum ActionState {
    Idle,
    Running(IssueAction),
    Failed {
        action: IssueAction,
        message: SharedString,
    },
}

impl ActionState {
    pub(super) fn running(&self) -> Option<IssueAction> {
        match self {
            Self::Running(action) => Some(*action),
            Self::Idle | Self::Failed { .. } => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ActionBar {
    pub assign: bool,
    pub close: bool,
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
            running,
        }
    }
}

impl IssueView {
    pub(super) fn run_action(&mut self, action: IssueAction, cx: &mut Context<Self>) {
        if self.action.running().is_some() {
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
                        this.action = ActionState::Idle;
                        this.reload_and_mark_read(cx);
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
    fn open_issue_offers_assign_and_close() {
        let bar = ActionBar::of(&issue(), Some("ana"), None);
        assert!(bar.assign);
        assert!(bar.close);
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
    }

    #[test]
    fn unknown_viewer_hides_assign_only() {
        let bar = ActionBar::of(&issue(), None, None);
        assert!(!bar.assign);
        assert!(bar.close);
    }

    #[test]
    fn running_action_is_carried_over() {
        let running = Some(IssueAction::Assign);
        assert_eq!(
            ActionBar::of(&issue(), Some("ana"), running).running,
            running
        );
        assert_eq!(ActionState::Running(IssueAction::Assign).running(), running);
        assert_eq!(ActionState::Idle.running(), None);
    }
}
