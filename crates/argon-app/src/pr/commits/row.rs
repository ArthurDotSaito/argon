use argon_github::CommitInfo;
use chrono::{DateTime, Utc};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{ActiveTheme as _, h_flex, v_flex};
use gpui_kit::{
    ClickEvent, Context, Div, InteractiveElement as _, MouseButton, ParentElement as _, Stateful,
    StatefulInteractiveElement as _, Styled as _, div, prelude::FluentBuilder as _, px,
};

use super::{CommitsTab, labels};
use crate::pr::labels::short_sha;
use crate::ui;

const AVATAR_SIZE: f32 = 20.;
const AUTHOR_MAX_WIDTH: f32 = 140.;
const SHA_WIDTH: f32 = 64.;
const CHECKS_WIDTH: f32 = 18.;
const DIFF_WIDTH: f32 = 96.;
const AGE_WIDTH: f32 = 52.;
const BODY_INDENT: f32 = 30.;
const ELLIPSIS: &str = "…";

#[derive(Clone, Copy)]
pub struct CommitRow {
    pub ix: usize,
    pub expanded: bool,
    pub copied: bool,
}

pub fn commit_row(
    commit: &CommitInfo,
    state: CommitRow,
    now: DateTime<Utc>,
    cx: &mut Context<CommitsTab>,
) -> Stateful<Div> {
    let ix = state.ix;
    let author = commit
        .author_login
        .as_deref()
        .unwrap_or(&commit.author_name);
    let has_body = !commit.body.trim().is_empty();
    let summary = h_flex()
        .w_full()
        .min_w_0()
        .gap_2()
        .items_center()
        .child(ui::avatar(author, AVATAR_SIZE))
        .child(headline(commit, state, has_body, cx))
        .child(
            div()
                .flex_none()
                .max_w(px(AUTHOR_MAX_WIDTH))
                .truncate()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(author.to_string()),
        )
        .child(sha(commit, state, cx))
        .child(
            h_flex()
                .w(px(CHECKS_WIDTH))
                .flex_none()
                .justify_center()
                .child(ui::checks_glyph(commit.checks, cx)),
        )
        .child(
            h_flex()
                .w(px(DIFF_WIDTH))
                .flex_none()
                .justify_end()
                .child(ui::diff_stat(commit.additions, commit.deletions, cx)),
        )
        .child(
            div()
                .w(px(AGE_WIDTH))
                .flex_none()
                .flex()
                .justify_end()
                .text_xs()
                .font_family(cx.theme().mono_font_family.clone())
                .text_color(cx.theme().muted_foreground)
                .child(ui::age_label(now, commit.committed_at)),
        );
    let theme = cx.theme();
    v_flex()
        .id(("commit-row", ix))
        .w_full()
        .min_w_0()
        .px_3()
        .py_2()
        .gap_1p5()
        .when(ix > 0, |this| this.border_t_1().border_color(theme.border))
        .cursor_pointer()
        .hover(|style| style.bg(theme.list_hover))
        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.open_commit(ix, cx)))
        .child(summary)
        .when(state.expanded && has_body, |this| {
            this.child(body(&commit.body, cx))
        })
}

fn headline(
    commit: &CommitInfo,
    state: CommitRow,
    has_body: bool,
    cx: &mut Context<CommitsTab>,
) -> Div {
    let ix = state.ix;
    let theme = cx.theme();
    let toggle = has_body.then(|| {
        div()
            .id(("commit-more", ix))
            .flex_none()
            .px_1()
            .rounded(theme.radius)
            .bg(theme.muted)
            .text_xs()
            .text_color(theme.muted_foreground)
            .cursor_pointer()
            .hover(|style| style.text_color(theme.foreground))
            .tooltip(|window, cx| Tooltip::new(labels::TOGGLE_MESSAGE).build(window, cx))
            .child(ELLIPSIS)
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                this.toggle_message(ix, cx);
            }))
    });
    h_flex()
        .flex_1()
        .min_w_0()
        .gap_1p5()
        .items_center()
        .child(
            div()
                .min_w_0()
                .truncate()
                .text_sm()
                .text_color(theme.foreground)
                .child(commit.headline.clone()),
        )
        .children(toggle)
}

fn sha(commit: &CommitInfo, state: CommitRow, cx: &mut Context<CommitsTab>) -> Stateful<Div> {
    let ix = state.ix;
    let theme = cx.theme();
    let (text, color) = if state.copied {
        (labels::COPIED, theme.success)
    } else {
        (short_sha(&commit.oid), theme.muted_foreground)
    };
    div()
        .id(("commit-sha", ix))
        .w(px(SHA_WIDTH))
        .flex_none()
        .px_1()
        .rounded(theme.radius)
        .text_xs()
        .text_center()
        .font_family(theme.mono_font_family.clone())
        .text_color(color)
        .cursor_pointer()
        .hover(|style| style.bg(theme.muted))
        .tooltip(|window, cx| Tooltip::new(labels::COPY_SHA).build(window, cx))
        .child(text.to_string())
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
            this.copy_sha(ix, cx);
        }))
}

fn body(text: &str, cx: &Context<CommitsTab>) -> Div {
    let theme = cx.theme();
    let lines: Vec<Div> = text
        .trim()
        .lines()
        .map(|line| div().min_h(px(8.)).child(line.to_string()))
        .collect();
    v_flex()
        .pl(px(BODY_INDENT))
        .text_xs()
        .font_family(theme.mono_font_family.clone())
        .text_color(theme.muted_foreground)
        .children(lines)
}
