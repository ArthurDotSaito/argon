use argon_core::IssueState;
use argon_store::PlanDraft;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{
    ActiveTheme as _, Icon, Sizable as _, StyledExt as _, WindowExt as _, h_flex,
};
use gpui_kit::{App, Context, Div, ParentElement as _, Styled as _, Window, div};

use super::issue_view::{IssueView, Load};
use super::labels;
use crate::agents::{ISSUE_CLOSED, PlanTarget, Target, open_start_dialog};
use crate::services::{AppServices, flatten};
use crate::workspace::{SessionPlacement, Workspace};

pub(super) fn pending_plan(drafts: &[PlanDraft]) -> Option<&PlanDraft> {
    drafts
        .iter()
        .filter(|draft| draft.published_at.is_none())
        .max_by_key(|draft| draft.id)
}

fn open_session_beside(session_id: String, window: &mut Window, cx: &mut App) {
    if let Some(workspace) = Workspace::global(cx) {
        _ = workspace.update(cx, |workspace, cx| {
            workspace.open_session(session_id, SessionPlacement::BesideSubject, window, cx)
        });
    }
}

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

    pub(super) fn reload_plans(&mut self, cx: &mut Context<Self>) {
        let Some(services) = cx
            .try_global::<AppServices>()
            .map(|global| global.0.clone())
        else {
            return;
        };
        let (host, id) = (self.key.host.clone(), self.key.id.0.clone());
        self.plans_task = Some(cx.spawn(async move |this, cx| {
            let stored = flatten(
                services
                    .store()
                    .call(move |store| store.plans_for_issue(&host, &id))
                    .await,
            );
            _ = this.update(cx, |this, cx| match stored {
                Ok(drafts) => {
                    this.plan_drafts = drafts;
                    cx.notify();
                }
                Err(error) => tracing::warn!(%error, "failed to load plan drafts for the issue"),
            });
        }));
    }

    pub(super) fn plan_banner(&self, cx: &App) -> Option<Div> {
        let session = pending_plan(&self.plan_drafts)?.session_id.clone();
        let theme = cx.theme();
        Some(
            h_flex()
                .flex_none()
                .mx_4()
                .mb_2()
                .px_3()
                .py_1p5()
                .gap_2()
                .items_center()
                .rounded(theme.radius)
                .border_1()
                .border_color(theme.primary.opacity(0.5))
                .bg(theme.primary.opacity(0.08))
                .text_sm()
                .child(
                    Icon::new(IconName::Sparkles)
                        .small()
                        .text_color(theme.primary),
                )
                .child(div().font_semibold().child(labels::PLAN_DRAFT_READY))
                .child(div().text_color(theme.muted_foreground).child("·"))
                .child(
                    Button::new("issue-plan-review")
                        .label(labels::REVIEW_AND_PUBLISH)
                        .small()
                        .link()
                        .on_click(move |_, window, cx| {
                            open_session_beside(session.clone(), window, cx)
                        }),
                ),
        )
    }
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};

    use super::*;

    fn draft(id: i64, session: &str, published: bool) -> PlanDraft {
        let at = Utc.with_ymd_and_hms(2026, 11, 13, 10, 0, 0).unwrap();
        PlanDraft {
            id,
            host: "github.com".into(),
            issue_id: "I_7".into(),
            session_id: session.into(),
            body: "## Passos".into(),
            base_sha: "abc123".into(),
            agent_body: None,
            edited: false,
            created_at: at,
            updated_at: at,
            published_at: published.then_some(at),
            comment_id: None,
            comment_url: None,
        }
    }

    #[test]
    fn banner_points_to_the_newest_unpublished_draft() {
        let drafts = [
            draft(5, "s2", true),
            draft(4, "s2", false),
            draft(3, "s1", false),
        ];
        assert_eq!(pending_plan(&drafts).map(|draft| draft.id), Some(4));
    }

    #[test]
    fn no_banner_once_every_draft_is_published() {
        let drafts = [draft(2, "s1", true), draft(1, "s1", true)];
        assert_eq!(pending_plan(&drafts), None);
        assert_eq!(pending_plan(&[]), None);
    }
}
