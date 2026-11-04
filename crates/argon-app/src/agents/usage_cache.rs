use std::collections::HashMap;
use std::time::{Duration, SystemTime};

use argon_agents::AccountUsage;
use argon_core::{AgentKind, Config};

use super::model_listing::Trigger;

pub const USAGE_TTL: Duration = Duration::from_secs(300);

pub fn shows_usage(kind: AgentKind) -> bool {
    kind != AgentKind::Antigravity
}

pub fn needs_consent(kind: AgentKind) -> bool {
    kind == AgentKind::OpenCode
}

pub fn plan_usage_consent(config: &Config, kind: AgentKind) -> bool {
    needs_consent(kind)
        && config
            .agents
            .get(&kind)
            .is_some_and(|agent| agent.read_plan_usage)
}

fn fetches_usage(kind: AgentKind, consent: bool) -> bool {
    shows_usage(kind) && (consent || !needs_consent(kind))
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum UsageState<'a> {
    Hidden,
    Loading,
    NeedsConsent,
    Shown(&'a AccountUsage),
}

#[derive(Default)]
pub struct UsageCache {
    entries: HashMap<AgentKind, (AccountUsage, SystemTime)>,
    in_flight: HashMap<AgentKind, u64>,
    next_ticket: u64,
    revision: u64,
}

impl UsageCache {
    pub fn get(&self, kind: AgentKind) -> Option<&AccountUsage> {
        self.entries.get(&kind).map(|(usage, _)| usage)
    }

    pub fn state(&self, kind: AgentKind, consent: bool) -> UsageState<'_> {
        if !shows_usage(kind) {
            UsageState::Hidden
        } else if needs_consent(kind) && !consent {
            UsageState::NeedsConsent
        } else {
            self.get(kind)
                .map_or(UsageState::Loading, UsageState::Shown)
        }
    }

    pub fn is_refreshing(&self, kind: AgentKind) -> bool {
        self.in_flight.contains_key(&kind)
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    fn is_fresh(&self, kind: AgentKind, now: SystemTime) -> bool {
        self.entries.get(&kind).is_some_and(|(_, fetched_at)| {
            now.duration_since(*fetched_at)
                .is_ok_and(|age| age < USAGE_TTL)
        })
    }

    pub fn begin(
        &mut self,
        kind: AgentKind,
        consent: bool,
        now: SystemTime,
        trigger: Trigger,
    ) -> Option<u64> {
        let wanted = fetches_usage(kind, consent)
            && (trigger == Trigger::Manual || !self.is_fresh(kind, now))
            && !self.in_flight.contains_key(&kind);
        if !wanted {
            return None;
        }
        self.next_ticket += 1;
        self.in_flight.insert(kind, self.next_ticket);
        self.revision += 1;
        Some(self.next_ticket)
    }

    pub fn finish(
        &mut self,
        kind: AgentKind,
        ticket: u64,
        usage: AccountUsage,
        now: SystemTime,
    ) -> bool {
        if self.in_flight.get(&kind) != Some(&ticket) {
            return false;
        }
        self.in_flight.remove(&kind);
        self.entries.insert(kind, (usage, now));
        self.revision += 1;
        true
    }

    pub fn evict(&mut self, kind: AgentKind) {
        self.entries.remove(&kind);
        self.in_flight.remove(&kind);
        self.revision += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLAUDE: AgentKind = AgentKind::ClaudeCode;
    const OPENCODE: AgentKind = AgentKind::OpenCode;

    fn secs(seconds: u64) -> Duration {
        Duration::from_secs(seconds)
    }

    fn usage(plan: &str) -> AccountUsage {
        AccountUsage::Ok {
            plan: plan.into(),
            windows: Vec::new(),
        }
    }

    #[test]
    fn a_fetch_is_never_started_twice_for_the_same_agent() {
        let mut cache = UsageCache::default();
        let now = SystemTime::now();
        assert!(
            cache
                .begin(CLAUDE, false, now, Trigger::Automatic)
                .is_some()
        );
        assert!(
            cache
                .begin(CLAUDE, false, now, Trigger::Automatic)
                .is_none()
        );
        assert!(cache.begin(CLAUDE, false, now, Trigger::Manual).is_none());
        assert!(
            cache
                .begin(AgentKind::Codex, false, now, Trigger::Automatic)
                .is_some()
        );
    }

    #[test]
    fn fresh_entries_are_reused_until_the_ttl_and_manual_refresh_bypasses_it() {
        let mut cache = UsageCache::default();
        let now = SystemTime::now();
        let ticket = cache.begin(CLAUDE, false, now, Trigger::Automatic).unwrap();
        assert!(cache.finish(CLAUDE, ticket, usage("Team"), now));
        assert!(
            cache
                .begin(CLAUDE, false, now + secs(299), Trigger::Automatic)
                .is_none()
        );
        assert!(
            cache
                .begin(CLAUDE, false, now + secs(299), Trigger::Manual)
                .is_some()
        );
        let mut stale = UsageCache::default();
        let ticket = stale.begin(CLAUDE, false, now, Trigger::Automatic).unwrap();
        stale.finish(CLAUDE, ticket, usage("Team"), now);
        assert!(
            stale
                .begin(CLAUDE, false, now + secs(301), Trigger::Automatic)
                .is_some()
        );
        assert_eq!(stale.get(CLAUDE), Some(&usage("Team")));
    }

    #[test]
    fn failures_are_cached_like_any_other_result() {
        let mut cache = UsageCache::default();
        let now = SystemTime::now();
        let ticket = cache.begin(CLAUDE, false, now, Trigger::Automatic).unwrap();
        let failure = AccountUsage::Unavailable("sem resposta".into());
        cache.finish(CLAUDE, ticket, failure.clone(), now);
        assert_eq!(cache.get(CLAUDE), Some(&failure));
        assert!(
            cache
                .begin(CLAUDE, false, now + secs(10), Trigger::Automatic)
                .is_none()
        );
    }

    #[test]
    fn opencode_is_fetched_only_with_consent_and_antigravity_never() {
        let mut cache = UsageCache::default();
        let now = SystemTime::now();
        assert!(cache.begin(OPENCODE, false, now, Trigger::Manual).is_none());
        assert!(
            cache
                .begin(OPENCODE, true, now, Trigger::Automatic)
                .is_some()
        );
        assert!(
            cache
                .begin(AgentKind::Antigravity, true, now, Trigger::Manual)
                .is_none()
        );
    }

    #[test]
    fn evicting_drops_the_entry_and_discards_a_fetch_still_running() {
        let mut cache = UsageCache::default();
        let now = SystemTime::now();
        let ticket = cache
            .begin(OPENCODE, true, now, Trigger::Automatic)
            .unwrap();
        cache.finish(OPENCODE, ticket, usage("OpenCode Go"), now);
        let running = cache.begin(OPENCODE, true, now, Trigger::Manual).unwrap();
        cache.evict(OPENCODE);
        assert!(cache.get(OPENCODE).is_none());
        assert!(!cache.is_refreshing(OPENCODE));
        assert!(!cache.finish(OPENCODE, running, usage("OpenCode Go"), now));
        assert!(cache.get(OPENCODE).is_none());
    }

    #[test]
    fn a_late_result_never_replaces_the_one_of_a_newer_fetch() {
        let mut cache = UsageCache::default();
        let now = SystemTime::now();
        let old = cache
            .begin(OPENCODE, true, now, Trigger::Automatic)
            .unwrap();
        cache.evict(OPENCODE);
        let new = cache
            .begin(OPENCODE, true, now, Trigger::Automatic)
            .unwrap();
        assert!(!cache.finish(OPENCODE, old, usage("stale"), now));
        assert!(cache.is_refreshing(OPENCODE));
        assert!(cache.finish(OPENCODE, new, usage("fresh"), now));
        assert_eq!(cache.get(OPENCODE), Some(&usage("fresh")));
    }

    #[test]
    fn the_revision_moves_whenever_something_visible_changes() {
        let mut cache = UsageCache::default();
        let now = SystemTime::now();
        let start = cache.revision();
        let ticket = cache.begin(CLAUDE, false, now, Trigger::Automatic).unwrap();
        let began = cache.revision();
        cache.finish(CLAUDE, ticket, usage("Team"), now);
        let finished = cache.revision();
        cache.evict(CLAUDE);
        assert!(start < began && began < finished && finished < cache.revision());
    }

    #[test]
    fn the_state_reflects_consent_cache_and_agent_support() {
        let mut cache = UsageCache::default();
        let now = SystemTime::now();
        assert_eq!(cache.state(CLAUDE, false), UsageState::Loading);
        assert_eq!(cache.state(OPENCODE, false), UsageState::NeedsConsent);
        assert_eq!(cache.state(OPENCODE, true), UsageState::Loading);
        assert_eq!(
            cache.state(AgentKind::Antigravity, true),
            UsageState::Hidden
        );
        let ticket = cache
            .begin(OPENCODE, true, now, Trigger::Automatic)
            .unwrap();
        cache.finish(OPENCODE, ticket, usage("OpenCode Go"), now);
        assert_eq!(
            cache.state(OPENCODE, true),
            UsageState::Shown(&usage("OpenCode Go"))
        );
        assert_eq!(cache.state(OPENCODE, false), UsageState::NeedsConsent);
    }

    #[test]
    fn consent_is_read_from_the_opencode_entry_only() {
        let mut config = Config::default();
        assert!(!plan_usage_consent(&config, OPENCODE));
        config.agents.entry(OPENCODE).or_default().read_plan_usage = true;
        assert!(plan_usage_consent(&config, OPENCODE));
        config.agents.entry(CLAUDE).or_default().read_plan_usage = true;
        assert!(!plan_usage_consent(&config, CLAUDE));
    }
}
