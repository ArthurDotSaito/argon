use std::borrow::Cow;

use gpui_kit::App;

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

pub const FALLBACK_UI_FONT: &str = "Segoe UI";
pub const FALLBACK_MONO_FONT: &str = "Cascadia Mono";

pub fn register(cx: &mut App) -> bool {
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
