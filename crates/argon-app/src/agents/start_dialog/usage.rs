use std::time::SystemTime;

use argon_core::AgentKind;
use gpui_kit::{App, Context};

use super::StartDialog;
use crate::agents::detection::AgentOption;
use crate::agents::{Trigger, UsageLine, card_line, plan_usage_consent};

impl StartDialog {
    pub(super) fn refresh_usage(&mut self, cx: &mut Context<Self>) {
        let Some(sessions) = Self::sessions(cx) else {
            return;
        };
        let kinds: Vec<AgentKind> = self
            .agents
            .ready()
            .into_iter()
            .flatten()
            .filter(|option| option.state.is_selectable())
            .map(|option| option.spec.kind)
            .collect();
        let config = self.config.clone();
        sessions.update(cx, |sessions, cx| {
            sessions.refresh_usage(&kinds, &config, Trigger::Automatic, cx)
        });
    }

    pub(super) fn usage_line(&self, option: &AgentOption, cx: &App) -> Option<UsageLine> {
        if !option.state.is_selectable() {
            return None;
        }
        let sessions = Self::sessions(cx)?;
        let sessions = sessions.read(cx);
        let kind = option.spec.kind;
        let consent = plan_usage_consent(&self.config, kind);
        card_line(sessions.usage_state(kind, consent), SystemTime::now())
    }

    pub(super) fn sync_usage_revision(&mut self, cx: &mut Context<Self>) {
        let Some(sessions) = Self::sessions(cx) else {
            return;
        };
        let revision = sessions.read(cx).usage_revision();
        if revision != self.usage_revision {
            self.usage_revision = revision;
            cx.notify();
        }
    }
}
