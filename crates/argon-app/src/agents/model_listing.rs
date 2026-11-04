use std::collections::HashSet;
use std::path::PathBuf;
use std::time::SystemTime;

use argon_agents::defaults::AGENT_DEFAULTS;
use argon_agents::{AgentSpec, ModelInfo, ModelsCache, list_models, listing_spec, lists_models};
use argon_core::{AgentKind, Config, Paths};
use gpui_kit::Context;

use super::detection::DetectionCache;
use super::sessions::AgentSessions;
use crate::services::{AppServices, Services};

const NO_MODELS: &str = "the agent returned no models";
const NOT_LISTED: &str = "the agent does not list models";

pub struct ModelListings {
    cache: ModelsCache,
    path: PathBuf,
    in_flight: HashSet<AgentKind>,
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
        }
    }

    pub fn seed(&self, detection: &mut DetectionCache) {
        for defaults in AGENT_DEFAULTS
            .iter()
            .filter(|defaults| lists_models(defaults.kind))
        {
            if let Some((_, models)) = self.cache.get(defaults.kind) {
                detection.report_models(defaults.kind, &models);
            }
        }
    }

    fn begin(&mut self, kind: AgentKind, now: SystemTime) -> bool {
        lists_models(kind) && !self.cache.is_fresh(kind, now) && self.in_flight.insert(kind)
    }

    fn finish(
        &mut self,
        kind: AgentKind,
        outcome: Result<Vec<ModelInfo>, String>,
        now: SystemTime,
    ) -> Result<Vec<ModelInfo>, String> {
        self.in_flight.remove(&kind);
        let models = outcome?;
        if models.is_empty() {
            return Err(NO_MODELS.to_owned());
        }
        self.cache.put(kind, now, models.clone());
        if let Err(error) = self.cache.save(&self.path) {
            tracing::warn!(path = %self.path.display(), %error, "failed to save the models cache");
        }
        Ok(models)
    }
}

impl AgentSessions {
    pub fn apply_listed_models(&self, spec: &mut AgentSpec) {
        if let Some(models) = self.listed_models(spec.kind) {
            spec.models = models.to_vec();
        }
    }

    pub fn listed_models(&self, kind: AgentKind) -> Option<&[ModelInfo]> {
        lists_models(kind)
            .then(|| self.detection().reported(kind))
            .flatten()
    }

    pub fn refresh_models(&mut self, specs: Vec<AgentSpec>, cx: &mut Context<Self>) {
        let Some(services) = cx
            .try_global::<AppServices>()
            .map(|global| global.0.clone())
        else {
            return;
        };
        let now = SystemTime::now();
        for spec in specs {
            if self.listings_mut().begin(spec.kind, now) {
                Self::spawn_listing(spec, &services, cx);
            }
        }
    }

    pub fn prefetch_models(&mut self, config: &Config, cx: &mut Context<Self>) {
        let specs = AGENT_DEFAULTS
            .iter()
            .filter_map(|defaults| listing_spec(defaults.kind, config))
            .collect();
        self.refresh_models(specs, cx);
    }

    fn spawn_listing(spec: AgentSpec, services: &Services, cx: &mut Context<Self>) {
        let (kind, label) = (spec.kind, spec.label);
        let job = services.spawn_runtime(async move { list_models(&spec).await });
        cx.spawn(async move |this, cx| {
            let outcome = match job.await {
                Ok(Some(result)) => result,
                Ok(None) => Err(NOT_LISTED.to_owned()),
                Err(error) => Err(error.to_string()),
            };
            _ = this.update(cx, |this, cx| this.finish_listing(kind, label, outcome, cx));
        })
        .detach();
    }

    fn finish_listing(
        &mut self,
        kind: AgentKind,
        label: &'static str,
        outcome: Result<Vec<ModelInfo>, String>,
        cx: &mut Context<Self>,
    ) {
        match self.listings_mut().finish(kind, outcome, SystemTime::now()) {
            Ok(models) => {
                self.detection_mut().report_models(kind, &models);
                cx.notify();
            }
            Err(error) => tracing::warn!(agent = label, %error, "failed to list agent models"),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

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

    #[test]
    fn a_listing_is_never_started_twice_for_the_same_agent() {
        let mut fixture = Fixture::new("guard");
        let listings = &mut fixture.listings;
        let now = SystemTime::now();
        assert!(listings.begin(AgentKind::OpenCode, now));
        assert!(!listings.begin(AgentKind::OpenCode, now));
        assert!(listings.begin(AgentKind::CommandCode, now));
        listings
            .finish(AgentKind::OpenCode, Err("boom".into()), now)
            .unwrap_err();
        assert!(listings.begin(AgentKind::OpenCode, now));
    }

    #[test]
    fn agents_without_listing_are_skipped() {
        let mut fixture = Fixture::new("skipped");
        assert!(
            !fixture
                .listings
                .begin(AgentKind::ClaudeCode, SystemTime::now())
        );
    }

    #[test]
    fn fresh_results_are_cached_and_persisted_until_stale() {
        let mut fixture = Fixture::new("fresh");
        let listings = &mut fixture.listings;
        let now = SystemTime::now();
        assert!(listings.begin(AgentKind::OpenCode, now));
        let models = vec![model("opencode-go/glm")];
        assert_eq!(
            listings.finish(AgentKind::OpenCode, Ok(models.clone()), now),
            Ok(models.clone())
        );
        assert!(!listings.begin(AgentKind::OpenCode, now + Duration::from_secs(599)));
        assert!(listings.begin(AgentKind::OpenCode, now + Duration::from_secs(601)));
        let reloaded = ModelsCache::load(&listings.path);
        assert_eq!(
            reloaded.get(AgentKind::OpenCode).map(|(_, models)| models),
            Some(models)
        );
    }

    #[test]
    fn empty_listings_are_rejected_and_not_cached() {
        let mut fixture = Fixture::new("empty");
        let listings = &mut fixture.listings;
        let now = SystemTime::now();
        listings.begin(AgentKind::OpenCode, now);
        assert_eq!(
            listings.finish(AgentKind::OpenCode, Ok(Vec::new()), now),
            Err(NO_MODELS.to_owned())
        );
        assert!(listings.cache.get(AgentKind::OpenCode).is_none());
        assert!(!listings.path.exists());
    }

    #[test]
    fn cached_models_seed_the_detection_cache() {
        let mut fixture = Fixture::new("seed");
        let listings = &mut fixture.listings;
        let now = SystemTime::now();
        listings.begin(AgentKind::CommandCode, now);
        listings
            .finish(AgentKind::CommandCode, Ok(vec![model("deepseek/v4")]), now)
            .unwrap();
        let mut detection = DetectionCache::default();
        listings.seed(&mut detection);
        assert_eq!(
            detection.reported(AgentKind::CommandCode),
            Some(&[model("deepseek/v4")][..])
        );
        assert_eq!(detection.reported(AgentKind::OpenCode), None);
    }
}
