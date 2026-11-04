mod fields;
mod form;
mod form_view;

use std::time::Instant;

use argon_agents::defaults::AGENT_DEFAULTS;
use argon_agents::{AgentSpec, Detection, Support, detect_all};
use argon_core::Config;
use gpui_kit::component::v_flex;
use gpui_kit::{
    App, AppContext as _, Context, Entity, IntoElement, ParentElement as _, Render, Styled as _,
    Subscription, Task, Window,
};

use self::form::{AgentForm, Source};
use super::labels;
use super::ui::{section, services};
use crate::agents::{AgentOption, AgentSessions, Trigger};
use crate::config_watch;

pub(super) type DetectionResult = (AgentSpec, Support, Detection);

pub(super) fn run_detection(
    config: Config,
    cx: &App,
) -> Option<Task<Result<Vec<DetectionResult>, String>>> {
    let job = services(cx)?.spawn_runtime(async move { detect_all(&config).await });
    Some(cx.background_spawn(async move { job.await.map_err(|error| error.to_string()) }))
}

pub(super) fn store_cache(results: &[DetectionResult], at: Instant, cx: &mut App) {
    let Some(sessions) = AgentSessions::global(cx) else {
        return;
    };
    let options: Vec<AgentOption> = results
        .iter()
        .map(|(spec, support, detection)| AgentOption::new(spec.clone(), support, detection))
        .collect();
    sessions.update(cx, |sessions, _| {
        sessions.detection_mut().store(at, options)
    });
}

pub struct AgentsSection {
    forms: Vec<Entity<AgentForm>>,
    _task: Option<Task<()>>,
    _sessions: Option<Subscription>,
}

impl AgentsSection {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let config = config_watch::current(cx);
        let forms: Vec<Entity<AgentForm>> = AGENT_DEFAULTS
            .iter()
            .map(|defaults| cx.new(|cx| AgentForm::new(defaults.kind, &config, window, cx)))
            .collect();
        let sessions = AgentSessions::global(cx).map(|sessions| {
            cx.observe_in(&sessions, window, |this, _, window, cx| {
                this.apply_listed_models(window, cx)
            })
        });
        let mut this = Self {
            forms,
            _task: None,
            _sessions: sessions,
        };
        this.detect(config, window, cx);
        this
    }

    fn apply_listed_models(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for form in &self.forms {
            form.update(cx, |form, cx| form.apply_listed_models(window, cx));
        }
    }

    fn detect(&mut self, config: Config, window: &mut Window, cx: &mut Context<Self>) {
        let Some(task) = run_detection(config, cx) else {
            return;
        };
        self._task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            _ = this.update_in(cx, |this, window, cx| {
                let results = match result {
                    Ok(results) => results,
                    Err(error) => {
                        tracing::warn!(%error, "agent detection failed in settings");
                        return;
                    }
                };
                store_cache(&results, Instant::now(), cx);
                for form in &this.forms {
                    form.update(cx, |form, cx| {
                        form.apply(&results, Source::Saved(Trigger::Automatic), window, cx)
                    });
                }
            });
        }));
    }
}

impl Render for AgentsSection {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        section(labels::AGENTS, labels::AGENTS_HINT, cx)
            .child(v_flex().gap_3().children(self.forms.iter().cloned()))
    }
}
