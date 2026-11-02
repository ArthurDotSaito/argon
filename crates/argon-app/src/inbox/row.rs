use argon_core::{ChecksState, PullRequest};
use chrono::{DateTime, Utc};
use gpui_kit::assets::IconName;
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, StyledExt as _, h_flex};
use gpui_kit::{
    App, Div, InteractiveElement as _, ParentElement as _, SharedString, Stateful, Styled as _,
    div, prelude::FluentBuilder as _, px,
};

use super::items::InboxItem;
use super::labels;
use crate::ui::{self, avatar, state_icon};

pub const PR_ROW_HEIGHT: f32 = 52.;
const HEADER_HEIGHT: f32 = 28.;
const AVATAR_SIZE: f32 = 18.;
const REVIEWER_SIZE: f32 = 16.;
const REVIEWER_OVERLAP: f32 = -5.;
const MAX_REVIEWERS: usize = 3;
const UNREAD_DOT: f32 = 6.;
const REASON_INDENT: f32 = 62.;
const BRANCH_MAX_WIDTH: f32 = 240.;
const SEPARATOR: &str = "·";
const CI_WIDTH: f32 = 18.;
const REVIEWERS_WIDTH: f32 = 46.;
const DIFF_WIDTH: f32 = 104.;
const COMMENTS_WIDTH: f32 = 40.;
const AGE_WIDTH: f32 = 52.;

pub fn header(title: SharedString, count: usize, cx: &App) -> Div {
    let theme = cx.theme();
    h_flex()
        .w_full()
        .h(px(HEADER_HEIGHT))
        .px_3()
        .gap_1p5()
        .items_center()
        .bg(theme.muted)
        .border_b_1()
        .border_color(theme.border)
        .text_xs()
        .child(
            div()
                .font_semibold()
                .text_color(theme.foreground)
                .child(title),
        )
        .child(
            div()
                .text_color(theme.muted_foreground)
                .child(count.to_string()),
        )
}

pub fn pr_row(
    position: usize,
    item: &InboxItem,
    selected: bool,
    now: DateTime<Utc>,
    cx: &App,
) -> Stateful<Div> {
    let theme = cx.theme();
    let pr = &item.pr;
    let summary = h_flex()
        .w_full()
        .min_w_0()
        .gap_2()
        .items_center()
        .child(unread_dot(item.unread, cx))
        .child(state_icon(pr, cx))
        .child(avatar(&pr.author, AVATAR_SIZE))
        .child(title(item, cx))
        .child(checks(pr.checks, cx))
        .child(reviewers(pr, cx))
        .child(diff_stat(pr, cx))
        .child(comments(pr.comments, cx))
        .child(
            mono(ui::age_label(now, pr.updated_at), cx)
                .w(px(AGE_WIDTH))
                .flex()
                .justify_end(),
        );
    let details = h_flex()
        .w_full()
        .min_w_0()
        .pl(px(REASON_INDENT))
        .gap_1p5()
        .items_center()
        .text_xs()
        .text_color(theme.muted_foreground)
        .child(mono(labels::repo_ref(&pr.repo, pr.number), cx))
        .child(SEPARATOR)
        .child(
            mono(pr.head.clone(), cx)
                .flex_shrink(1.)
                .min_w_0()
                .max_w(px(BRANCH_MAX_WIDTH)),
        )
        .child(SEPARATOR)
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .child(labels::reason_line(&item.reason, now, pr.updated_at)),
        );

    div()
        .id(("inbox-row", position))
        .w_full()
        .overflow_hidden()
        .h(px(PR_ROW_HEIGHT))
        .flex()
        .flex_col()
        .justify_center()
        .gap_0p5()
        .pl_2()
        .pr_3()
        .border_l_2()
        .border_color(if selected {
            theme.primary
        } else {
            gpui_kit::transparent_black()
        })
        .when(selected, |this| this.bg(theme.list_active))
        .when(!selected, |this| {
            this.hover(|style| style.bg(theme.list_hover))
        })
        .cursor_pointer()
        .child(summary)
        .child(details)
}

fn unread_dot(unread: bool, cx: &App) -> Div {
    div()
        .size(px(UNREAD_DOT))
        .flex_none()
        .rounded_full()
        .when(unread, |this| this.bg(cx.theme().primary))
}

fn title(item: &InboxItem, cx: &App) -> Div {
    div()
        .flex_1()
        .min_w_0()
        .truncate()
        .text_color(cx.theme().foreground)
        .when(item.unread, |this| this.font_semibold())
        .child(item.pr.title.clone())
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

fn checks(checks: Option<ChecksState>, cx: &App) -> Div {
    h_flex()
        .w(px(CI_WIDTH))
        .flex_none()
        .justify_center()
        .child(ui::checks_glyph(checks, cx))
}
fn reviewers(pr: &PullRequest, cx: &App) -> Div {
    let theme = cx.theme();
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
    h_flex().w(px(REVIEWERS_WIDTH)).flex_none().children(
        logins
            .into_iter()
            .take(MAX_REVIEWERS)
            .enumerate()
            .map(|(ix, login)| {
                avatar(login, REVIEWER_SIZE)
                    .border_1()
                    .border_color(theme.background)
                    .when(ix > 0, |this| this.ml(px(REVIEWER_OVERLAP)))
            }),
    )
}

fn diff_stat(pr: &PullRequest, cx: &App) -> Div {
    h_flex()
        .w(px(DIFF_WIDTH))
        .flex_none()
        .justify_end()
        .child(ui::diff_stat(pr.additions, pr.deletions, cx))
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
