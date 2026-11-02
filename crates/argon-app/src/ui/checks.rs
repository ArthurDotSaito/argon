use argon_core::ChecksState;
use gpui_kit::assets::IconName;
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _};
use gpui_kit::{AnyElement, App, IntoElement as _, Styled as _};

pub fn checks_glyph(checks: Option<ChecksState>, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let (icon, color) = match checks {
        Some(ChecksState::Success) => (IconName::Check, theme.success),
        Some(ChecksState::Failure | ChecksState::Error) => (IconName::X, theme.danger),
        Some(ChecksState::Pending) => (IconName::CircleDashed, theme.warning),
        None => (IconName::Minus, theme.muted_foreground),
    };
    Icon::new(icon)
        .xsmall()
        .text_color(color)
        .into_any_element()
}
