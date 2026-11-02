use std::f32::consts::PI;

use argon_core::{DiffLine, LineKind, Side, anchor};
use gpui_kit::assets::IconName;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, h_flex};
use gpui_kit::{
    Animation, AnimationExt as _, AnyElement, App, Div, ElementId, Entity, Hsla,
    InteractiveElement as _, IntoElement, MouseButton, MouseDownEvent, MouseUpEvent,
    ParentElement as _, StatefulInteractiveElement as _, Styled as _, StyledText, div, px,
    relative,
};

use super::{DiffMode, DiffView, FLASH_DURATION};
use crate::review::file_diff::{FileDiff, ParsedDiff};
use crate::review::labels;

pub(super) const ROW_HEIGHT: f32 = 20.;
pub(super) const NUMBER_WIDTH: f32 = 48.;
const SIGN_WIDTH: f32 = 16.;
const CHANGE_ALPHA: f32 = 0.13;
const EMPTY_ALPHA: f32 = 0.6;
const MARK_ALPHA: f32 = 0.1;
const FLASH_ALPHA: f32 = 0.45;
const FLASH_PULSES: f32 = 3.;

pub(super) struct RowCx<'a> {
    pub view: &'a Entity<DiffView>,
    pub this: &'a DiffView,
    pub file_ix: usize,
    pub file: &'a FileDiff,
    pub diff: &'a ParsedDiff,
}

pub(super) fn render_row(
    rcx: &RowCx<'_>,
    hunk: usize,
    row: usize,
    marked: bool,
    cx: &App,
) -> AnyElement {
    let body = match rcx.this.mode {
        DiffMode::Inline => inline_row(rcx, &rcx.diff.hunks[hunk].lines[row], cx),
        DiffMode::Split => {
            let pair = &rcx.diff.split[hunk][row];
            h_flex()
                .w_full()
                .child(split_half(rcx, pair.left.as_ref(), Side::Left, cx))
                .child(div().w(px(1.)).h_full().flex_none().bg(cx.theme().border))
                .child(split_half(rcx, pair.right.as_ref(), Side::Right, cx))
        }
    };
    let body = if marked {
        body.child(overlay(cx.theme().primary.opacity(MARK_ALPHA)))
    } else {
        body
    };
    body.h(px(ROW_HEIGHT)).into_any_element()
}

fn inline_row(rcx: &RowCx<'_>, line: &DiffLine, cx: &App) -> Div {
    let target = anchor(line);
    let numbers = h_flex()
        .child(number(line.old_no))
        .child(number(line.new_no));
    let side = match line.kind {
        LineKind::Removed => Side::Left,
        LineKind::Added | LineKind::Context => Side::Right,
    };
    cell(rcx, line, target, side, numbers, cx)
}

fn split_half(rcx: &RowCx<'_>, line: Option<&DiffLine>, side: Side, cx: &App) -> Div {
    let Some(line) = line else {
        return div()
            .flex_1()
            .min_w_0()
            .h_full()
            .bg(cx.theme().muted.opacity(EMPTY_ALPHA));
    };
    let number_value = match side {
        Side::Left => line.old_no,
        Side::Right => line.new_no,
    };
    let target = (number_value.unwrap_or_default(), side);
    cell(rcx, line, target, side, number(number_value), cx)
        .flex_1()
        .min_w_0()
}

fn cell(
    rcx: &RowCx<'_>,
    line: &DiffLine,
    target: (u32, Side),
    side: Side,
    numbers: Div,
    cx: &App,
) -> Div {
    let theme = cx.theme();
    let (sign, tint) = match line.kind {
        LineKind::Added => ("+", Some(theme.success)),
        LineKind::Removed => ("−", Some(theme.danger)),
        LineKind::Context => (" ", None),
    };
    let selected = rcx.this.is_selected(rcx.file_ix, target);
    let gutter = gutter(rcx, target, numbers, selected, cx);
    let code = code(rcx, line, side, cx);
    let row = h_flex()
        .relative()
        .w_full()
        .h_full()
        .items_center()
        .font_family(theme.mono_font_family.clone())
        .text_xs()
        .child(gutter)
        .child(
            div()
                .w(px(SIGN_WIDTH))
                .flex_none()
                .text_center()
                .text_color(tint.unwrap_or(theme.muted_foreground))
                .child(sign),
        )
        .child(code);
    let row = match tint {
        Some(color) => row.bg(color.opacity(CHANGE_ALPHA)),
        None => row,
    };
    let row = if selected {
        row.child(overlay(theme.selection.opacity(0.5)))
    } else {
        row
    };
    match rcx.this.flash_seq(rcx.file_ix, target) {
        Some(seq) => row.child(flash(seq, theme.warning)),
        None => row,
    }
}

fn gutter(rcx: &RowCx<'_>, target: (u32, Side), numbers: Div, selected: bool, cx: &App) -> Div {
    let theme = cx.theme();
    let file = rcx.file_ix;
    let down = rcx.view.clone();
    let up = rcx.view.clone();
    numbers
        .flex_none()
        .h_full()
        .items_center()
        .cursor_pointer()
        .text_color(if selected {
            theme.primary
        } else {
            theme.muted_foreground.opacity(0.8)
        })
        .hover(|style| style.bg(theme.primary.opacity(0.18)))
        .on_mouse_down(MouseButton::Left, move |event: &MouseDownEvent, _, cx| {
            down.update(cx, |this, cx| {
                this.gutter_down(file, target, event.modifiers.shift, cx)
            });
        })
        .on_mouse_up(MouseButton::Left, move |_: &MouseUpEvent, _, cx| {
            up.update(cx, |this, cx| this.gutter_up(file, target, cx));
        })
}

fn number(value: Option<u32>) -> Div {
    div()
        .w(px(NUMBER_WIDTH))
        .flex_none()
        .pr_2()
        .text_right()
        .children(value.map(|value| value.to_string()))
}

fn code(rcx: &RowCx<'_>, line: &DiffLine, side: Side, cx: &App) -> Div {
    let theme = cx.theme();
    let number = match side {
        Side::Left => line.old_no,
        Side::Right => line.new_no,
    };
    let spans = rcx
        .file
        .highlights
        .as_ref()
        .zip(number)
        .and_then(|(highlights, number)| highlights.spans(side, number))
        .cloned()
        .unwrap_or_default();
    let text = StyledText::new(line.text.clone()).with_highlights(spans);
    let content = h_flex()
        .flex_1()
        .min_w_0()
        .overflow_hidden()
        .whitespace_nowrap()
        .text_color(theme.foreground)
        .child(text);
    if line.no_newline {
        content.child(no_newline_marker(cx))
    } else {
        content
    }
}

fn no_newline_marker(cx: &App) -> AnyElement {
    let theme = cx.theme();
    div()
        .id(ElementId::Name(labels::NO_NEWLINE.into()))
        .relative()
        .flex_none()
        .ml_1()
        .text_color(theme.muted_foreground)
        .child(Icon::new(IconName::CornerDownLeft).xsmall())
        .child(
            div()
                .absolute()
                .left_0()
                .right_0()
                .top(relative(0.5))
                .h(px(1.))
                .bg(theme.muted_foreground),
        )
        .tooltip(|window, cx| Tooltip::new(labels::NO_NEWLINE).build(window, cx))
        .into_any_element()
}

fn overlay(color: Hsla) -> Div {
    div().absolute().inset_0().bg(color)
}

fn flash(seq: u64, color: Hsla) -> AnyElement {
    overlay(color.opacity(FLASH_ALPHA))
        .with_animation(
            ElementId::NamedInteger("diff-flash".into(), seq),
            Animation::new(FLASH_DURATION),
            |element, delta| {
                let pulse = 0.6 + 0.4 * (delta * PI * 2. * FLASH_PULSES).cos();
                element.opacity(pulse * (1. - delta))
            },
        )
        .into_any_element()
}
