use gpui_kit::component::{ActiveTheme as _, h_flex};
use gpui_kit::{App, Div, ParentElement as _, Styled as _, div};

pub fn diff_stat(additions: u32, deletions: u32, cx: &App) -> Div {
    let theme = cx.theme();
    h_flex()
        .gap_1p5()
        .font_family(theme.mono_font_family.clone())
        .text_xs()
        .child(
            div()
                .text_color(theme.success)
                .child(format!("+{additions}")),
        )
        .child(
            div()
                .text_color(theme.danger)
                .child(format!("−{deletions}")),
        )
}
