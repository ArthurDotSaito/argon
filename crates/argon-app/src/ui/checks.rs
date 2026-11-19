use argon_core::ChecksState;
use gpui_kit::assets::IconName;
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, StyledExt as _, h_flex};
use gpui_kit::{AnyElement, App, Div, Hsla, IntoElement as _, ParentElement as _, Styled as _, px};

const CHECKS_OK: &str = "CI ok";
const CHECKS_FAILED: &str = "CI falhou";
const CHECKS_RUNNING: &str = "CI rodando";
const CHECKS_NONE: &str = "sem CI";
const LABEL_GAP: f32 = 4.;

fn checks_style(checks: Option<ChecksState>, cx: &App) -> (IconName, Hsla) {
    let theme = cx.theme();
    match checks {
        Some(ChecksState::Success) => (IconName::Check, theme.success),
        Some(ChecksState::Failure | ChecksState::Error) => (IconName::X, theme.danger),
        Some(ChecksState::Pending) => (IconName::CircleDashed, theme.warning),
        None => (IconName::Minus, theme.muted_foreground),
    }
}

pub(crate) fn checks_text(checks: Option<ChecksState>) -> &'static str {
    match checks {
        Some(ChecksState::Success) => CHECKS_OK,
        Some(ChecksState::Failure | ChecksState::Error) => CHECKS_FAILED,
        Some(ChecksState::Pending) => CHECKS_RUNNING,
        None => CHECKS_NONE,
    }
}

pub fn checks_glyph(checks: Option<ChecksState>, cx: &App) -> AnyElement {
    let (icon, color) = checks_style(checks, cx);
    Icon::new(icon)
        .xsmall()
        .text_color(color)
        .into_any_element()
}

pub fn checks_label(checks: Option<ChecksState>, cx: &App) -> Div {
    let (icon, color) = checks_style(checks, cx);
    h_flex()
        .gap(px(LABEL_GAP))
        .items_center()
        .text_xs()
        .font_medium()
        .text_color(color)
        .child(Icon::new(icon).xsmall().text_color(color))
        .child(checks_text(checks))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checks_text_covers_every_state() {
        assert_eq!(checks_text(Some(ChecksState::Success)), "CI ok");
        assert_eq!(checks_text(Some(ChecksState::Failure)), "CI falhou");
        assert_eq!(checks_text(Some(ChecksState::Error)), "CI falhou");
        assert_eq!(checks_text(Some(ChecksState::Pending)), "CI rodando");
        assert_eq!(checks_text(None), "sem CI");
    }
}
