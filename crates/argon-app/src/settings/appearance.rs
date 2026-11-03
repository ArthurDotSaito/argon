use argon_core::{Density, ThemeChoice};
use gpui_kit::component::radio::{Radio, RadioGroup};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{Sizable as _, v_flex};
use gpui_kit::{App, Div, ParentElement as _, Styled as _};

use super::labels;
use super::ui::{card, field, section};
use crate::config_watch;

const THEMES: [(ThemeChoice, &str); 3] = [
    (ThemeChoice::OneDark, labels::ONE_DARK),
    (ThemeChoice::OneLight, labels::ONE_LIGHT),
    (ThemeChoice::System, labels::SYSTEM),
];

const DENSITIES: [(Density, &str); 2] = [
    (Density::Compact, labels::COMPACT),
    (Density::Comfortable, labels::COMFORTABLE),
];

fn choice_group<T: Copy + PartialEq + 'static>(
    id: &'static str,
    options: &'static [(T, &'static str)],
    current: T,
    apply: fn(&mut argon_core::Config, T),
) -> RadioGroup {
    RadioGroup::horizontal(id)
        .selected_index(options.iter().position(|(value, _)| *value == current))
        .children(
            options
                .iter()
                .enumerate()
                .map(|(ix, (_, label))| Radio::new((id, ix)).label(*label)),
        )
        .on_click(move |ix, _, cx| {
            if let Some((value, _)) = options.get(*ix) {
                let value = *value;
                config_watch::update(cx, |config| apply(config, value));
            }
        })
}

pub fn appearance(cx: &App) -> Div {
    let current = config_watch::current(cx).appearance;
    let theme = choice_group("settings-theme", &THEMES, current.theme, |config, value| {
        config.appearance.theme = value
    });
    let density = choice_group(
        "settings-density",
        &DENSITIES,
        current.density,
        |config, value| config.appearance.density = value,
    );
    section(labels::APPEARANCE, labels::APPEARANCE_HINT, cx).child(
        card(cx)
            .child(field(labels::THEME, theme, cx))
            .child(field(labels::DENSITY, density, cx)),
    )
}

pub fn notifications(cx: &App) -> Div {
    let current = config_watch::current(cx).notifications;
    let needs_you = Switch::new("settings-notify-needs-you")
        .checked(current.needs_you)
        .small()
        .label(labels::NOTIFY_NEEDS_YOU)
        .on_click(|on, _, cx| {
            let on = *on;
            config_watch::update(cx, |config| config.notifications.needs_you = on);
        });
    let agents = Switch::new("settings-notify-agents")
        .checked(current.agent_events)
        .small()
        .label(labels::NOTIFY_AGENTS)
        .on_click(|on, _, cx| {
            let on = *on;
            config_watch::update(cx, |config| config.notifications.agent_events = on);
        });
    section(labels::NOTIFICATIONS, labels::NOTIFICATIONS_HINT, cx)
        .child(card(cx).child(v_flex().gap_2().child(needs_you).child(agents)))
}
