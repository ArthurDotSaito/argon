use gpui_kit::component::StyledExt as _;
use gpui_kit::{Div, Hsla, ParentElement as _, Styled as _, div, hsla, px, white};

const AVATAR_SATURATION: f32 = 0.45;
const AVATAR_LIGHTNESS: f32 = 0.42;

pub fn avatar(login: &str, size: f32) -> Div {
    div()
        .size(px(size))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .bg(avatar_color(login))
        .text_color(white())
        .text_size(px(size * 0.45))
        .font_semibold()
        .child(initials(login))
}

fn initials(name: &str) -> String {
    let parts: Vec<&str> = name
        .split(['-', '_', '.', ' '])
        .filter(|part| !part.is_empty())
        .collect();
    let picked: String = match parts.as_slice() {
        [first, second, ..] => first
            .chars()
            .take(1)
            .chain(second.chars().take(1))
            .collect(),
        _ => name
            .chars()
            .filter(|c| c.is_alphanumeric())
            .take(2)
            .collect(),
    };
    picked.to_uppercase()
}

fn avatar_color(name: &str) -> Hsla {
    let hash = name
        .to_lowercase()
        .bytes()
        .fold(0x811c_9dc5_u32, |hash, byte| {
            (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193)
        });
    let hue = (hash % 360) as f32 / 360.;
    hsla(hue, AVATAR_SATURATION, AVATAR_LIGHTNESS, 1.)
}
