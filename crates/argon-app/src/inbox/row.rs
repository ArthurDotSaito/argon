use argon_core::{Issue, IssueLabel, PullRequest};
use chrono::{DateTime, Utc};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::Button;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Icon, Sizable as _, StyledExt as _, h_flex,
};
use gpui_kit::{
    App, ClickEvent, Div, ElementId, Hsla, ParentElement as _, SharedString, Stateful, Styled as _,
    Window, div, prelude::FluentBuilder as _, px,
};

use super::filter::KindFilter;
use super::items::{InboxItem, Subject};
use super::labels;
use crate::issue::{IssueStatus, status_style, type_style};
use crate::pr::label_color;
use crate::theme::density;
use crate::ui::{self, avatar, state_icon, state_style_for};

const AVATAR_SIZE: f32 = 18.;
const REVIEWER_OVERLAP: f32 = -5.;
const MAX_REVIEWERS: usize = 3;
const UNREAD_DOT: f32 = 6.;
const REASON_DOT: f32 = 7.;
const REASON_GAP: f32 = 7.;
const REASON_WIDTH: f32 = 284.;
const REASON_MIN_WIDTH: f32 = 140.;
const CI_WIDTH: f32 = 100.;
const REVIEWERS_WIDTH: f32 = 56.;
const COMMENTS_WIDTH: f32 = 44.;
const DIFF_WIDTH: f32 = 92.;
const AGE_WIDTH: f32 = 66.;
const LABEL_DOT: f32 = 8.;
const TAG_HEIGHT: f32 = 20.;

pub fn pr_row_height(cx: &App) -> f32 {
    density::scaled(ui::columns::LIST_ROW_HEIGHT, cx)
}

pub fn header(title: SharedString, count: usize, cx: &App) -> Div {
    ui::columns::group_row(cx).child(title).child(
        div()
            .font_normal()
            .text_color(cx.theme().muted_foreground)
            .child(count.to_string()),
    )
}

pub fn column_header(kind: KindFilter, cx: &App) -> Div {
    let titles = labels::column_titles(kind);
    ui::columns::column_header(cx)
        .child(div().flex_1().min_w_0().child(titles.subject))
        .child(reason_cell().child(labels::COLUMN_REASON))
        .child(div().w(px(CI_WIDTH)).flex_none().child(titles.signal))
        .child(
            div()
                .w(px(REVIEWERS_WIDTH))
                .flex_none()
                .child(titles.people),
        )
        .child(right_cell(COMMENTS_WIDTH).child(Icon::new(IconName::MessageSquare).xsmall()))
        .child(right_cell(DIFF_WIDTH).child(titles.size))
        .child(right_cell(AGE_WIDTH).child(labels::COLUMN_UPDATED))
}

fn reason_cell() -> Div {
    div()
        .w(px(REASON_WIDTH))
        .flex_shrink(1.)
        .min_w(px(REASON_MIN_WIDTH))
}

fn right_cell(width: f32) -> Div {
    h_flex().w(px(width)).flex_none().justify_end()
}

pub fn load_more(
    loading: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    cx: &App,
) -> Div {
    h_flex()
        .w_full()
        .h(px(pr_row_height(cx)))
        .items_center()
        .justify_center()
        .child(
            Button::new("inbox-load-more")
                .label(labels::LOAD_MORE)
                .small()
                .outline()
                .loading(loading)
                .disabled(loading)
                .on_click(on_click),
        )
}

pub fn item_row(
    position: usize,
    item: &InboxItem,
    selected: bool,
    now: DateTime<Utc>,
    cx: &App,
) -> Stateful<Div> {
    let row = ui::columns::list_row(("inbox-row", position), selected, cx);
    match &item.subject {
        Subject::Pr(pr) => pr_cells(row, pr, Some(item), now, cx),
        Subject::Issue(issue) => issue_cells(row, item, issue, now, cx),
    }
}

pub fn pr_row(
    id: impl Into<ElementId>,
    pr: &PullRequest,
    item: Option<&InboxItem>,
    now: DateTime<Utc>,
    cx: &App,
) -> Stateful<Div> {
    pr_cells(ui::columns::list_row(id, false, cx), pr, item, now, cx)
}

fn pr_cells(
    row: Stateful<Div>,
    pr: &PullRequest,
    item: Option<&InboxItem>,
    now: DateTime<Utc>,
    cx: &App,
) -> Stateful<Div> {
    let unread = item.is_some_and(|item| item.unread);
    let reason = match item {
        Some(item) => reason(item, now, pr.updated_at, cx),
        None => reason_cell().child(mono(pr.head.clone(), cx).min_w_0()),
    };
    row.child(
        subject_cell()
            .child(unread_dot(unread, cx))
            .child(state_icon(pr, cx))
            .child(avatar(&pr.author, AVATAR_SIZE))
            .child(title(&pr.title, unread, cx))
            .child(mono(labels::short_repo_ref(&pr.repo, pr.number), cx)),
    )
    .child(reason)
    .child(
        div()
            .w(px(CI_WIDTH))
            .flex_none()
            .child(ui::checks_label(pr.checks, cx)),
    )
    .child(reviewers(pr, cx))
    .child(comments(pr.comments, cx))
    .child(right_cell(DIFF_WIDTH).child(ui::diff_stat(pr.additions, pr.deletions, cx)))
    .child(age(now, pr.updated_at, cx))
}

fn issue_cells(
    row: Stateful<Div>,
    item: &InboxItem,
    issue: &Issue,
    now: DateTime<Utc>,
    cx: &App,
) -> Stateful<Div> {
    let (state_icon, state_color) =
        status_style(IssueStatus::of(issue.state, issue.state_reason), cx);
    row.child(
        subject_cell()
            .child(unread_dot(item.unread, cx))
            .child(Icon::new(state_icon).small().text_color(state_color))
            .child(avatar(&issue.author, AVATAR_SIZE))
            .child(title(&issue.title, item.unread, cx))
            .children(issue.issue_type.as_deref().map(|name| {
                let (icon, color) = type_style(name, cx);
                Icon::new(icon).xsmall().text_color(color)
            }))
            .child(mono(labels::short_repo_ref(&issue.repo, issue.number), cx)),
    )
    .child(reason(item, now, issue.updated_at, cx))
    .child(label_dots(&issue.labels, cx))
    .child(avatar_stack(
        issue.assignees.iter().map(String::as_str).collect(),
        cx,
    ))
    .child(comments(issue.comments, cx))
    .child(linkage(issue, cx))
    .child(age(now, issue.updated_at, cx))
}

fn subject_cell() -> Div {
    h_flex().flex_1().min_w_0().items_center().gap_2()
}

fn reason(item: &InboxItem, now: DateTime<Utc>, updated_at: DateTime<Utc>, cx: &App) -> Div {
    let color = ui::tone_color(labels::reason_tone(&item.reason), cx);
    reason_cell()
        .flex()
        .items_center()
        .gap(px(REASON_GAP))
        .child(
            div()
                .size(px(REASON_DOT))
                .flex_none()
                .rounded_full()
                .bg(color),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_sm()
                .text_color(color)
                .child(labels::reason_line(&item.reason, now, updated_at)),
        )
}

fn age(now: DateTime<Utc>, updated_at: DateTime<Utc>, cx: &App) -> Div {
    right_cell(AGE_WIDTH)
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .child(ui::age_label(now, updated_at))
}

fn unread_dot(unread: bool, cx: &App) -> Div {
    div()
        .size(px(UNREAD_DOT))
        .flex_none()
        .rounded_full()
        .when(unread, |this| this.bg(cx.theme().primary))
}

fn title(text: &str, unread: bool, cx: &App) -> Div {
    div()
        .flex_1()
        .min_w_0()
        .truncate()
        .text_color(cx.theme().foreground)
        .when(unread, |this| this.font_semibold())
        .child(text.to_string())
}

fn mono(text: impl Into<SharedString>, cx: &App) -> Div {
    let theme = cx.theme();
    div()
        .flex_none()
        .truncate()
        .font_family(theme.mono_font_family.clone())
        .text_xs()
        .text_color(theme.muted_foreground)
        .child(text.into())
}

fn no_value(cx: &App) -> Div {
    div()
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .child(labels::NO_VALUE)
}

fn reviewers(pr: &PullRequest, cx: &App) -> Div {
    let mut logins: Vec<&str> = Vec::new();
    let candidates = pr.requested_reviewers.iter().map(String::as_str).chain(
        pr.latest_reviews
            .iter()
            .map(|review| review.author.as_str()),
    );
    for login in candidates {
        if !logins.iter().any(|known| known.eq_ignore_ascii_case(login)) {
            logins.push(login);
        }
    }
    avatar_stack(logins, cx)
}

fn avatar_stack(logins: Vec<&str>, cx: &App) -> Div {
    let theme = cx.theme();
    h_flex().w(px(REVIEWERS_WIDTH)).flex_none().children(
        logins
            .into_iter()
            .take(MAX_REVIEWERS)
            .enumerate()
            .map(|(ix, login)| {
                avatar(login, AVATAR_SIZE)
                    .border_1()
                    .border_color(theme.background)
                    .when(ix > 0, |this| this.ml(px(REVIEWER_OVERLAP)))
            }),
    )
}

fn label_dots(issue_labels: &[IssueLabel], cx: &App) -> Div {
    let theme = cx.theme();
    let cell = h_flex()
        .w(px(CI_WIDTH))
        .flex_none()
        .min_w_0()
        .overflow_hidden()
        .gap_1p5()
        .items_center()
        .text_xs()
        .text_color(theme.muted_foreground);
    if issue_labels.is_empty() {
        return cell.child(no_value(cx));
    }
    let (shown, more) = labels::visible_labels(issue_labels);
    cell.children(shown.iter().map(|label| {
        let color: Hsla = label_color(&label.color).unwrap_or(theme.muted_foreground);
        h_flex()
            .flex_shrink(1.)
            .min_w_0()
            .gap_1()
            .items_center()
            .child(
                div()
                    .size(px(LABEL_DOT))
                    .flex_none()
                    .rounded_full()
                    .bg(color),
            )
            .child(div().min_w_0().truncate().child(label.name.clone()))
    }))
    .children(more.map(|more| div().flex_none().child(more)))
}

fn linkage(issue: &Issue, cx: &App) -> Div {
    let theme = cx.theme();
    let chip = if let Some(linked) = issue.linked_prs.first() {
        let style = state_style_for(linked.state, linked.is_draft, cx);
        Some(
            tag(cx)
                .child(Icon::new(style.icon).xsmall().text_color(style.color))
                .child(
                    div()
                        .font_family(theme.mono_font_family.clone())
                        .text_color(theme.foreground)
                        .child(format!("#{}", linked.number)),
                ),
        )
    } else {
        issue.sub_issues.map(|sub| {
            tag(cx)
                .child(Icon::new(IconName::ListTree).xsmall())
                .child(format!("{}/{}", sub.completed, sub.total))
        })
    };
    right_cell(DIFF_WIDTH).child(chip.unwrap_or_else(|| no_value(cx)))
}

fn tag(cx: &App) -> Div {
    let theme = cx.theme();
    h_flex()
        .flex_none()
        .h(px(TAG_HEIGHT))
        .px_1p5()
        .gap_1()
        .items_center()
        .rounded(theme.radius)
        .border_1()
        .border_color(theme.border)
        .text_xs()
        .text_color(theme.muted_foreground)
}

fn comments(count: u32, cx: &App) -> Div {
    let theme = cx.theme();
    h_flex()
        .w(px(COMMENTS_WIDTH))
        .flex_none()
        .gap_1()
        .justify_end()
        .text_xs()
        .text_color(theme.muted_foreground)
        .child(Icon::new(IconName::MessageSquare).xsmall())
        .child(count.to_string())
}
