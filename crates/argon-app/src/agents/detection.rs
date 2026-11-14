use std::collections::HashMap;
use std::time::{Duration, Instant};

use argon_agents::{AgentSpec, Detection, ModelInfo, Support, agent_spec};
use argon_core::{AgentKind, Config, PromptTemplate};

pub const DETECTION_TTL: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CardState {
    Ready { version: String },
    NeedsLogin { hint: String },
    Missing,
    Unsupported { reason: String },
    Failed { message: String },
}

impl CardState {
    pub fn is_selectable(&self) -> bool {
        matches!(self, Self::Ready { .. })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentOption {
    pub spec: AgentSpec,
    pub state: CardState,
}

impl AgentOption {
    pub fn new(spec: AgentSpec, support: &Support, detection: &Detection) -> Self {
        Self {
            state: card_state(support, detection),
            spec,
        }
    }
}

pub fn card_state(support: &Support, detection: &Detection) -> CardState {
    if let Support::NotYet { reason } = support {
        return CardState::Unsupported {
            reason: reason.clone(),
        };
    }
    match detection {
        Detection::Found { version } => CardState::Ready {
            version: version.clone(),
        },
        Detection::NeedsLogin { hint } => CardState::NeedsLogin { hint: hint.clone() },
        Detection::NotFound => CardState::Missing,
        Detection::Error(message) => CardState::Failed {
            message: message.clone(),
        },
    }
}

pub fn default_agent(options: &[AgentOption], last: Option<AgentKind>) -> Option<AgentKind> {
    let selectable = |kind: AgentKind| {
        options
            .iter()
            .any(|option| option.spec.kind == kind && option.state.is_selectable())
    };
    last.filter(|kind| selectable(*kind)).or_else(|| {
        options
            .iter()
            .find(|option| option.state.is_selectable())
            .map(|option| option.spec.kind)
    })
}

pub fn model_choices(spec: &AgentSpec) -> Vec<ModelInfo> {
    spec.models.clone()
}

pub fn default_model(spec: &AgentSpec) -> Option<String> {
    spec.models
        .iter()
        .find(|model| model.id == spec.default_model)
        .or_else(|| spec.models.first())
        .map(|model| model.id.clone())
}

pub fn default_template(templates: &[PromptTemplate], configured: &str) -> Option<String> {
    templates
        .iter()
        .find(|template| template.id == configured)
        .or_else(|| templates.first())
        .map(|template| template.id.clone())
}

#[derive(Default)]
pub struct DetectionCache {
    entry: Option<(Instant, Vec<AgentOption>)>,
    reported: HashMap<AgentKind, Vec<ModelInfo>>,
}

impl DetectionCache {
    pub fn fresh(&self, now: Instant) -> Option<&[AgentOption]> {
        self.entry
            .as_ref()
            .filter(|(at, _)| now.saturating_duration_since(*at) < DETECTION_TTL)
            .map(|(_, options)| options.as_slice())
    }

    pub fn store(&mut self, at: Instant, mut options: Vec<AgentOption>) {
        for option in &mut options {
            if let Some(models) = self.reported.get(&option.spec.kind) {
                option.spec.models = models.clone();
            }
        }
        self.entry = Some((at, options));
    }

    pub fn reported(&self, kind: AgentKind) -> Option<&[ModelInfo]> {
        self.reported.get(&kind).map(Vec::as_slice)
    }

    pub fn reset_models(&mut self, kind: AgentKind) {
        let defaults = agent_spec(kind, &Config::default()).models;
        if let Some((_, options)) = self.entry.as_mut() {
            for option in options.iter_mut().filter(|option| option.spec.kind == kind) {
                option.spec.models.clone_from(&defaults);
            }
        }
        self.reported.insert(kind, defaults);
    }

    pub fn report_models(&mut self, kind: AgentKind, models: &[ModelInfo]) {
        if models.is_empty() {
            return;
        }
        if let Some((_, options)) = self.entry.as_mut() {
            for option in options.iter_mut().filter(|option| option.spec.kind == kind) {
                option.spec.models = models.to_vec();
            }
        }
        self.reported.insert(kind, models.to_vec());
    }
}

#[cfg(test)]
mod tests {
    use argon_agents::agent_spec;
    use argon_core::Config;

    use super::*;

    fn option(kind: AgentKind, state: CardState) -> AgentOption {
        AgentOption {
            spec: agent_spec(kind, &Config::default()),
            state,
        }
    }

    fn ready() -> CardState {
        CardState::Ready {
            version: "1.0.0".into(),
        }
    }

    #[test]
    fn detection_maps_to_card_state() {
        let supported = Support::Supported;
        assert_eq!(
            card_state(
                &supported,
                &Detection::Found {
                    version: "2.4.1".into()
                }
            ),
            CardState::Ready {
                version: "2.4.1".into()
            }
        );
        assert_eq!(
            card_state(
                &supported,
                &Detection::NeedsLogin {
                    hint: "rode `codex login`".into()
                }
            ),
            CardState::NeedsLogin {
                hint: "rode `codex login`".into()
            }
        );
        assert_eq!(
            card_state(&supported, &Detection::NotFound),
            CardState::Missing
        );
        assert_eq!(
            card_state(&supported, &Detection::Error("boom".into())),
            CardState::Failed {
                message: "boom".into()
            }
        );
    }

    #[test]
    fn unsupported_wins_over_detection() {
        let support = Support::NotYet {
            reason: "headless".into(),
        };
        let found = Detection::Found {
            version: "1.0.3".into(),
        };
        assert_eq!(
            card_state(&support, &found),
            CardState::Unsupported {
                reason: "headless".into()
            }
        );
        assert!(!card_state(&support, &found).is_selectable());
    }

    #[test]
    fn default_agent_prefers_last_used_when_ready() {
        let options = [
            option(AgentKind::ClaudeCode, ready()),
            option(AgentKind::OpenCode, ready()),
        ];
        assert_eq!(
            default_agent(&options, Some(AgentKind::OpenCode)),
            Some(AgentKind::OpenCode)
        );
    }

    #[test]
    fn default_agent_falls_back_to_first_ready() {
        let options = [
            option(
                AgentKind::Codex,
                CardState::NeedsLogin {
                    hint: "login".into(),
                },
            ),
            option(AgentKind::ClaudeCode, ready()),
            option(AgentKind::OpenCode, ready()),
        ];
        assert_eq!(
            default_agent(&options, Some(AgentKind::Codex)),
            Some(AgentKind::ClaudeCode)
        );
        assert_eq!(default_agent(&options, None), Some(AgentKind::ClaudeCode));
        assert_eq!(
            default_agent(&[option(AgentKind::Codex, CardState::Missing)], None),
            None
        );
    }

    #[test]
    fn default_model_prefers_configured_when_listed() {
        let mut spec = agent_spec(AgentKind::ClaudeCode, &Config::default());
        spec.default_model = "sonnet".into();
        assert_eq!(default_model(&spec).as_deref(), Some("sonnet"));
        spec.default_model = "opus[1m]".into();
        assert_eq!(
            default_model(&spec),
            spec.models.first().map(|m| m.id.clone())
        );
        spec.models.clear();
        assert_eq!(default_model(&spec), None);
        assert!(model_choices(&spec).is_empty());
    }

    #[test]
    fn default_template_prefers_configured() {
        let template = |id: &str| PromptTemplate {
            id: id.into(),
            name: id.into(),
            ..PromptTemplate::default()
        };
        let templates = [template("review-padrao"), template("seguranca")];
        assert_eq!(
            default_template(&templates, "seguranca").as_deref(),
            Some("seguranca")
        );
        assert_eq!(
            default_template(&templates, "missing").as_deref(),
            Some("review-padrao")
        );
        assert_eq!(default_template(&[], "seguranca"), None);
    }

    #[test]
    fn reported_models_survive_new_detections() {
        let mut cache = DetectionCache::default();
        let at = Instant::now();
        let live = [ModelInfo {
            id: "claude-opus-5-5".into(),
            label: "Opus 5.5".into(),
        }];
        cache.store(at, vec![option(AgentKind::ClaudeCode, ready())]);
        cache.report_models(AgentKind::ClaudeCode, &live);
        cache.report_models(AgentKind::ClaudeCode, &[]);
        let models = |cache: &DetectionCache| cache.fresh(at).unwrap()[0].spec.models.clone();
        assert_eq!(models(&cache), live);
        cache.store(at, vec![option(AgentKind::ClaudeCode, ready())]);
        assert_eq!(models(&cache), live);
    }

    #[test]
    fn cache_expires_after_ttl() {
        let mut cache = DetectionCache::default();
        let at = Instant::now();
        assert!(cache.fresh(at).is_none());
        cache.store(at, vec![option(AgentKind::ClaudeCode, ready())]);
        assert_eq!(
            cache.fresh(at + Duration::from_secs(59)).map(<[_]>::len),
            Some(1)
        );
        assert!(cache.fresh(at + DETECTION_TTL).is_none());
    }

    #[test]
    fn reset_models_drops_listed_models_of_a_previous_spec() {
        let mut cache = DetectionCache::default();
        let at = Instant::now();
        let listed = [ModelInfo {
            id: "opencode-go/glm".into(),
            label: "GLM".into(),
        }];
        cache.store(at, vec![option(AgentKind::OpenCode, ready())]);
        cache.report_models(AgentKind::OpenCode, &listed);
        assert_eq!(cache.fresh(at).unwrap()[0].spec.models, listed);
        cache.reset_models(AgentKind::OpenCode);
        assert!(cache.fresh(at).unwrap()[0].spec.models.is_empty());
        assert_eq!(cache.reported(AgentKind::OpenCode), Some(&[][..]));
    }
}
