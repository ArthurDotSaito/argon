use argon_core::{Issue, IssueState, IssueStateReason, LinkedPr};
use chrono::{DateTime, Utc};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Icon, Sizable as _, StyledExt as _, h_flex, v_flex,
};
use gpui_kit::{
    App, ClickEvent, Context, Div, Hsla, IntoElement, ParentElement as _, Styled as _, WeakEntity,
    div, px,
};

use super::actions::{ActionBar, IssueAction};
use super::issue_view::IssueView;
use super::labels;
use crate::pr::{self, labels as pr_labels};
use crate::ui;

const META_AVATAR: f32 = 18.;
const ASSIGNEE_AVATAR: f32 = 16.;
const CHIP_HEIGHT: f32 = 20.;
const TAG_HEIGHT: f32 = 22.;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IssueStatus {
    Open,
    Closed,
    NotPlanned,
}

impl IssueStatus {
    pub fn of(state: IssueState, reason: Option<IssueStateReason>) -> Self {
        match (state, reason) {
            (IssueState::Open, _) => Self::Open,
            (IssueState::Closed, Some(IssueStateReason::NotPlanned)) => Self::NotPlanned,
            (IssueState::Closed, _) => Self::Closed,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Open => "Open",
            Self::Closed => "Closed",
            Self::NotPlanned => "Not planned",
        }
    }
}

pub fn status_style(status: IssueStatus, cx: &App) -> (IconName, Hsla) {
    let theme = cx.theme();
    match status {
        IssueStatus::Open => (IconName::CircleDot, theme.success),
        IssueStatus::Closed => (IconName::CircleCheck, theme.magenta),
        IssueStatus::NotPlanned => (IconName::CircleSlash, theme.muted_foreground),
    }
}

pub fn header(
    issue: &Issue,
    refreshing: bool,
    now: DateTime<Utc>,
    bar: ActionBar,
    cx: &mut Context<IssueView>,
) -> Div {
    let theme = cx.theme();
    let title = h_flex()
        .gap_2()
        .items_baseline()
        .child(
            div()
                .text_xl()
                .font_semibold()
                .text_color(theme.foreground)
                .child(issue.title.clone()),
        )
        .child(
            div()
                .text_lg()
                .text_color(theme.muted_foreground)
                .child(format!("#{}", issue.number)),
        );
    v_flex()
        .w_full()
        .flex_none()
        .px_4()
        .pt_3()
        .pb_3()
        .gap_2()
        .child(
            h_flex()
                .w_full()
                .gap_2()
                .items_center()
                .child(pr::breadcrumb(&issue.repo, issue.number, cx))
                .child(div().flex_1())
                .child(actions(refreshing, bar, cx)),
        )
        .child(title)
        .child(meta(issue, now, cx))
        .child(stats(issue, cx))
}

fn actions(refreshing: bool, bar: ActionBar, cx: &mut Context<IssueView>) -> Div {
    let open = Button::new("issue-open-github")
        .icon(Icon::new(IconName::ExternalLink))
        .label(pr_labels::OPEN_ON_GITHUB)
        .small()
        .ghost()
        .on_click(cx.listener(|this, _: &ClickEvent, _, _| this.open_on_github()));
    let refresh = Button::new("issue-refresh")
        .icon(Icon::new(IconName::RefreshCw))
        .small()
        .ghost()
        .loading(refreshing)
        .tooltip(pr_labels::REFRESH)
        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.reload(cx)));
    let busy = bar.running.is_some();
    let assign = bar.assign.then(|| {
        Button::new("issue-assign")
            .icon(Icon::new(IconName::UserPlus))
            .label(labels::ASSIGN_TO_ME)
            .small()
            .outline()
            .loading(bar.running == Some(IssueAction::Assign))
            .disabled(busy)
            .on_click(
                cx.listener(|this, _: &ClickEvent, _, cx| this.run_action(IssueAction::Assign, cx)),
            )
    });
    let close = bar.close.then(|| close_button(bar.running, cx));
    h_flex()
        .gap_1()
        .items_center()
        .child(open)
        .child(refresh)
        .children(assign)
        .children(close)
}

fn close_button(running: Option<IssueAction>, cx: &mut Context<IssueView>) -> impl IntoElement {
    let view = cx.weak_entity();
    Button::new("issue-close")
        .label(labels::CLOSE)
        .small()
        .outline()
        .dropdown_caret(true)
        .loading(matches!(running, Some(IssueAction::Close(_))))
        .disabled(running.is_some())
        .dropdown_menu(move |menu, _, _| {
            menu.item(close_item(
                labels::CLOSE_COMPLETED,
                IconName::CircleCheck,
                IssueStateReason::Completed,
                &view,
            ))
            .item(close_item(
                labels::CLOSE_NOT_PLANNED,
                IconName::CircleSlash,
                IssueStateReason::NotPlanned,
                &view,
            ))
        })
}

fn close_item(
    label: &'static str,
    icon: IconName,
    reason: IssueStateReason,
    view: &WeakEntity<IssueView>,
) -> PopupMenuItem {
    let view = view.clone();
    PopupMenuItem::new(label)
        .icon(Icon::new(icon))
        .on_click(move |_, _, cx| {
            _ = view.update(cx, |this, cx| {
                this.run_action(IssueAction::Close(reason), cx)
            });
        })
}

fn meta(issue: &Issue, now: DateTime<Utc>, cx: &App) -> Div {
    let theme = cx.theme();
    let status = IssueStatus::of(issue.state, issue.state_reason);
    let (icon, color) = status_style(status, cx);
    h_flex()
        .w_full()
        .flex_wrap()
        .gap_2()
        .items_center()
        .text_sm()
        .text_color(theme.muted_foreground)
        .child(pr::state_pill(icon, color, status.label()))
        .children(issue.issue_type.as_deref().map(|name| type_tag(name, cx)))
        .child(ui::avatar(&issue.author, META_AVATAR))
        .child(
            div()
                .font_semibold()
                .text_color(theme.foreground)
                .child(issue.author.clone()),
        )
        .child(labels::opened(&ui::ago(now, issue.created_at)))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum IssueTypeKind {
    Bug,
    Feature,
    Other,
}

impl IssueTypeKind {
    fn of(name: &str) -> Self {
        if name.eq_ignore_ascii_case("bug") {
            Self::Bug
        } else if name.eq_ignore_ascii_case("feature") {
            Self::Feature
        } else {
            Self::Other
        }
    }
}

pub fn type_style(name: &str, cx: &App) -> (IconName, Hsla) {
    let theme = cx.theme();
    match IssueTypeKind::of(name) {
        IssueTypeKind::Bug => (IconName::Bug, theme.danger),
        IssueTypeKind::Feature => (IconName::Lightbulb, theme.blue),
        IssueTypeKind::Other => (IconName::SquareCheck, theme.muted_foreground),
    }
}

fn type_tag(name: &str, cx: &App) -> Div {
    let theme = cx.theme();
    let (icon, color) = type_style(name, cx);
    h_flex()
        .gap_1()
        .px_1p5()
        .h(px(TAG_HEIGHT))
        .items_center()
        .rounded(theme.radius)
        .bg(theme.muted)
        .border_1()
        .border_color(theme.border)
        .text_xs()
        .font_medium()
        .text_color(color)
        .child(Icon::new(icon).xsmall())
        .child(name.to_string())
}

fn stats(issue: &Issue, cx: &App) -> Div {
    let theme = cx.theme();
    let sub_issues = issue
        .sub_issues
        .map(|sub| stat(IconName::ListTree, labels::sub_issues(sub)));
    let milestone = issue
        .milestone
        .clone()
        .map(|milestone| stat(IconName::Milestone, milestone));
    let label_row = (!issue.labels.is_empty()).then(|| label_list(issue, cx));
    h_flex()
        .w_full()
        .flex_wrap()
        .gap_x_4()
        .gap_y_1p5()
        .items_center()
        .text_sm()
        .text_color(theme.muted_foreground)
        .child(assignees(issue, cx))
        .child(linked_prs(issue, cx))
        .children(sub_issues)
        .children(milestone)
        .children(label_row)
}

fn stat(icon: IconName, text: String) -> Div {
    h_flex()
        .gap_1()
        .items_center()
        .child(Icon::new(icon).xsmall())
        .child(text)
}

fn assignees(issue: &Issue, cx: &App) -> Div {
    let theme = cx.theme();
    let row = h_flex()
        .gap_2()
        .items_center()
        .child(Icon::new(IconName::Users).xsmall());
    if issue.assignees.is_empty() {
        return row.child(labels::NO_ASSIGNEES);
    }
    row.children(issue.assignees.iter().map(|login| {
        h_flex()
            .gap_1()
            .items_center()
            .child(ui::avatar(login, ASSIGNEE_AVATAR))
            .child(div().text_color(theme.foreground).child(login.clone()))
    }))
}

fn linked_prs(issue: &Issue, cx: &App) -> Div {
    let row = h_flex()
        .gap_1p5()
        .items_center()
        .child(Icon::new(IconName::GitPullRequest).xsmall());
    if issue.linked_prs.is_empty() {
        return row.child(labels::NO_LINKED_PR);
    }
    row.children(
        issue
            .linked_prs
            .iter()
            .map(|linked| linked_pr_chip(&issue.repo, linked, cx)),
    )
}

fn linked_pr_chip(issue_repo: &str, linked: &LinkedPr, cx: &App) -> Div {
    let style = ui::state_style_for(linked.state, linked.is_draft, cx);
    let text = labels::reference(issue_repo, &linked.repo, linked.number);
    reference_chip(style.icon, style.color, text, cx)
}

pub fn reference_chip(icon: IconName, color: Hsla, text: String, cx: &App) -> Div {
    let theme = cx.theme();
    h_flex()
        .flex_none()
        .gap_1()
        .px_1p5()
        .h(px(CHIP_HEIGHT))
        .items_center()
        .rounded(theme.radius)
        .border_1()
        .border_color(theme.border)
        .child(Icon::new(icon).xsmall().text_color(color))
        .child(
            div()
                .font_family(theme.mono_font_family.clone())
                .text_xs()
                .text_color(theme.foreground)
                .child(text),
        )
}

fn label_list(issue: &Issue, cx: &App) -> Div {
    h_flex().gap_1().flex_wrap().items_center().children(
        issue
            .labels
            .iter()
            .map(|label| pr::label_chip(&label.name, Some(label.color.as_str()), cx)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_issue_is_open_whatever_the_reason() {
        assert_eq!(IssueStatus::of(IssueState::Open, None), IssueStatus::Open);
        assert_eq!(
            IssueStatus::of(IssueState::Open, Some(IssueStateReason::Reopened)),
            IssueStatus::Open
        );
    }

    #[test]
    fn closed_issue_is_not_planned_only_with_that_reason() {
        assert_eq!(
            IssueStatus::of(IssueState::Closed, Some(IssueStateReason::NotPlanned)),
            IssueStatus::NotPlanned
        );
        for reason in [
            None,
            Some(IssueStateReason::Completed),
            Some(IssueStateReason::Duplicate),
        ] {
            assert_eq!(
                IssueStatus::of(IssueState::Closed, reason),
                IssueStatus::Closed
            );
        }
    }

    #[test]
    fn type_kind_ignores_case_and_defaults_to_other() {
        assert_eq!(IssueTypeKind::of("Bug"), IssueTypeKind::Bug);
        assert_eq!(IssueTypeKind::of("FEATURE"), IssueTypeKind::Feature);
        assert_eq!(IssueTypeKind::of("Task"), IssueTypeKind::Other);
        assert_eq!(IssueTypeKind::of(""), IssueTypeKind::Other);
    }

    #[test]
    fn status_labels_match_github() {
        assert_eq!(IssueStatus::Open.label(), "Open");
        assert_eq!(IssueStatus::Closed.label(), "Closed");
        assert_eq!(IssueStatus::NotPlanned.label(), "Not planned");
    }
}
