use argon_core::Density;
use gpui_kit::{App, Context, Global, Subscription};

const COMFORTABLE_SCALE: f32 = 1.25;

struct DensityState(Density);

impl Global for DensityState {}

pub fn current(cx: &App) -> Density {
    cx.try_global::<DensityState>()
        .map_or(Density::Compact, |state| state.0)
}

pub fn scaled(base: f32, cx: &App) -> f32 {
    match current(cx) {
        Density::Compact => base,
        Density::Comfortable => (base * COMFORTABLE_SCALE).round(),
    }
}

pub fn set(density: Density, cx: &mut App) {
    if cx
        .try_global::<DensityState>()
        .is_some_and(|state| state.0 == density)
    {
        return;
    }
    cx.set_global(DensityState(density));
    cx.refresh_windows();
}

pub fn observe<T: 'static>(
    cx: &mut Context<T>,
    mut on_change: impl FnMut(&mut T, &mut Context<T>) + 'static,
) -> Subscription {
    cx.observe_global::<DensityState>(move |this, cx| on_change(this, cx))
}
