use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use argon_agents::defaults::AGENT_DEFAULTS;
use argon_agents::{
    AgentSpec, ModelInfo, ModelsCache, list_models, listing_spec, spec_fingerprint,
    spec_lists_models,
};
use argon_core::{AgentKind, Config, Paths};
use gpui_kit::Context;

use super::sessions::AgentSessions;
use crate::services::{AppServices, Services};

const NO_MODELS: &str = "the agent returned no models";
const NOT_LISTED: &str = "the agent does not list models";
const RETRY_AFTER_FAILURE: Duration = Duration::from_secs(60);

type Key = (AgentKind, String);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trigger {
    Automatic,
    Manual,
}

#[derive(Debug, PartialEq, Eq)]
enum Adoption {
    Unchanged,
    Cached(Vec<ModelInfo>),
    Reset,
}

pub struct ModelListings {
    cache: ModelsCache,
    path: PathBuf,
    in_flight: HashSet<Key>,
    failed: HashMap<Key, SystemTime>,
    current: HashMap<AgentKind, String>,
}

impl ModelListings {
    pub fn load(paths: &Paths) -> Self {
        Self::new(paths.data_dir.join("agents").join("models.json"))
    }

    fn new(path: PathBuf) -> Self {
        Self {
            cache: ModelsCache::load(&path),
            path,
            in_flight: HashSet::new(),
            failed: HashMap::new(),
            current: HashMap::new(),
        }
    }

    fn adopt(&mut self, kind: AgentKind, fingerprint: &str) -> Adoption {
        let previous = self.current.insert(kind, fingerprint.to_owned());
        match self.cache.get(kind, fingerprint) {
            Some((_, models)) => Adoption::Cached(models),
            None if previous.is_some_and(|previous| previous != fingerprint) => Adoption::Reset,
            None => Adoption::Unchanged,
        }
    }

    fn begin(
        &mut self,
        kind: AgentKind,
        fingerprint: &str,
        now: SystemTime,
        trigger: Trigger,
    ) -> bool {
        let key = (kind, fingerprint.to_owned());
        !self.cache.is_fresh(kind, fingerprint, now)
            && (trigger == Trigger::Manual || !self.backing_off(&key, now))
            && self.in_flight.insert(key)
    }

    fn backing_off(&self, key: &Key, now: SystemTime) -> bool {
        self.failed
            .get(key)
            .and_then(|at| now.duration_since(*at).ok())
            .is_some_and(|age| age < RETRY_AFTER_FAILURE)
    }

    fn finish(
        &mut self,
        kind: AgentKind,
        fingerprint: &str,
        outcome: Result<Vec<ModelInfo>, String>,
        now: SystemTime,
    ) -> Result<Option<Vec<ModelInfo>>, String> {
        let key = (kind, fingerprint.to_owned());
        self.in_flight.remove(&key);
        let outcome = outcome.and_then(|models| {
            if models.is_empty() {
                Err(NO_MODELS.to_owned())
            } else {
                Ok(models)
            }
        });
        let models = match outcome {
            Ok(models) => models,
            Err(error) => {
                self.failed.insert(key, now);
                return Err(error);
            }
        };
        self.failed.remove(&key);
        if self.current.get(&kind).map(String::as_str) != Some(fingerprint) {
            return Ok(None);
        }
        self.cache.put(kind, fingerprint, now, models.clone());
        if let Err(error) = self.cache.save(&self.path) {
            tracing::warn!(path = %self.path.display(), %error, "failed to save the models cache");
        }
        Ok(Some(models))
    }
}

impl AgentSessions {
    pub fn apply_listed_models(&self, spec: &mut AgentSpec) {
        if let Some(models) = self.listed_models(spec) {
            spec.models = models.to_vec();
        }
    }

    pub fn listed_models(&self, spec: &AgentSpec) -> Option<&[ModelInfo]> {
        spec_lists_models(spec)
            .then(|| self.detection().reported(spec.kind))
            .flatten()
    }

    pub fn refresh_models(
        &mut self,
        specs: Vec<AgentSpec>,
        trigger: Trigger,
        cx: &mut Context<Self>,
    ) {
        let services = cx
            .try_global::<AppServices>()
            .map(|global| global.0.clone());
        let now = SystemTime::now();
        let mut changed = false;
        for spec in specs.into_iter().filter(spec_lists_models) {
            let kind = spec.kind;
            let fingerprint = spec_fingerprint(&spec);
            match self.listings_mut().adopt(kind, &fingerprint) {
                Adoption::Cached(models) => {
                    self.detection_mut().report_models(kind, &models);
                    changed = true;
                }
                Adoption::Reset => {
                    self.detection_mut().reset_models(kind);
                    changed = true;
                }
                Adoption::Unchanged => {}
            }
            let started = self.listings_mut().begin(kind, &fingerprint, now, trigger);
            if let (true, Some(services)) = (started, &services) {
                Self::spawn_listing(spec, fingerprint, services, cx);
            }
        }
        if changed {
            cx.notify();
        }
    }

    pub fn prefetch_models(&mut self, config: &Config, cx: &mut Context<Self>) {
        let specs = AGENT_DEFAULTS
            .iter()
            .filter_map(|defaults| listing_spec(defaults.kind, config))
            .collect();
        self.refresh_models(specs, Trigger::Automatic, cx);
    }

    fn spawn_listing(
        spec: AgentSpec,
        fingerprint: String,
        services: &Services,
        cx: &mut Context<Self>,
    ) {
        let (kind, label) = (spec.kind, spec.label);
        let job = services.spawn_runtime(async move { list_models(&spec).await });
        cx.spawn(async move |this, cx| {
            let outcome = match job.await {
                Ok(Some(result)) => result,
                Ok(None) => Err(NOT_LISTED.to_owned()),
                Err(error) => Err(error.to_string()),
            };
            _ = this.update(cx, |this, cx| {
                this.finish_listing(kind, label, &fingerprint, outcome, cx)
            });
        })
        .detach();
    }

    fn finish_listing(
        &mut self,
        kind: AgentKind,
        label: &'static str,
        fingerprint: &str,
        outcome: Result<Vec<ModelInfo>, String>,
        cx: &mut Context<Self>,
    ) {
        match self
            .listings_mut()
            .finish(kind, fingerprint, outcome, SystemTime::now())
        {
            Ok(Some(models)) => {
                self.detection_mut().report_models(kind, &models);
                cx.notify();
            }
            Ok(None) => {
                tracing::debug!(
                    agent = label,
                    "discarded models listed for a superseded spec"
                );
            }
            Err(error) => tracing::warn!(agent = label, %error, "failed to list agent models"),
        }
    }
}

#[cfg(test)]
mod tests {
    use argon_agents::{Transport, agent_spec};

    use super::*;

    const FP_A: &str = "fp-a";
    const FP_B: &str = "fp-b";
    const OPENCODE: AgentKind = AgentKind::OpenCode;

    struct Fixture {
        listings: ModelListings,
        root: PathBuf,
    }

    impl Fixture {
        fn new(name: &str) -> Self {
            let root =
                std::env::temp_dir().join(format!("argon-listings-{name}-{}", std::process::id()));
            _ = std::fs::remove_dir_all(&root);
            let listings = ModelListings::new(root.join("agents").join("models.json"));
            Self { listings, root }
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            _ = std::fs::remove_dir_all(&self.root);
        }
    }

    fn model(id: &str) -> ModelInfo {
        ModelInfo {
            id: id.into(),
            label: id.into(),
        }
    }

    fn secs(seconds: u64) -> Duration {
        Duration::from_secs(seconds)
    }

    #[test]
    fn a_listing_is_never_started_twice_for_the_same_agent_and_spec() {
        let mut fixture = Fixture::new("guard");
        let listings = &mut fixture.listings;
        let now = SystemTime::now();
        assert!(listings.begin(OPENCODE, FP_A, now, Trigger::Automatic));
        assert!(!listings.begin(OPENCODE, FP_A, now, Trigger::Automatic));
        assert!(!listings.begin(OPENCODE, FP_A, now, Trigger::Manual));
        assert!(listings.begin(AgentKind::CommandCode, FP_A, now, Trigger::Automatic));
    }

    #[test]
    fn a_new_spec_is_not_blocked_by_a_running_listing_of_the_old_one() {
        let mut fixture = Fixture::new("keyed");
        let listings = &mut fixture.listings;
        let now = SystemTime::now();
        assert!(listings.begin(OPENCODE, FP_A, now, Trigger::Automatic));
        assert!(listings.begin(OPENCODE, FP_B, now, Trigger::Automatic));
        listings
            .finish(OPENCODE, FP_A, Err("boom".into()), now)
            .unwrap_err();
        assert!(!listings.begin(OPENCODE, FP_B, now, Trigger::Automatic));
    }

    #[test]
    fn codex_listings_start_like_the_other_listing_agents() {
        let mut fixture = Fixture::new("codex");
        let listings = &mut fixture.listings;
        let now = SystemTime::now();
        listings.adopt(AgentKind::Codex, FP_A);
        assert!(listings.begin(AgentKind::Codex, FP_A, now, Trigger::Automatic));
        let models = vec![model("gpt-5.5")];
        assert_eq!(
            listings.finish(AgentKind::Codex, FP_A, Ok(models.clone()), now),
            Ok(Some(models.clone()))
        );
        assert_eq!(
            listings.adopt(AgentKind::Codex, FP_A),
            Adoption::Cached(models)
        );
    }

    #[test]
    fn only_specs_that_list_models_are_refreshed_or_applied() {
        let config = Config::default();
        assert!(!spec_lists_models(&agent_spec(
            AgentKind::ClaudeCode,
            &config
        )));
        assert!(spec_lists_models(&agent_spec(OPENCODE, &config)));
        let mut codex = agent_spec(AgentKind::Codex, &config);
        codex.transport = Transport::Acp;
        assert!(!spec_lists_models(&codex));
        codex.transport = Transport::CodexAppServer;
        assert!(spec_lists_models(&codex));
    }

    #[test]
    fn fresh_results_are_cached_and_persisted_until_stale() {
        let mut fixture = Fixture::new("fresh");
        let listings = &mut fixture.listings;
        let now = SystemTime::now();
        listings.adopt(OPENCODE, FP_A);
        assert!(listings.begin(OPENCODE, FP_A, now, Trigger::Automatic));
        let models = vec![model("opencode-go/glm")];
        assert_eq!(
            listings.finish(OPENCODE, FP_A, Ok(models.clone()), now),
            Ok(Some(models.clone()))
        );
        let soon = now + secs(599);
        assert!(!listings.begin(OPENCODE, FP_A, soon, Trigger::Manual));
        let later = now + secs(601);
        assert!(listings.begin(OPENCODE, FP_A, later, Trigger::Automatic));
        let reloaded = ModelsCache::load(&listings.path);
        assert_eq!(
            reloaded.get(OPENCODE, FP_A).map(|(_, models)| models),
            Some(models)
        );
    }

    #[test]
    fn a_failed_listing_is_not_retried_automatically_for_a_minute() {
        let mut fixture = Fixture::new("backoff");
        let listings = &mut fixture.listings;
        let now = SystemTime::now();
        listings.adopt(OPENCODE, FP_A);
        assert!(listings.begin(OPENCODE, FP_A, now, Trigger::Automatic));
        listings
            .finish(OPENCODE, FP_A, Err("boom".into()), now)
            .unwrap_err();
        assert!(!listings.begin(OPENCODE, FP_A, now + secs(59), Trigger::Automatic));
        assert!(listings.begin(OPENCODE, FP_A, now + secs(59), Trigger::Manual));
        listings
            .finish(OPENCODE, FP_A, Err("boom".into()), now + secs(59))
            .unwrap_err();
        assert!(!listings.begin(OPENCODE, FP_A, now + secs(100), Trigger::Automatic));
        assert!(listings.begin(OPENCODE, FP_A, now + secs(120), Trigger::Automatic));
        assert!(listings.begin(OPENCODE, FP_B, now, Trigger::Automatic));
    }

    #[test]
    fn empty_listings_are_failures_and_not_cached() {
        let mut fixture = Fixture::new("empty");
        let listings = &mut fixture.listings;
        let now = SystemTime::now();
        listings.adopt(OPENCODE, FP_A);
        listings.begin(OPENCODE, FP_A, now, Trigger::Automatic);
        assert_eq!(
            listings.finish(OPENCODE, FP_A, Ok(Vec::new()), now),
            Err(NO_MODELS.to_owned())
        );
        assert!(listings.cache.get(OPENCODE, FP_A).is_none());
        assert!(!listings.path.exists());
        assert!(!listings.begin(OPENCODE, FP_A, now, Trigger::Automatic));
    }

    #[test]
    fn results_for_a_superseded_spec_are_neither_shown_nor_persisted() {
        let mut fixture = Fixture::new("superseded");
        let listings = &mut fixture.listings;
        let now = SystemTime::now();
        listings.adopt(OPENCODE, FP_A);
        listings.begin(OPENCODE, FP_A, now, Trigger::Automatic);
        listings.adopt(OPENCODE, FP_B);
        assert_eq!(
            listings.finish(OPENCODE, FP_A, Ok(vec![model("old/model")]), now),
            Ok(None)
        );
        assert!(listings.cache.get(OPENCODE, FP_A).is_none());
        assert!(!listings.path.exists());
    }

    #[test]
    fn adopting_a_spec_reuses_its_cached_models_and_resets_on_change() {
        let mut fixture = Fixture::new("adopt");
        let listings = &mut fixture.listings;
        let now = SystemTime::now();
        assert_eq!(listings.adopt(OPENCODE, FP_A), Adoption::Unchanged);
        listings.begin(OPENCODE, FP_A, now, Trigger::Automatic);
        listings
            .finish(OPENCODE, FP_A, Ok(vec![model("a/b")]), now)
            .unwrap();
        assert_eq!(
            listings.adopt(OPENCODE, FP_A),
            Adoption::Cached(vec![model("a/b")])
        );
        assert_eq!(listings.adopt(OPENCODE, FP_B), Adoption::Reset);
        assert_eq!(listings.adopt(OPENCODE, FP_B), Adoption::Unchanged);
        assert_eq!(
            listings.adopt(AgentKind::CommandCode, FP_A),
            Adoption::Unchanged
        );
    }

    #[test]
    fn the_persisted_cache_is_loaded_for_a_matching_spec_only() {
        let mut fixture = Fixture::new("reload");
        let now = SystemTime::now();
        {
            let listings = &mut fixture.listings;
            listings.adopt(OPENCODE, FP_A);
            listings.begin(OPENCODE, FP_A, now, Trigger::Automatic);
            listings
                .finish(OPENCODE, FP_A, Ok(vec![model("a/b")]), now)
                .unwrap();
        }
        let mut reloaded = ModelListings::new(fixture.listings.path.clone());
        assert_eq!(reloaded.adopt(OPENCODE, FP_B), Adoption::Unchanged);
        assert_eq!(
            reloaded.adopt(OPENCODE, FP_A),
            Adoption::Cached(vec![model("a/b")])
        );
    }
}
