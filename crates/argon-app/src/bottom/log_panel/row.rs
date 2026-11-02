use std::ops::Range;

use gpui_kit::assets::IconName;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, StyledExt as _, h_flex};
use gpui_kit::{
    AnyElement, ClickEvent, Context, HighlightStyle, InteractiveElement as _, IntoElement,
    ListHorizontalSizingBehavior, ParentElement as _, StatefulInteractiveElement as _, Styled as _,
    StyledText, div, px, uniform_list,
};

use super::{LogPanel, centered};
use crate::bottom::labels;
use crate::bottom::log_lines::{LineKind, find_matches};
use crate::bottom::log_view::LogView;

const ROW_HEIGHT: f32 = 18.;
const GUTTER_WIDTH: f32 = 52.;
const GROUP_INDENT: f32 = 14.;

impl LogPanel {
    pub(super) fn lines(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(view) = self.view.as_ref().filter(|view| view.row_count() > 0) else {
            return centered()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(labels::EMPTY_LOG)
                .into_any_element();
        };
        let theme = cx.theme();
        div()
            .relative()
            .flex_1()
            .min_h_0()
            .font_family(theme.mono_font_family.clone())
            .text_xs()
            .child(
                uniform_list(
                    "job-log-lines",
                    view.row_count(),
                    cx.processor(|this, range: Range<usize>, _, cx| this.rows(range, cx)),
                )
                .track_scroll(&self.scroll)
                .with_horizontal_sizing_behavior(ListHorizontalSizingBehavior::Unconstrained)
                .with_width_from_item(view.widest_row())
                .size_full(),
            )
            .vertical_scrollbar(&self.scroll)
            .into_any_element()
    }

    fn rows(&self, range: Range<usize>, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let Some(view) = self.view.as_ref() else {
            return Vec::new();
        };
        range
            .filter_map(|row| Some((row, view.line_at(row)?)))
            .map(|(row, line)| self.row(view, row, line, cx))
            .collect()
    }

    fn row(
        &self,
        view: &LogView,
        row: usize,
        line_ix: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme();
        let line = &view.doc().lines[line_ix];
        let highlights: Vec<(Range<usize>, HighlightStyle)> = if view.is_hit(line_ix) {
            let background = if view.current_line() == Some(line_ix) {
                theme.warning.opacity(0.7)
            } else {
                theme.warning.opacity(0.28)
            };
            find_matches(&line.text, view.query())
                .into_iter()
                .map(|range| {
                    (
                        range,
                        HighlightStyle {
                            background_color: Some(background),
                            ..HighlightStyle::default()
                        },
                    )
                })
                .collect()
        } else {
            Vec::new()
        };
        let color = match line.kind {
            LineKind::Error => theme.danger,
            LineKind::Warning => theme.warning,
            LineKind::Notice | LineKind::Command => theme.info,
            LineKind::Debug => theme.muted_foreground,
            LineKind::GroupStart | LineKind::GroupEnd | LineKind::Plain => theme.foreground,
        };
        let text = StyledText::new(line.text.clone()).with_highlights(highlights);
        let gutter = div()
            .flex_none()
            .w(px(GUTTER_WIDTH))
            .pr_2()
            .text_right()
            .text_color(theme.muted_foreground.opacity(0.7))
            .child((line_ix + 1).to_string());
        let base = h_flex()
            .id(("log-row", row))
            .h(px(ROW_HEIGHT))
            .min_w_full()
            .items_center()
            .whitespace_nowrap()
            .child(gutter);
        let base = match line.kind {
            LineKind::Error => base.bg(theme.danger.opacity(0.1)),
            LineKind::Warning => base.bg(theme.warning.opacity(0.08)),
            _ => base,
        };
        if line.kind == LineKind::GroupStart {
            let chevron = if view.is_collapsed(line_ix) {
                IconName::ChevronRight
            } else {
                IconName::ChevronDown
            };
            return base
                .cursor_pointer()
                .hover(|style| style.bg(theme.muted.opacity(0.5)))
                .child(
                    Icon::new(chevron)
                        .xsmall()
                        .text_color(theme.muted_foreground),
                )
                .child(div().pl_1().font_semibold().text_color(color).child(text))
                .on_click(
                    cx.listener(move |this, _: &ClickEvent, _, cx| this.toggle_group(line_ix, cx)),
                )
                .into_any_element();
        }
        let indent = if line.group.is_some() {
            GROUP_INDENT
        } else {
            0.
        };
        base.child(div().pl(px(indent)).text_color(color).child(text))
            .into_any_element()
    }
}
