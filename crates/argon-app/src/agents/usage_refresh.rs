use std::time::SystemTime;

use argon_agents::{AccountUsage, account_usage, agent_spec};
use argon_core::{AgentKind, Config};
use gpui_kit::Context;

use super::model_listing::Trigger;
use super::sessions::AgentSessions;
use super::usage_cache::{UsageState, plan_usage_consent};
use crate::services::AppServices;

impl AgentSessions {
    pub fn usage_state(&self, kind: AgentKind, consent: bool) -> UsageState<'_> {
        self.usage().state(kind, consent)
    }

    pub fn usage_refreshing(&self, kind: AgentKind) -> bool {
        self.usage().is_refreshing(kind)
    }

    pub fn usage_revision(&self) -> u64 {
        self.usage().revision()
    }

    pub fn refresh_usage(
        &mut self,
        kinds: &[AgentKind],
        config: &Config,
        trigger: Trigger,
        cx: &mut Context<Self>,
    ) {
        let Some(services) = cx
            .try_global::<AppServices>()
            .map(|global| global.0.clone())
        else {
            return;
        };
        let now = SystemTime::now();
        let mut started = false;
        for &kind in kinds {
            let consent = plan_usage_consent(config, kind);
            let Some(ticket) = self.usage_mut().begin(kind, consent, now, trigger) else {
                continue;
            };
            started = true;
            let spec = agent_spec(kind, config);
            let job =
                services.spawn_runtime(async move { account_usage(kind, &spec, consent).await });
            cx.spawn(async move |this, cx| {
                let usage = job
                    .await
                    .unwrap_or_else(|error| AccountUsage::Unavailable(error.to_string()));
                _ = this.update(cx, |this, cx| {
                    if this
                        .usage_mut()
                        .finish(kind, ticket, usage, SystemTime::now())
                    {
                        cx.notify();
                    }
                });
            })
            .detach();
        }
        if started {
            cx.notify();
        }
    }

    pub fn forget_usage(&mut self, kind: AgentKind, cx: &mut Context<Self>) {
        self.usage_mut().evict(kind);
        cx.notify();
    }
}
