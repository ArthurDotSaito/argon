use argon_core::ReviewState;
use argon_github::TimelineItem;
use chrono::{DateTime, Utc};
use gpui_kit::assets::IconName;
use gpui_kit::component::text::TextView;
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, StyledExt as _, h_flex, v_flex};
use gpui_kit::{
    AnyElement, App, Div, ElementId, Hsla, IntoElement, ParentElement as _, SharedString,
    Styled as _, div, px,
};

use super::labels;
use crate::ui;

const CARD_AVATAR: f32 = 28.;
pub const EVENT_ICON_BOX: f32 = 24.;
const HUNK_TAIL_LINES: usize = 6;
const COMMENT_OPEN: &str = "<!--";
const COMMENT_CLOSE: &str = "-->";
const FENCES: [&str; 2] = ["```", "~~~"];
const INLINE_CODE: char = '`';

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
        | TimelineItem::ReadyForReview { at, .. }
        | TimelineItem::Assigned { at, .. }
        | TimelineItem::Unassigned { at, .. }
        | TimelineItem::Unlabeled { at, .. }
        | TimelineItem::Reopened { at, .. }
        | TimelineItem::CrossReferenced { at, .. }
        | TimelineItem::Milestoned { at, .. } => *at,
    }
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

pub fn strip_html_comments(body: &str) -> String {
    let mut out = String::with_capacity(body.len());
    let mut in_fence = false;
    let mut in_block = false;
    for line in body.split_inclusive('\n') {
        if in_block {
            if let Some(end) = line.find(COMMENT_CLOSE) {
                let (text, opened) = strip_line(&line[end + COMMENT_CLOSE.len()..]);
                out.push_str(&text);
                in_block = opened;
            }
            continue;
        }
        let fence = FENCES
            .iter()
            .any(|fence| line.trim_start().starts_with(fence));
        if fence {
            in_fence = !in_fence;
        }
        if fence || in_fence {
            out.push_str(line);
            continue;
        }
        let (text, opened) = strip_line(line);
        out.push_str(&text);
        in_block = opened;
    }
    out
}

fn strip_line(line: &str) -> (String, bool) {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    let mut in_code = false;
    loop {
        let next = rest.char_indices().find(|(ix, ch)| {
            *ch == INLINE_CODE || (!in_code && rest[*ix..].starts_with(COMMENT_OPEN))
        });
        let Some((ix, ch)) = next else {
            out.push_str(rest);
            return (out, false);
        };
        out.push_str(&rest[..ix]);
        if ch == INLINE_CODE {
            in_code = !in_code;
            out.push(ch);
            rest = &rest[ix + ch.len_utf8()..];
            continue;
        }
        let after = &rest[ix + COMMENT_OPEN.len()..];
        match after.find(COMMENT_CLOSE) {
            Some(end) => rest = &after[end + COMMENT_CLOSE.len()..],
            None if out.trim().is_empty() => return (out, true),
            None => {
                out.push_str(&rest[ix..]);
                return (out, false);
            }
        }
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

pub fn event(
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

pub fn status_pill(icon: Option<IconName>, label: &'static str, color: Hsla, text: Hsla) -> Div {
    h_flex()
        .flex_none()
        .gap_1()
        .px_1p5()
        .h(px(18.))
        .items_center()
        .rounded_full()
        .border_1()
        .border_color(color.opacity(0.5))
        .bg(color.opacity(0.12))
        .text_xs()
        .text_color(text)
        .children(icon.map(|icon| Icon::new(icon).xsmall()))
        .child(label)
}

pub fn review_pill(state: ReviewState, cx: &App) -> Div {
    let (icon, color) = review_style(state, cx);
    let text = if state == ReviewState::Commented {
        cx.theme().foreground
    } else {
        color
    };
    status_pill(Some(icon), labels::review_state(state), color, text)
}

pub fn file_line(path: &str, line: Option<u32>, cx: &App) -> Div {
    let theme = cx.theme();
    h_flex()
        .px_3()
        .py_1()
        .gap_1p5()
        .items_center()
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
                .child(labels::line_ref(path, line)),
        )
}

pub fn diff_hunk(hunk: &str, cx: &App) -> Div {
    let theme = cx.theme();
    let lines: Vec<&str> = hunk.lines().collect();
    let tail = lines.len().saturating_sub(HUNK_TAIL_LINES);
    v_flex()
        .py_1()
        .border_b_1()
        .border_color(theme.border)
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
                .px_3()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_comments_disappear_from_the_text() {
        assert_eq!(
            strip_html_comments("antes <!-- marcador --> depois"),
            "antes  depois"
        );
        assert_eq!(strip_html_comments("a<!--x-->b<!--y-->c"), "abc");
    }

    #[test]
    fn a_comment_spanning_lines_is_removed_whole() {
        assert_eq!(
            strip_html_comments("linha 1\n<!-- DIVIDER\ncontinua\n--> fim\nlinha 2"),
            "linha 1\n fim\nlinha 2"
        );
    }

    #[test]
    fn code_fences_keep_their_comments() {
        let body = "```html\n<!-- fica -->\n```\n<!-- sai -->texto";
        assert_eq!(
            strip_html_comments(body),
            "```html\n<!-- fica -->\n```\ntexto"
        );
    }

    #[test]
    fn an_unclosed_comment_opening_a_line_hides_the_rest() {
        assert_eq!(
            strip_html_comments("início\n<!-- sem fim\nmais"),
            "início\n"
        );
    }

    #[test]
    fn an_unclosed_marker_in_the_middle_of_a_line_stays_as_text() {
        let body = "visível <!-- sem fim\nmais";
        assert_eq!(strip_html_comments(body), body);
    }

    #[test]
    fn inline_code_keeps_its_markers() {
        let body = "remova o `<!--` da linha 3 e o `-->` da 9";
        assert_eq!(strip_html_comments(body), body);
    }

    #[test]
    fn text_without_comments_is_untouched() {
        let body = "# Título\n\ntexto com -> seta e <b>html</b>\n";
        assert_eq!(strip_html_comments(body), body);
    }
}
