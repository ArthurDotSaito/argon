pub mod density;
mod fonts;

use std::rc::Rc;

use argon_core::ThemeChoice;
use gpui_kit::component::theme::{Theme, ThemeConfig, ThemeMode, ThemeSet};
use gpui_kit::{App, Window, WindowAppearance};

use crate::actions::ToggleTheme;
use crate::config_watch::{self, ConfigChanged};

pub const BODY_TEXT_SIZE: f32 = 14.;

const ONE_DARK: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/themes/one-dark.json"
));
const ONE_LIGHT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/themes/one-light.json"
));

pub fn init(cx: &mut App) {
    install_themes(cx);
    let appearance = config_watch::current(cx).appearance;
    apply_theme(appearance.theme, cx.window_appearance(), cx);
    density::set(appearance.density, cx);
    cx.on_action(|_: &ToggleTheme, cx| toggle_theme(cx));
    let watch = config_watch::entity(cx);
    cx.subscribe(&watch, |_, event: &ConfigChanged, cx| {
        let appearance = event.0.appearance;
        apply_theme(appearance.theme, cx.window_appearance(), cx);
        density::set(appearance.density, cx);
    })
    .detach();
}

pub fn follow_system_appearance(window: &mut Window) {
    window
        .observe_window_appearance(|window, cx| {
            let choice = config_watch::current(cx).appearance.theme;
            if choice == ThemeChoice::System {
                apply_theme(choice, window.appearance(), cx);
            }
        })
        .detach();
}

pub fn apply_theme(choice: ThemeChoice, window_appearance: WindowAppearance, cx: &mut App) {
    Theme::change(resolve_mode(choice, window_appearance), None, cx);
}

fn resolve_mode(choice: ThemeChoice, window_appearance: WindowAppearance) -> ThemeMode {
    match choice {
        ThemeChoice::OneDark => ThemeMode::Dark,
        ThemeChoice::OneLight => ThemeMode::Light,
        ThemeChoice::System => ThemeMode::from(window_appearance),
    }
}

fn toggle_theme(cx: &mut App) {
    let current = config_watch::current(cx).appearance.theme;
    let next = if resolve_mode(current, cx.window_appearance()).is_dark() {
        ThemeChoice::OneLight
    } else {
        ThemeChoice::OneDark
    };
    config_watch::update(cx, |config| config.appearance.theme = next);
}

fn install_themes(cx: &mut App) {
    let fonts_loaded = fonts::register(cx);
    for (name, source) in [("one-dark", ONE_DARK), ("one-light", ONE_LIGHT)] {
        let Some(mut config) = parse_theme(name, source) else {
            continue;
        };
        if !fonts_loaded {
            config.font_family = Some(fonts::FALLBACK_UI_FONT.into());
            config.mono_font_family = Some(fonts::FALLBACK_MONO_FONT.into());
        }
        Theme::update(cx, |theme| theme.apply_config(&Rc::new(config)));
    }
}

fn parse_theme(name: &str, source: &str) -> Option<ThemeConfig> {
    match serde_json::from_str::<ThemeSet>(source) {
        Ok(set) => set.themes.into_iter().next(),
        Err(error) => {
            eprintln!("failed to parse {name} theme: {error}");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_choices_ignore_the_window_appearance() {
        assert!(resolve_mode(ThemeChoice::OneDark, WindowAppearance::Light).is_dark());
        assert!(!resolve_mode(ThemeChoice::OneLight, WindowAppearance::Dark).is_dark());
    }

    #[test]
    fn system_follows_the_window_appearance() {
        assert!(resolve_mode(ThemeChoice::System, WindowAppearance::Dark).is_dark());
        assert!(resolve_mode(ThemeChoice::System, WindowAppearance::VibrantDark).is_dark());
        assert!(!resolve_mode(ThemeChoice::System, WindowAppearance::Light).is_dark());
    }

    #[test]
    fn both_embedded_themes_parse_with_their_modes() {
        let dark = parse_theme("one-dark", ONE_DARK).expect("one dark parses");
        let light = parse_theme("one-light", ONE_LIGHT).expect("one light parses");
        assert!(dark.mode.is_dark());
        assert!(!light.mode.is_dark());
    }
}
