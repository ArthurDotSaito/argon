use gpui_kit::component::ActiveTheme as _;
use gpui_kit::{App, Hsla};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Action,
    Danger,
    Warning,
    Success,
    Neutral,
}

pub fn tone_color(tone: Tone, cx: &App) -> Hsla {
    let theme = cx.theme();
    match tone {
        Tone::Action => theme.primary,
        Tone::Danger => theme.danger,
        Tone::Warning => theme.warning,
        Tone::Success => theme.success,
        Tone::Neutral => theme.muted_foreground,
    }
}
