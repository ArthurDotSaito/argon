use argon_core::ReviewState;
use argon_github::{Label, ReviewComment, TimelineItem};
use chrono::{DateTime, Utc};
use gpui_kit::assets::IconName;
use gpui_kit::component::text::TextView;
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, StyledExt as _, h_flex, v_flex};
use gpui_kit::{
    AnyElement, App, Div, ElementId, Hsla, IntoElement, ParentElement as _, SharedString,
    Styled as _, div, px,
};

use super::header::label_chip;
use super::labels;
use crate::ui;

const CARD_AVATAR: f32 = 28.;
const EVENT_ICON_BOX: f32 = 24.;
const HUNK_TAIL_LINES: usize = 6;

pub fn timestamp(item: &TimelineItem) -> DateTime<Utc> {
    match item {
        TimelineItem::Comment { created_at, .. } => *created_at,
        TimelineItem::Review { submitted_at, .. } => *submitted_at,
        TimelineItem::Commit { committed_at, .. } => *committed_at,
        TimelineItem::ReviewRequested { at, .. }
        | TimelineItem::Labeled { at, .. }
        | TimelineItem::HeadRefForcePushed { at, .. }
        | TimelineItem::Merged { at, .. }
        | TimelineItem::Closed { at, .. }
        | TimelineItem::ReadyForReview { at, .. } => *at,
    }
}

pub fn is_conversation(item: &TimelineItem) -> bool {
    matches!(
        item,
        TimelineItem::Comment { .. } | TimelineItem::Review { .. }
    )
}

pub fn markdown(id: SharedString, body: SharedString) -> TextView {
    TextView::markdown(ElementId::Name(id), body).selectable(true)
}

pub struct MarkdownBlock {
    pub id: SharedString,
    pub body: SharedString,
}

impl MarkdownBlock {
    pub fn new(id: String, body: &str) -> Self {
        Self {
            id: id.into(),
            body: body.to_string().into(),
        }
    }

    pub fn view(&self) -> TextView {
        markdown(self.id.clone(), self.body.clone())
    }
}

pub struct Entry {
    pub ix: usize,
    body: Option<MarkdownBlock>,
    comments: Vec<MarkdownBlock>,
}

impl Entry {
    pub fn new(ix: usize, item: &TimelineItem) -> Self {
        let (body, comments) = match item {
            TimelineItem::Comment { id, body, .. } => (
                Some(MarkdownBlock::new(format!("pr-comment-{id}"), body)),
                Vec::new(),
            ),
            TimelineItem::Review {
                id, body, comments, ..
            } => (
                (!body.trim().is_empty())
                    .then(|| MarkdownBlock::new(format!("pr-review-{id}"), body)),
                comments
                    .iter()
                    .enumerate()
                    .map(|(ix, comment)| {
                        MarkdownBlock::new(format!("pr-review-{id}-comment-{ix}"), &comment.body)
                    })
                    .collect(),
            ),
            _ => (None, Vec::new()),
        };
        Self { ix, body, comments }
    }
}

pub fn timeline_item(
    item: &TimelineItem,
    entry: &Entry,
    pr_labels: &[Label],
    now: DateTime<Utc>,
    cx: &App,
) -> AnyElement {
    let theme = cx.theme();
    match item {
        TimelineItem::Comment {
            author, created_at, ..
        } => card(
            author,
            card_body(author, div().child(labels::COMMENTED), *created_at, now, cx)
                .children(entry.body.as_ref().map(|body| card_text(body.view()))),
        )
        .into_any_element(),
        TimelineItem::Review {
            author,
            state,
            submitted_at,
            comments,
            ..
        } => {
            let header = h_flex()
                .gap_1p5()
                .items_center()
                .child(labels::REVIEWED)
                .child(review_pill(*state, cx));
            let mut body_el = card_body(author, header, *submitted_at, now, cx)
                .children(entry.body.as_ref().map(|body| card_text(body.view())));
            if !comments.is_empty() {
                let quotes = comments
                    .iter()
                    .zip(&entry.comments)
                    .map(|(comment, body)| review_comment(author, comment, body, cx));
                body_el = body_el.child(v_flex().gap_2().p_3().children(quotes));
            }
            card(author, body_el).into_any_element()
        }
        TimelineItem::Commit {
            oid,
            headline,
            committed_at,
            ..
        } => event(
            IconName::GitCommitHorizontal,
            theme.muted_foreground,
            h_flex()
                .gap_2()
                .min_w_0()
                .child(
                    div()
                        .flex_none()
                        .font_family(theme.mono_font_family.clone())
                        .text_xs()
                        .text_color(theme.link)
                        .child(labels::short_sha(oid).to_string()),
                )
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_color(theme.foreground)
                        .child(headline.clone()),
                ),
            *committed_at,
            now,
            cx,
        ),
        TimelineItem::ReviewRequested {
            actor,
            reviewer,
            at,
        } => event(
            IconName::UserPlus,
            theme.muted_foreground,
            div().child(labels::review_requested(actor, reviewer)),
            *at,
            now,
            cx,
        ),
        TimelineItem::Labeled { actor, label, at } => {
            let color = pr_labels
                .iter()
                .find(|known| known.name.eq_ignore_ascii_case(label))
                .map(|known| known.color.as_str());
            event(
                IconName::Tag,
                theme.muted_foreground,
                h_flex()
                    .gap_1p5()
                    .items_center()
                    .child(labels::labeled(actor))
                    .child(label_chip(label, color, cx)),
                *at,
                now,
                cx,
            )
        }
        TimelineItem::HeadRefForcePushed { actor, at } => event(
            IconName::GitBranch,
            theme.warning,
            div().child(labels::force_pushed(actor)),
            *at,
            now,
            cx,
        ),
        TimelineItem::Merged { actor, at } => event(
            IconName::GitMerge,
            theme.magenta,
            div().child(labels::merged(actor)),
            *at,
            now,
            cx,
        ),
        TimelineItem::Closed { actor, at } => event(
            IconName::GitPullRequestClosed,
            theme.danger,
            div().child(labels::closed(actor)),
            *at,
            now,
            cx,
        ),
        TimelineItem::ReadyForReview { actor, at } => event(
            IconName::Eye,
            theme.primary,
            div().child(labels::ready_for_review(actor)),
            *at,
            now,
            cx,
        ),
    }
}

pub fn card(author: &str, body: Div) -> Div {
    h_flex()
        .w_full()
        .min_w_0()
        .gap_3()
        .items_start()
        .child(ui::avatar(author, CARD_AVATAR))
        .child(body)
}

pub fn card_body(
    author: &str,
    action: impl IntoElement,
    at: DateTime<Utc>,
    now: DateTime<Utc>,
    cx: &App,
) -> Div {
    let theme = cx.theme();
    let header = h_flex()
        .w_full()
        .px_3()
        .py_1p5()
        .gap_1p5()
        .items_center()
        .bg(theme.muted)
        .border_b_1()
        .border_color(theme.border)
        .text_sm()
        .text_color(theme.muted_foreground)
        .child(
            div()
                .font_semibold()
                .text_color(theme.foreground)
                .child(author.to_string()),
        )
        .child(action)
        .child(div().flex_1())
        .child(
            div()
                .font_family(theme.mono_font_family.clone())
                .text_xs()
                .child(ui::age_label(now, at)),
        );
    v_flex()
        .flex_1()
        .min_w_0()
        .overflow_hidden()
        .border_1()
        .border_color(theme.border)
        .rounded(theme.radius)
        .bg(theme.background)
        .child(header)
}

pub fn card_text(content: impl IntoElement) -> Div {
    div()
        .w_full()
        .min_w_0()
        .px_3()
        .py_2()
        .text_sm()
        .child(content)
}

fn event(
    icon: IconName,
    color: Hsla,
    text: impl IntoElement,
    at: DateTime<Utc>,
    now: DateTime<Utc>,
    cx: &App,
) -> AnyElement {
    let theme = cx.theme();
    h_flex()
        .w_full()
        .gap_3()
        .items_center()
        .pl(px(2.))
        .text_sm()
        .text_color(theme.muted_foreground)
        .child(
            div()
                .size(px(EVENT_ICON_BOX))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .rounded_full()
                .border_1()
                .border_color(theme.border)
                .bg(theme.background)
                .child(Icon::new(icon).xsmall().text_color(color)),
        )
        .child(div().flex_1().min_w_0().child(text))
        .child(
            div()
                .flex_none()
                .font_family(theme.mono_font_family.clone())
                .text_xs()
                .child(ui::age_label(now, at)),
        )
        .into_any_element()
}

pub fn review_style(state: ReviewState, cx: &App) -> (IconName, Hsla) {
    let theme = cx.theme();
    match state {
        ReviewState::Approved => (IconName::CircleCheck, theme.success),
        ReviewState::ChangesRequested => (IconName::CircleX, theme.danger),
        ReviewState::Commented => (IconName::MessageSquare, theme.muted_foreground),
        ReviewState::Dismissed => (IconName::Minus, theme.muted_foreground),
        ReviewState::Pending => (IconName::CircleDashed, theme.warning),
    }
}

fn review_pill(state: ReviewState, cx: &App) -> Div {
    let theme = cx.theme();
    let (icon, color) = review_style(state, cx);
    h_flex()
        .gap_1()
        .px_1p5()
        .h(px(18.))
        .items_center()
        .rounded_full()
        .border_1()
        .border_color(color.opacity(0.5))
        .bg(color.opacity(0.12))
        .text_xs()
        .text_color(if state == ReviewState::Commented {
            theme.foreground
        } else {
            color
        })
        .child(Icon::new(icon).xsmall())
        .child(labels::review_state(state))
}

fn review_comment(author: &str, comment: &ReviewComment, body: &MarkdownBlock, cx: &App) -> Div {
    let theme = cx.theme();
    let file = h_flex()
        .px_2()
        .py_1()
        .gap_1p5()
        .items_center()
        .bg(theme.muted)
        .border_b_1()
        .border_color(theme.border)
        .font_family(theme.mono_font_family.clone())
        .text_xs()
        .text_color(theme.muted_foreground)
        .child(Icon::new(IconName::FileCode).xsmall())
        .child(
            div()
                .min_w_0()
                .truncate()
                .child(labels::line_ref(&comment.path, comment.line)),
        );
    let reply = h_flex()
        .px_2()
        .py_1p5()
        .gap_2()
        .items_start()
        .border_t_1()
        .border_color(theme.border)
        .child(ui::avatar(author, 18.))
        .child(div().flex_1().min_w_0().text_sm().child(body.view()));
    v_flex()
        .w_full()
        .border_1()
        .border_color(theme.border)
        .rounded(theme.radius)
        .overflow_hidden()
        .child(file)
        .child(diff_hunk(&comment.diff_hunk, cx))
        .child(reply)
}

fn diff_hunk(hunk: &str, cx: &App) -> Div {
    let theme = cx.theme();
    let lines: Vec<&str> = hunk.lines().collect();
    let tail = lines.len().saturating_sub(HUNK_TAIL_LINES);
    v_flex()
        .py_1()
        .bg(theme.background)
        .font_family(theme.mono_font_family.clone())
        .text_xs()
        .children(lines[tail..].iter().map(|line| {
            let (color, bg) = match line.chars().next() {
                Some('+') => (theme.success, Some(theme.success.opacity(0.1))),
                Some('-') => (theme.danger, Some(theme.danger.opacity(0.1))),
                Some('@') => (theme.muted_foreground, None),
                _ => (theme.foreground, None),
            };
            let row = div()
                .px_2()
                .whitespace_nowrap()
                .truncate()
                .text_color(color)
                .child(display_line(line));
            match bg {
                Some(bg) => row.bg(bg),
                None => row,
            }
        }))
}

fn display_line(line: &str) -> String {
    let line = line.replace('\t', "    ");
    if line.is_empty() {
        " ".to_string()
    } else {
        line
    }
}
