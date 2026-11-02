use gpui_kit::assets::IconName;
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, StyledExt as _, v_flex};
use gpui_kit::{App, Div, ParentElement as _, Styled as _, div};

pub fn tab_placeholder(icon: IconName, title: &str, hint: &str, cx: &App) -> Div {
    let theme = cx.theme();
    v_flex()
        .size_full()
        .items_center()
        .justify_center()
        .gap_2()
        .text_color(theme.muted_foreground)
        .child(Icon::new(icon).large())
        .child(
            div()
                .text_base()
                .font_semibold()
                .text_color(theme.foreground)
                .child(title.to_string()),
        )
        .child(div().text_sm().child(hint.to_string()))
}
