use argon_core::Config;
use gpui_kit::{App, AppContext as _, Entity, EventEmitter, Global};

use crate::services::AppServices;

#[derive(Clone)]
pub struct ConfigChanged(pub Config);

pub struct ConfigWatch;

impl EventEmitter<ConfigChanged> for ConfigWatch {}

struct ConfigWatchGlobal(Entity<ConfigWatch>);

impl Global for ConfigWatchGlobal {}

pub fn init(cx: &mut App) {
    let entity = cx.new(|_| ConfigWatch);
    cx.set_global(ConfigWatchGlobal(entity));
}

pub fn entity(cx: &App) -> Entity<ConfigWatch> {
    cx.global::<ConfigWatchGlobal>().0.clone()
}

pub fn current(cx: &App) -> Config {
    cx.global::<AppServices>().0.config()
}

pub fn update(cx: &mut App, f: impl FnOnce(&mut Config)) -> Config {
    let config = cx.global::<AppServices>().0.update_config(f);
    let changed = ConfigChanged(config.clone());
    entity(cx).update(cx, |_, cx| cx.emit(changed));
    config
}
