use gpui_kit::component::{ActiveTheme as _, StyledExt as _, h_flex, v_flex};
use gpui_kit::{
    App, Div, Hsla, IntoElement, ParentElement as _, SharedString, Styled as _, div, px,
};

use crate::services::{AppServices, Services};

const LABEL_WIDTH: f32 = 132.;

pub fn services(cx: &App) -> Option<Services> {
    cx.try_global::<AppServices>()
        .map(|global| global.0.clone())
}

pub fn section(title: &'static str, hint: &'static str, cx: &App) -> Div {
    let theme = cx.theme();
    v_flex().w_full().gap_3().child(
        v_flex()
            .gap_0p5()
            .child(
                div()
                    .text_lg()
                    .font_semibold()
                    .text_color(theme.foreground)
                    .child(title),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child(hint),
            ),
    )
}

pub fn card(cx: &App) -> Div {
    let theme = cx.theme();
    v_flex()
        .w_full()
        .p_3()
        .gap_2()
        .rounded(theme.radius)
        .border_1()
        .border_color(theme.border)
        .bg(theme.background)
}

pub fn field(label: &'static str, control: impl IntoElement, cx: &App) -> Div {
    h_flex()
        .w_full()
        .gap_3()
        .items_center()
        .child(
            div()
                .flex_none()
                .w(px(LABEL_WIDTH))
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(label),
        )
        .child(div().flex_1().min_w_0().child(control))
}

pub fn indented(child: impl IntoElement) -> Div {
    h_flex().w_full().pl(px(LABEL_WIDTH + 12.)).child(child)
}

pub fn note(text: impl Into<SharedString>, color: Hsla) -> Div {
    div().text_xs().text_color(color).child(text.into())
}

pub fn mono(text: impl Into<SharedString>, cx: &App) -> Div {
    div()
        .min_w_0()
        .truncate()
        .font_family(cx.theme().mono_font_family.clone())
        .text_xs()
        .child(text.into())
}

pub fn badge(text: &'static str, color: Hsla) -> Div {
    div()
        .flex_none()
        .px_1p5()
        .rounded_sm()
        .border_1()
        .border_color(color.opacity(0.5))
        .text_xs()
        .text_color(color)
        .child(text)
}
