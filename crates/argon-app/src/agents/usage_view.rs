use gpui_kit::component::ActiveTheme as _;
use gpui_kit::{App, Hsla};

use super::usage_format::Tone;

pub fn tone_color(tone: Tone, cx: &App) -> Hsla {
    let theme = cx.theme();
    match tone {
        Tone::Muted => theme.muted_foreground,
        Tone::Warning => theme.warning,
        Tone::Danger => theme.danger,
    }
}

pub fn open_usage_url(url: &str) {
    if let Err(error) = open::that_detached(url) {
        tracing::warn!(%error, url, "failed to open the usage page in the browser");
    }
}
