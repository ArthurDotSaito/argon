use argon_agents::AgentSpec;
use gpui_kit::{App, Context, Window};

use super::{Load, StartDialog};
use crate::agents::Trigger;
use crate::agents::detection::AgentOption;

impl StartDialog {
    pub(super) fn with_listed_models(mut options: Vec<AgentOption>, cx: &App) -> Vec<AgentOption> {
        if let Some(sessions) = Self::sessions(cx) {
            let sessions = sessions.read(cx);
            for option in &mut options {
                sessions.apply_listed_models(&mut option.spec);
            }
        }
        options
    }

    pub(super) fn refresh_listed_models(&mut self, cx: &mut Context<Self>) {
        let Some(sessions) = Self::sessions(cx) else {
            return;
        };
        let specs: Vec<AgentSpec> = self
            .agents
            .ready()
            .into_iter()
            .flatten()
            .filter(|option| option.state.is_selectable())
            .map(|option| option.spec.clone())
            .collect();
        sessions.update(cx, |sessions, cx| {
            sessions.refresh_models(specs, Trigger::Automatic, cx)
        });
    }

    pub(super) fn apply_listed_models(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.is_launching() {
            return;
        }
        let Some(sessions) = Self::sessions(cx) else {
            return;
        };
        let selected = self.agent;
        let mut selected_changed = false;
        if let Load::Ready(options) = &mut self.agents {
            let sessions = sessions.read(cx);
            for option in options {
                let Some(listed) = sessions.listed_models(&option.spec) else {
                    continue;
                };
                if option.spec.models != listed {
                    option.spec.models = listed.to_vec();
                    selected_changed |= selected == Some(option.spec.kind);
                }
            }
        }
        if selected_changed {
            self.set_model_choices(self.model.clone(), window, cx);
            cx.notify();
        }
    }
}
