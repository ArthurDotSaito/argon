use std::borrow::Cow;
use std::rc::Rc;

use gpui_kit::App;
use gpui_kit::component::theme::{Theme, ThemeConfig, ThemeMode, ThemeSet};

const ONE_DARK: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/themes/one-dark.json"
));

const EMBEDDED_FONTS: [&[u8]; 6] = [
    include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/fonts/IBMPlexSans-Regular.ttf"
    )),
    include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/fonts/IBMPlexSans-Italic.ttf"
    )),
    include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/fonts/IBMPlexSans-Medium.ttf"
    )),
    include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/fonts/IBMPlexSans-SemiBold.ttf"
    )),
    include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/fonts/Lilex-Regular.ttf"
    )),
    include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/fonts/Lilex-Medium.ttf"
    )),
];

const FALLBACK_UI_FONT: &str = "Segoe UI";
const FALLBACK_MONO_FONT: &str = "Cascadia Mono";

pub fn apply_one_dark(cx: &mut App) {
    let fonts_loaded = register_fonts(cx);
    Theme::change(ThemeMode::Dark, None, cx);
    let Some(config) = one_dark_config() else {
        return;
    };
    Theme::update(cx, |theme| {
        theme.apply_config(&Rc::new(config));
        if !fonts_loaded {
            theme.font_family = FALLBACK_UI_FONT.into();
            theme.mono_font_family = FALLBACK_MONO_FONT.into();
        }
    });
}

fn register_fonts(cx: &mut App) -> bool {
    let fonts = EMBEDDED_FONTS
        .iter()
        .map(|bytes| Cow::Borrowed(*bytes))
        .collect();
    match cx.text_system().add_fonts(fonts) {
        Ok(()) => true,
        Err(error) => {
            eprintln!("failed to register embedded fonts: {error:#}");
            false
        }
    }
}

fn one_dark_config() -> Option<ThemeConfig> {
    match serde_json::from_str::<ThemeSet>(ONE_DARK) {
        Ok(set) => set.themes.into_iter().next(),
        Err(error) => {
            eprintln!("failed to parse one-dark theme: {error}");
            None
        }
    }
}
