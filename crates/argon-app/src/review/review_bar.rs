use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Icon, Sizable as _, StyledExt as _, h_flex, v_flex,
};
use gpui_kit::{App, ClickEvent, Div, ParentElement as _, Styled as _, Window, div};

use super::labels;

pub struct BarProps {
    pub count: usize,
    pub in_progress: bool,
    pub head_moved: bool,
    pub sending: bool,
}

pub fn review_bar(
    props: BarProps,
    on_finish: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    cx: &App,
) -> Div {
    let theme = cx.theme();
    let warning = props.head_moved.then(|| {
        h_flex()
            .w_full()
            .px_4()
            .py_1p5()
            .gap_2()
            .items_center()
            .bg(theme.warning.opacity(0.08))
            .border_b_1()
            .border_color(theme.warning.opacity(0.4))
            .text_xs()
            .text_color(theme.foreground)
            .child(
                Icon::new(IconName::TriangleAlert)
                    .xsmall()
                    .text_color(theme.warning),
            )
            .child(labels::HEAD_MOVED)
    });
    let (summary, button) = if props.count == 0 {
        (
            labels::NO_PENDING_COMMENTS.to_string(),
            labels::START_REVIEW,
        )
    } else {
        (labels::pending_count(props.count), labels::FINISH_REVIEW)
    };
    let progress = props.in_progress.then(|| {
        h_flex()
            .gap_2()
            .child(
                div()
                    .font_semibold()
                    .text_color(theme.foreground)
                    .child(labels::REVIEW_IN_PROGRESS),
            )
            .child(
                div()
                    .text_color(theme.muted_foreground)
                    .child(labels::SEPARATOR),
            )
    });
    let icon_color = if props.in_progress {
        theme.warning
    } else {
        theme.muted_foreground
    };
    let status = h_flex()
        .w_full()
        .h_10()
        .px_4()
        .gap_2()
        .items_center()
        .text_sm()
        .child(
            Icon::new(IconName::MessageSquare)
                .small()
                .text_color(icon_color),
        )
        .children(progress)
        .child(
            div()
                .min_w_0()
                .truncate()
                .text_color(theme.muted_foreground)
                .child(summary),
        )
        .child(div().flex_1())
        .child(
            Button::new("review-finish")
                .label(button)
                .small()
                .primary()
                .loading(props.sending)
                .disabled(props.sending)
                .on_click(on_finish),
        );
    v_flex()
        .w_full()
        .flex_none()
        .border_t_1()
        .border_color(theme.border)
        .bg(theme.secondary)
        .children(warning)
        .child(status)
}
