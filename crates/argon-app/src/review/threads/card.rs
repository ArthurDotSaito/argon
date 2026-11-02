use argon_github::{ReviewThread, ThreadComment};
use chrono::{DateTime, Utc};
use gpui_kit::assets::IconName;
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, StyledExt as _, h_flex, v_flex};
use gpui_kit::{
    AnyElement, App, ClickEvent, Div, ElementId, Entity, InteractiveElement as _, IntoElement,
    ParentElement as _, StatefulInteractiveElement as _, Styled as _, Window, div,
};

use super::{ThreadUi, footer};
use crate::pr::markdown;
use crate::review::labels;
use crate::ui;

const AVATAR_SIZE: f32 = 20.;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CardKind {
    Line,
    Outdated,
    File,
}

pub fn outdated_header(cx: &mut App) -> AnyElement {
    let theme = cx.theme();
    h_flex()
        .gap_2()
        .items_center()
        .text_xs()
        .font_semibold()
        .text_color(theme.muted_foreground)
        .child(Icon::new(IconName::Clock).xsmall())
        .child(labels::OUTDATED_COMMENTS)
        .into_any_element()
}

pub fn thread_card(
    thread: &ReviewThread,
    ui_state: &Entity<ThreadUi>,
    kind: CardKind,
    _: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let expanded = !thread.resolved || ui_state.read(cx).is_expanded(&thread.id);
    let theme = cx.theme();
    let now = Utc::now();
    let card = v_flex()
        .w_full()
        .overflow_hidden()
        .rounded(theme.radius)
        .border_1()
        .border_color(theme.border)
        .bg(theme.background)
        .text_sm()
        .font_family(theme.font_family.clone());
    let caption = match kind {
        CardKind::Line => None,
        CardKind::Outdated => Some((IconName::Clock, labels::original_line(thread.original_line))),
        CardKind::File => Some((IconName::FileCode, labels::FILE_COMMENT.to_string())),
    };
    let card = if let Some((icon, caption)) = caption {
        card.child(
            h_flex()
                .px_3()
                .py_1()
                .gap_1p5()
                .items_center()
                .bg(theme.muted.opacity(0.5))
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(Icon::new(icon).xsmall())
                .child(caption),
        )
    } else {
        card
    };
    if !expanded {
        return card
            .child(resolved_bar(thread, ui_state, false, cx))
            .into_any_element();
    }
    let comments: Vec<Div> = thread
        .comments
        .iter()
        .enumerate()
        .map(|(ix, comment)| comment_row(ix, comment, now, cx))
        .collect();
    let more = thread.truncated_comments.then(|| {
        div()
            .px_3()
            .py_1p5()
            .border_t_1()
            .border_color(theme.border)
            .text_xs()
            .text_color(theme.muted_foreground)
            .child(labels::MORE_ON_GITHUB)
    });
    let resolved = thread
        .resolved
        .then(|| resolved_bar(thread, ui_state, true, cx));
    let footer = footer::thread_footer(thread, ui_state, cx);
    card.children(resolved)
        .children(comments)
        .children(more)
        .child(footer)
        .into_any_element()
}

fn resolved_bar(
    thread: &ReviewThread,
    ui_state: &Entity<ThreadUi>,
    expanded: bool,
    cx: &App,
) -> Div {
    let theme = cx.theme();
    let toggle_state = ui_state.clone();
    let id = thread.id.clone();
    let action = if expanded {
        labels::HIDE_RESOLVED
    } else {
        labels::SHOW_RESOLVED
    };
    let summary = thread.comments.first().map(|comment| {
        format!(
            "{} {} {}",
            comment.author,
            labels::SEPARATOR,
            labels::comment_count(thread.comments.len())
        )
    });
    let bar = h_flex()
        .px_3()
        .py_1p5()
        .gap_2()
        .items_center()
        .text_xs()
        .text_color(theme.muted_foreground)
        .child(
            Icon::new(IconName::CircleCheck)
                .xsmall()
                .text_color(theme.success),
        )
        .child(labels::RESOLVED)
        .children(summary.map(|summary| div().min_w_0().truncate().child(summary)))
        .child(div().flex_1())
        .child(
            div()
                .id(ElementId::Name(
                    format!("thread-toggle-{}", thread.id).into(),
                ))
                .flex_none()
                .text_color(theme.link)
                .cursor_pointer()
                .hover(|style| style.underline())
                .child(action)
                .on_click(move |_: &ClickEvent, _, cx| {
                    toggle_state.update(cx, |state, cx| state.toggle(&id, cx));
                }),
        );
    if expanded {
        bar.border_b_1().border_color(theme.border)
    } else {
        bar
    }
}

fn comment_row(ix: usize, comment: &ThreadComment, now: DateTime<Utc>, cx: &App) -> Div {
    let theme = cx.theme();
    let row = v_flex()
        .px_3()
        .py_2()
        .gap_1()
        .child(
            h_flex()
                .gap_2()
                .items_center()
                .child(ui::avatar(&comment.author, AVATAR_SIZE))
                .child(
                    div()
                        .font_semibold()
                        .text_color(theme.foreground)
                        .child(comment.author.clone()),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(ui::age_label(now, comment.created_at)),
                ),
        )
        .child(div().pl_7().text_color(theme.foreground).child(markdown(
            format!("thread-comment-{}", comment.id).into(),
            comment.body.clone().into(),
        )));
    if ix > 0 {
        row.border_t_1().border_color(theme.border)
    } else {
        row
    }
}
