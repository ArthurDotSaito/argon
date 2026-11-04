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

fn is_https(url: &str) -> bool {
    url.get(..8)
        .is_some_and(|scheme| scheme.eq_ignore_ascii_case("https://"))
}

pub fn open_usage_url(url: &str) {
    if !is_https(url) {
        tracing::warn!(url, "refused to open a non-https usage url");
        return;
    }
    if let Err(error) = open::that_detached(url) {
        tracing::warn!(%error, url, "failed to open the usage page in the browser");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_https_urls_are_opened() {
        assert!(is_https("https://commandcode.ai/usage"));
        assert!(is_https("HTTPS://example.com"));
        assert!(!is_https("http://example.com"));
        assert!(!is_https("file:///C:/Windows/System32/calc.exe"));
        assert!(!is_https("javascript:alert(1)"));
        assert!(!is_https("https:/"));
        assert!(!is_https(""));
    }
}
