use gpui_kit::component::{
    ActiveTheme as _, Icon, IconName, Sizable as _, StyledExt as _, TitleBar, h_flex,
};
use gpui_kit::{
    App, Hsla, InteractiveElement as _, IntoElement, MouseButton, ParentElement as _, RenderOnce,
    SharedString, StatefulInteractiveElement as _, Styled as _, Window, div, px,
};

use crate::actions::OpenPalette;
use crate::services::AppServices;
use crate::sync_status::{SyncIndicator, SyncTone};

const APP_NAME: &str = "Argon";
const SEARCH_PLACEHOLDER: &str = "Buscar PRs, repos, comandos…";
const SEARCH_SHORTCUT: &str = "Ctrl K";
const SEARCH_WIDTH: f32 = 440.;
const SIDE_WIDTH: f32 = 320.;

#[derive(IntoElement)]
pub struct AppTitleBar {
    account: Option<SharedString>,
    sync: SyncIndicator,
}

impl AppTitleBar {
    pub fn new(account: Option<SharedString>, sync: SyncIndicator) -> Self {
        Self { account, sync }
    }
}

fn tone_color(tone: SyncTone, cx: &App) -> Hsla {
    let theme = cx.theme();
    match tone {
        SyncTone::Neutral => theme.muted_foreground,
        SyncTone::Busy => theme.warning,
        SyncTone::Success => theme.success,
        SyncTone::Danger => theme.danger,
    }
}

pub fn request_sync(cx: &mut App) {
    if let Some(services) = cx.try_global::<AppServices>() {
        services.0.sync_now();
    }
}

impl RenderOnce for AppTitleBar {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let dot = tone_color(self.sync.tone, cx);
        let theme = cx.theme();
        let sync = h_flex()
            .id("title-sync")
            .occlude()
            .gap_1()
            .items_center()
            .text_xs()
            .text_color(theme.muted_foreground)
            .cursor_pointer()
            .hover(|this| this.text_color(theme.foreground))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(|_, _, cx| request_sync(cx))
            .child(div().size(px(6.)).rounded_full().bg(dot))
            .child(self.sync.label);
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
            .children(
                self.account
                    .map(|account| div().text_color(theme.muted_foreground).child(account)),
            )
            .child(sync);

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
            .on_click(|_, window, cx| window.dispatch_action(Box::new(OpenPalette), cx))
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
