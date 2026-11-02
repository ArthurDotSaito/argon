use gpui_kit::component::{ActiveTheme as _, TitleBar};
use gpui_kit::{Context, IntoElement, ParentElement as _, Render, Styled as _, Window, div, px};

pub struct Shell;

impl Render for Shell {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(theme.background)
            .text_color(theme.foreground)
            .child(
                TitleBar::new().child(
                    div()
                        .text_size(px(13.))
                        .text_color(theme.foreground)
                        .child("Argon"),
                ),
            )
            .child(div().flex_1())
    }
}
