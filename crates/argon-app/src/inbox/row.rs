use argon_core::PullRequest;
use chrono::{DateTime, Utc};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::Button;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Icon, Sizable as _, StyledExt as _, h_flex,
};
use gpui_kit::{
    App, ClickEvent, Div, ParentElement as _, SharedString, Stateful, Styled as _, Window, div,
    prelude::FluentBuilder as _, px,
};

use super::items::InboxItem;
use super::labels;
use crate::theme::density;
use crate::ui::{self, avatar, state_icon};

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

pub fn column_header(cx: &App) -> Div {
    ui::columns::column_header(cx)
        .child(div().flex_1().min_w_0().child(labels::COLUMN_PR))
        .child(reason_cell().child(labels::COLUMN_REASON))
        .child(div().w(px(CI_WIDTH)).flex_none().child(labels::COLUMN_CI))
        .child(
            div()
                .w(px(REVIEWERS_WIDTH))
                .flex_none()
                .child(labels::COLUMN_REVIEW),
        )
        .child(right_cell(COMMENTS_WIDTH).child(Icon::new(IconName::MessageSquare).xsmall()))
        .child(right_cell(DIFF_WIDTH).child(labels::COLUMN_DIFF))
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

pub fn pr_row(
    position: usize,
    item: &InboxItem,
    selected: bool,
    now: DateTime<Utc>,
    cx: &App,
) -> Stateful<Div> {
    let pr = &item.pr;
    ui::columns::list_row(("inbox-row", position), selected, cx)
        .child(
            h_flex()
                .flex_1()
                .min_w_0()
                .items_center()
                .gap_2()
                .child(unread_dot(item.unread, cx))
                .child(state_icon(pr, cx))
                .child(avatar(&pr.author, AVATAR_SIZE))
                .child(title(item, cx))
                .child(mono(labels::short_repo_ref(&pr.repo, pr.number), cx)),
        )
        .child(reason(item, now, cx))
        .child(
            div()
                .w(px(CI_WIDTH))
                .flex_none()
                .child(ui::checks_label(pr.checks, cx)),
        )
        .child(reviewers(pr, cx))
        .child(comments(pr.comments, cx))
        .child(right_cell(DIFF_WIDTH).child(ui::diff_stat(pr.additions, pr.deletions, cx)))
        .child(
            right_cell(AGE_WIDTH)
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(ui::age_label(now, pr.updated_at)),
        )
}

fn reason(item: &InboxItem, now: DateTime<Utc>, cx: &App) -> Div {
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
                .child(labels::reason_line(&item.reason, now, item.pr.updated_at)),
        )
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
                avatar(login, AVATAR_SIZE)
                    .border_1()
                    .border_color(theme.background)
                    .when(ix > 0, |this| this.ml(px(REVIEWER_OVERLAP)))
            }),
    )
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
