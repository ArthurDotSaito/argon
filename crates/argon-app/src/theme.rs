use gpui_kit::component::theme::{Theme, ThemeMode};
use gpui_kit::{App, Hsla, rgb};

pub const BACKGROUND: u32 = 0x282c33;
pub const SURFACE: u32 = 0x2f343e;
pub const BORDER: u32 = 0x464b57;
pub const TEXT: u32 = 0xdce0e5;
pub const MUTED: u32 = 0xa9afbc;
pub const ACCENT: u32 = 0x74ade8;

fn color(hex: u32) -> Hsla {
    rgb(hex).into()
}

pub fn apply_one_dark(cx: &mut App) {
    Theme::change(ThemeMode::Dark, None, cx);
    Theme::update(cx, |theme| {
        theme.background = color(BACKGROUND);
        theme.foreground = color(TEXT);
        theme.border = color(BORDER);
        theme.muted = color(SURFACE);
        theme.muted_foreground = color(MUTED);
        theme.primary = color(ACCENT);
        theme.title_bar = color(SURFACE);
        theme.title_bar_border = color(BORDER);
    });
}
