use gpui_kit::component::{
    ActiveTheme as _, Icon, IconName, Sizable as _, StyledExt as _, TitleBar, h_flex,
};
use gpui_kit::{
    App, InteractiveElement as _, IntoElement, MouseButton, ParentElement as _, RenderOnce,
    SharedString, Styled as _, Window, div, px,
};

const APP_NAME: &str = "Argon";
const SEARCH_PLACEHOLDER: &str = "Buscar PRs, repos, comandos…";
const SEARCH_SHORTCUT: &str = "Ctrl K";
const SEARCH_WIDTH: f32 = 440.;
const SIDE_WIDTH: f32 = 320.;

#[derive(IntoElement)]
pub struct AppTitleBar {
    account: SharedString,
    sync_status: SharedString,
}

impl AppTitleBar {
    pub fn new(account: impl Into<SharedString>, sync_status: impl Into<SharedString>) -> Self {
        Self {
            account: account.into(),
            sync_status: sync_status.into(),
        }
    }
}

impl RenderOnce for AppTitleBar {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let identity = h_flex()
            .w(px(SIDE_WIDTH))
            .gap_2()
            .items_center()
            .child(
                div()
                    .font_semibold()
                    .text_color(theme.foreground)
                    .child(APP_NAME),
            )
            .child(div().text_color(theme.muted_foreground).child(self.account))
            .child(
                h_flex()
                    .gap_1()
                    .items_center()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(div().size(px(6.)).rounded_full().bg(theme.success))
                    .child(self.sync_status),
            );

        let search = h_flex()
            .id("title-search")
            .occlude()
            .w(px(SEARCH_WIDTH))
            .h(px(24.))
            .px_2()
            .gap_2()
            .items_center()
            .rounded(theme.radius)
            .border_1()
            .border_color(theme.border)
            .bg(theme.background)
            .text_color(theme.muted_foreground)
            .cursor_pointer()
            .hover(|this| this.text_color(theme.foreground))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(Icon::new(IconName::Search).xsmall())
            .child(div().flex_1().text_xs().child(SEARCH_PLACEHOLDER))
            .child(
                div()
                    .px_1()
                    .rounded(theme.radius)
                    .border_1()
                    .border_color(theme.border)
                    .text_xs()
                    .child(SEARCH_SHORTCUT),
            );

        TitleBar::new()
            .child(identity)
            .child(search)
            .child(div().w(px(SIDE_WIDTH)))
    }
}
