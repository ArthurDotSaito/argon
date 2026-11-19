use gpui_kit::component::{ActiveTheme as _, StyledExt as _, h_flex};
use gpui_kit::{
    App, Div, ElementId, InteractiveElement as _, ParentElement as _, Stateful, Styled as _, div,
    prelude::FluentBuilder as _, px,
};

use crate::theme::density;

pub const COLUMN_HEADER_HEIGHT: f32 = 30.;
pub const LIST_ROW_HEIGHT: f32 = 38.;
pub const GROUP_ROW_HEIGHT: f32 = 28.;
const ROW_BORDER_OPACITY: f32 = 0.45;
const ROW_PADDING_LEFT: f32 = 8.;
const ROW_SELECTION_BORDER: f32 = 2.;

pub fn column_header(cx: &App) -> Div {
    let theme = cx.theme();
    h_flex()
        .h(px(density::scaled(COLUMN_HEADER_HEIGHT, cx)))
        .flex_none()
        .w_full()
        .items_center()
        .gap_3()
        .pl(px(ROW_PADDING_LEFT + ROW_SELECTION_BORDER))
        .pr_3()
        .text_xs()
        .font_medium()
        .text_color(theme.muted_foreground)
        .border_b_1()
        .border_color(theme.border)
}

pub fn list_row(id: impl Into<ElementId>, selected: bool, cx: &App) -> Stateful<Div> {
    let theme = cx.theme();
    h_flex()
        .id(id)
        .h(px(density::scaled(LIST_ROW_HEIGHT, cx)))
        .flex_none()
        .w_full()
        .items_center()
        .gap_3()
        .pl(px(ROW_PADDING_LEFT))
        .pr_3()
        .overflow_hidden()
        .relative()
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
        .child(
            div()
                .absolute()
                .left_0()
                .right_0()
                .bottom_0()
                .h(px(1.))
                .bg(theme.border.opacity(ROW_BORDER_OPACITY)),
        )
}

pub fn group_row(cx: &App) -> Div {
    let theme = cx.theme();
    h_flex()
        .h(px(density::scaled(GROUP_ROW_HEIGHT, cx)))
        .flex_none()
        .w_full()
        .items_center()
        .gap_2()
        .px_3()
        .bg(theme.muted)
        .border_b_1()
        .border_color(theme.border)
        .text_xs()
        .font_semibold()
        .text_color(theme.foreground)
}
