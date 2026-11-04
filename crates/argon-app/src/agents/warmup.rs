use std::time::Duration;

use argon_agents::warm_up_agents;
use gpui_kit::App;

use super::AgentSessions;
use crate::services::AppServices;

const STARTUP_DELAY: Duration = Duration::from_secs(5);

pub fn on_app_start(cx: &mut App) {
    spawn(Some(STARTUP_DELAY), cx);
    prefetch_models(cx);
}

fn prefetch_models(cx: &mut App) {
    let (Some(sessions), Some(services)) = (
        AgentSessions::global(cx),
        cx.try_global::<AppServices>()
            .map(|global| global.0.clone()),
    ) else {
        return;
    };
    let config = services.config();
    sessions.update(cx, |sessions, cx| sessions.prefetch_models(&config, cx));
}

pub fn now(cx: &App) {
    spawn(None, cx);
}

fn spawn(delay: Option<Duration>, cx: &App) {
    let Some(services) = cx
        .try_global::<AppServices>()
        .map(|global| global.0.clone())
    else {
        return;
    };
    let config = services.config();
    drop(services.spawn_runtime(async move {
        if let Some(delay) = delay {
            tokio::time::sleep(delay).await;
        }
        warm_up_agents(config).await;
    }));
}
