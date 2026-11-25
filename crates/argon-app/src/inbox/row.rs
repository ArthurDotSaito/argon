use argon_core::{Issue, IssueLabel, PullRequest};
use chrono::{DateTime, Utc};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::Button;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Icon, Sizable as _, StyledExt as _, h_flex,
};
use gpui_kit::{
    AnyElement, App, ClickEvent, Div, ElementId, Hsla, ParentElement as _, SharedString, Stateful,
    Styled as _, Window, div, prelude::FluentBuilder as _, px,
};

use super::columns::{Column, ColumnLayout, SUBJECT_MIN_WIDTH};
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
const TITLE_MIN_WIDTH: f32 = 96.;
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

pub fn column_header(
    kind: KindFilter,
    layout: &ColumnLayout,
    resize_handle: impl Fn(Column) -> AnyElement,
    cx: &App,
) -> Div {
    let titles = labels::column_titles(kind);
    let head = |column: Column, label: Div| {
        let width = layout.width(column)?;
        Some(
            sized(column, width)
                .relative()
                .h_full()
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .left(px(-ui::columns::COLUMN_GAP / 2.))
                        .h_full()
                        .w_0()
                        .child(resize_handle(column)),
                )
                .child(label.size_full().overflow_hidden()),
        )
    };
    let left = |title: &'static str| h_flex().child(title);
    let right = |title: &'static str| h_flex().justify_end().child(title);
    ui::columns::column_header(cx)
        .child(
            div()
                .flex_1()
                .min_w(px(SUBJECT_MIN_WIDTH))
                .truncate()
                .child(titles.subject),
        )
        .children(head(Column::Reason, left(labels::COLUMN_REASON)))
        .children(head(Column::Signal, left(titles.signal)))
        .children(head(Column::People, left(titles.people)))
        .children(head(
            Column::Comments,
            h_flex()
                .justify_end()
                .child(Icon::new(IconName::MessageSquare).xsmall()),
        ))
        .children(head(Column::Size, right(titles.size)))
        .children(head(Column::Updated, right(labels::COLUMN_UPDATED)))
}

fn sized(column: Column, width: f32) -> Div {
    let cell = div().w(px(width));
    if column == Column::Reason {
        cell.flex_shrink(1.).min_w(px(column.min_width()))
    } else {
        cell.flex_none()
    }
}

fn cell(column: Column, layout: &ColumnLayout) -> Option<Div> {
    let width = layout.width(column)?;
    Some(sized(column, width).flex().items_center().overflow_hidden())
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
    layout: &ColumnLayout,
    now: DateTime<Utc>,
    cx: &App,
) -> Stateful<Div> {
    let row = ui::columns::list_row(("inbox-row", position), selected, cx);
    match &item.subject {
        Subject::Pr(pr) => pr_cells(row, pr, Some(item), layout, now, cx),
        Subject::Issue(issue) => issue_cells(row, item, issue, layout, now, cx),
    }
}

pub fn pr_row(
    id: impl Into<ElementId>,
    pr: &PullRequest,
    item: Option<&InboxItem>,
    now: DateTime<Utc>,
    cx: &App,
) -> Stateful<Div> {
    let row = ui::columns::list_row(id, false, cx);
    pr_cells(row, pr, item, &ColumnLayout::default(), now, cx)
}

fn pr_cells(
    row: Stateful<Div>,
    pr: &PullRequest,
    item: Option<&InboxItem>,
    layout: &ColumnLayout,
    now: DateTime<Utc>,
    cx: &App,
) -> Stateful<Div> {
    let unread = item.is_some_and(|item| item.unread);
    let reason = match item {
        Some(item) => reason(item, layout, now, pr.updated_at, cx),
        None => cell(Column::Reason, layout)
            .map(|cell| cell.child(mono(pr.head.clone(), cx).flex_shrink(1.).min_w_0())),
    };
    row.child(
        subject_cell()
            .child(unread_dot(unread, cx))
            .child(state_icon(pr, cx))
            .child(avatar(&pr.author, AVATAR_SIZE))
            .child(title(&pr.title, unread, cx))
            .child(repo_ref(&pr.repo, pr.number, cx)),
    )
    .children(reason)
    .children(cell(Column::Signal, layout).map(|cell| cell.child(ui::checks_label(pr.checks, cx))))
    .children(reviewers(pr, layout, cx))
    .children(comments(pr.comments, layout, cx))
    .children(cell(Column::Size, layout).map(|cell| {
        cell.justify_end()
            .child(ui::diff_stat(pr.additions, pr.deletions, cx))
    }))
    .children(age(layout, now, pr.updated_at, cx))
}

fn issue_cells(
    row: Stateful<Div>,
    item: &InboxItem,
    issue: &Issue,
    layout: &ColumnLayout,
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
            .child(repo_ref(&issue.repo, issue.number, cx)),
    )
    .children(reason(item, layout, now, issue.updated_at, cx))
    .children(label_dots(&issue.labels, layout, cx))
    .children(avatar_stack(
        issue.assignees.iter().map(String::as_str).collect(),
        layout,
        cx,
    ))
    .children(comments(issue.comments, layout, cx))
    .children(linkage(issue, layout, cx))
    .children(age(layout, now, issue.updated_at, cx))
}

fn subject_cell() -> Div {
    h_flex()
        .flex_1()
        .min_w(px(SUBJECT_MIN_WIDTH))
        .overflow_hidden()
        .items_center()
        .gap_2()
}

fn reason(
    item: &InboxItem,
    layout: &ColumnLayout,
    now: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    cx: &App,
) -> Option<Div> {
    let color = ui::tone_color(labels::reason_tone(&item.reason), cx);
    Some(
        cell(Column::Reason, layout)?
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
            ),
    )
}

fn age(
    layout: &ColumnLayout,
    now: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    cx: &App,
) -> Option<Div> {
    Some(
        cell(Column::Updated, layout)?
            .justify_end()
            .text_xs()
            .text_color(cx.theme().muted_foreground)
            .child(ui::age_label(now, updated_at)),
    )
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
        .min_w(px(TITLE_MIN_WIDTH))
        .truncate()
        .text_color(cx.theme().foreground)
        .when(unread, |this| this.font_semibold())
        .child(text.to_string())
}

fn repo_ref(repo: &str, number: u64, cx: &App) -> Div {
    mono(labels::short_repo_ref(repo, number), cx)
        .flex_shrink(1.)
        .min_w_0()
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

fn reviewers(pr: &PullRequest, layout: &ColumnLayout, cx: &App) -> Option<Div> {
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
    avatar_stack(logins, layout, cx)
}

fn avatar_stack(logins: Vec<&str>, layout: &ColumnLayout, cx: &App) -> Option<Div> {
    let theme = cx.theme();
    Some(
        cell(Column::People, layout)?.children(
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
        ),
    )
}

fn label_dots(issue_labels: &[IssueLabel], layout: &ColumnLayout, cx: &App) -> Option<Div> {
    let theme = cx.theme();
    let cell = cell(Column::Signal, layout)?
        .gap_1p5()
        .text_xs()
        .text_color(theme.muted_foreground);
    if issue_labels.is_empty() {
        return Some(cell.child(no_value(cx)));
    }
    let (shown, more) = labels::visible_labels(issue_labels);
    let dots = cell.children(shown.iter().map(|label| {
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
    }));
    Some(dots.children(more.map(|more| div().flex_none().child(more))))
}

fn linkage(issue: &Issue, layout: &ColumnLayout, cx: &App) -> Option<Div> {
    let theme = cx.theme();
    let size = cell(Column::Size, layout)?.justify_end();
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
    Some(size.child(chip.unwrap_or_else(|| no_value(cx))))
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

fn comments(count: u32, layout: &ColumnLayout, cx: &App) -> Option<Div> {
    let theme = cx.theme();
    Some(
        cell(Column::Comments, layout)?
            .gap_1()
            .justify_end()
            .text_xs()
            .text_color(theme.muted_foreground)
            .child(Icon::new(IconName::MessageSquare).xsmall())
            .child(count.to_string()),
    )
}
