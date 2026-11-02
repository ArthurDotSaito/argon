use argon_core::{ChecksState, PrState, PullRequest};
use chrono::{DateTime, Utc};
use gpui_kit::assets::IconName;
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, StyledExt as _, h_flex};
use gpui_kit::{
    AnyElement, App, Div, Hsla, InteractiveElement as _, IntoElement, ParentElement as _,
    SharedString, Stateful, Styled as _, div, hsla, prelude::FluentBuilder as _, px, white,
};

use super::items::InboxItem;
use super::labels;

pub const PR_ROW_HEIGHT: f32 = 52.;
const HEADER_HEIGHT: f32 = 28.;
const AVATAR_SIZE: f32 = 18.;
const REVIEWER_SIZE: f32 = 16.;
const REVIEWER_OVERLAP: f32 = -5.;
const MAX_REVIEWERS: usize = 3;
const UNREAD_DOT: f32 = 6.;
const REASON_INDENT: f32 = 62.;
const BRANCH_MAX_WIDTH: f32 = 200.;
const BRANCH_SHRINK: f32 = 1.5;
const REPO_WIDTH: f32 = 230.;
const CI_WIDTH: f32 = 18.;
const REVIEWERS_WIDTH: f32 = 46.;
const DIFF_WIDTH: f32 = 104.;
const COMMENTS_WIDTH: f32 = 40.;
const AGE_WIDTH: f32 = 52.;
const AVATAR_SATURATION: f32 = 0.45;
const AVATAR_LIGHTNESS: f32 = 0.42;

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
        .child(title_and_branch(item, cx))
        .child(mono(labels::repo_ref(&pr.repo, pr.number), cx).w(px(REPO_WIDTH)))
        .child(checks(pr.checks, cx))
        .child(reviewers(pr, cx))
        .child(diff_stat(pr, cx))
        .child(comments(pr.comments, cx))
        .child(
            mono(labels::age_label(now, pr.updated_at), cx)
                .w(px(AGE_WIDTH))
                .flex()
                .justify_end(),
        );
    let reason = div()
        .pl(px(REASON_INDENT))
        .truncate()
        .text_xs()
        .text_color(theme.muted_foreground)
        .child(labels::reason_line(&item.reason, now, pr.updated_at));

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
        .child(reason)
}

pub fn avatar(login: &str, size: f32) -> Div {
    div()
        .size(px(size))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .bg(avatar_color(login))
        .text_color(white())
        .text_size(px(size * 0.45))
        .font_semibold()
        .child(initials(login))
}

fn initials(name: &str) -> String {
    let parts: Vec<&str> = name
        .split(['-', '_', '.', ' '])
        .filter(|part| !part.is_empty())
        .collect();
    let picked: String = match parts.as_slice() {
        [first, second, ..] => first
            .chars()
            .take(1)
            .chain(second.chars().take(1))
            .collect(),
        _ => name
            .chars()
            .filter(|c| c.is_alphanumeric())
            .take(2)
            .collect(),
    };
    picked.to_uppercase()
}

fn avatar_color(name: &str) -> Hsla {
    let hash = name
        .to_lowercase()
        .bytes()
        .fold(0x811c_9dc5_u32, |hash, byte| {
            (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193)
        });
    let hue = (hash % 360) as f32 / 360.;
    hsla(hue, AVATAR_SATURATION, AVATAR_LIGHTNESS, 1.)
}

fn unread_dot(unread: bool, cx: &App) -> Div {
    div()
        .size(px(UNREAD_DOT))
        .flex_none()
        .rounded_full()
        .when(unread, |this| this.bg(cx.theme().primary))
}

fn state_icon(pr: &PullRequest, cx: &App) -> Icon {
    let theme = cx.theme();
    let (icon, color) = match pr.state {
        PrState::Open if pr.is_draft => (IconName::GitPullRequestDraft, theme.muted_foreground),
        PrState::Open => (IconName::GitPullRequest, theme.success),
        PrState::Merged => (IconName::GitMerge, theme.magenta),
        PrState::Closed => (IconName::GitPullRequestClosed, theme.danger),
    };
    Icon::new(icon).small().text_color(color)
}

fn title_and_branch(item: &InboxItem, cx: &App) -> Div {
    let theme = cx.theme();
    h_flex()
        .flex_1()
        .min_w_0()
        .gap_2()
        .items_center()
        .child(
            div()
                .min_w_0()
                .flex_shrink(1.)
                .truncate()
                .text_color(theme.foreground)
                .when(item.unread, |this| this.font_semibold())
                .child(item.pr.title.clone()),
        )
        .child(
            mono(item.pr.head.clone(), cx)
                .min_w_0()
                .max_w(px(BRANCH_MAX_WIDTH))
                .flex_shrink(BRANCH_SHRINK),
        )
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
    let theme = cx.theme();
    let glyph: AnyElement = match checks {
        Some(ChecksState::Success) => Icon::new(IconName::Check)
            .xsmall()
            .text_color(theme.success)
            .into_any_element(),
        Some(ChecksState::Failure | ChecksState::Error) => Icon::new(IconName::X)
            .xsmall()
            .text_color(theme.danger)
            .into_any_element(),
        Some(ChecksState::Pending) => Icon::new(IconName::CircleDashed)
            .xsmall()
            .text_color(theme.warning)
            .into_any_element(),
        None => Icon::new(IconName::Minus)
            .xsmall()
            .text_color(theme.muted_foreground)
            .into_any_element(),
    };
    h_flex()
        .w(px(CI_WIDTH))
        .flex_none()
        .justify_center()
        .child(glyph)
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
    let theme = cx.theme();
    h_flex()
        .w(px(DIFF_WIDTH))
        .flex_none()
        .justify_end()
        .gap_1p5()
        .font_family(theme.mono_font_family.clone())
        .text_xs()
        .child(
            div()
                .text_color(theme.success)
                .child(format!("+{}", pr.additions)),
        )
        .child(
            div()
                .text_color(theme.danger)
                .child(format!("−{}", pr.deletions)),
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
