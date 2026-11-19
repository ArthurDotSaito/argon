use gpui_kit::component::{ActiveTheme as _, h_flex, v_flex};
use gpui_kit::{App, Div, IntoElement, ParentElement as _, Styled as _, div, px};

pub const PANEL_WIDTH: f32 = 288.;
pub const LABEL_WIDTH: f32 = 78.;
const PANEL_PADDING: f32 = 14.;
const GROUP_GAP: f32 = 10.;
const ROW_GAP: f32 = 10.;
const VALUE_GAP: f32 = 6.;
const LABEL_TOP_OFFSET: f32 = 2.;

pub fn panel(cx: &App) -> Div {
    let theme = cx.theme();
    v_flex()
        .w(px(PANEL_WIDTH))
        .flex_none()
        .h_full()
        .border_l_1()
        .border_color(theme.border)
        .bg(theme.muted)
        .p(px(PANEL_PADDING))
        .gap(px(PANEL_PADDING))
        .overflow_hidden()
}

pub fn group(cx: &App) -> Div {
    let theme = cx.theme();
    v_flex()
        .gap(px(GROUP_GAP))
        .pt(px(PANEL_PADDING))
        .border_t_1()
        .border_color(theme.border)
}

pub fn row(label: &'static str, value: impl IntoElement, cx: &App) -> Div {
    let theme = cx.theme();
    h_flex()
        .items_start()
        .gap(px(ROW_GAP))
        .min_w_0()
        .child(
            div()
                .w(px(LABEL_WIDTH))
                .flex_none()
                .pt(px(LABEL_TOP_OFFSET))
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(label),
        )
        .child(
            h_flex()
                .flex_1()
                .min_w_0()
                .flex_wrap()
                .items_center()
                .gap(px(VALUE_GAP))
                .child(value),
        )
}
