use std::time::{Duration, Instant};

use argon_agents::{AgentSpec, Detection, ModelInfo, Support};
use argon_core::{AgentKind, PromptTemplate};

use super::labels;

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
    let mut models = spec.models.clone();
    if !spec.default_model.is_empty() && !models.iter().any(|model| model.id == spec.default_model)
    {
        models.insert(
            0,
            ModelInfo {
                id: spec.default_model.clone(),
                label: spec.default_model.clone(),
            },
        );
    }
    if models.is_empty() {
        models.push(ModelInfo {
            id: String::new(),
            label: labels::AGENT_DEFAULT_MODEL.to_owned(),
        });
    }
    models
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
}

impl DetectionCache {
    pub fn fresh(&self, now: Instant) -> Option<&[AgentOption]> {
        self.entry
            .as_ref()
            .filter(|(at, _)| now.saturating_duration_since(*at) < DETECTION_TTL)
            .map(|(_, options)| options.as_slice())
    }

    pub fn store(&mut self, at: Instant, options: Vec<AgentOption>) {
        self.entry = Some((at, options));
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
    fn model_choices_keep_configured_default() {
        let mut spec = agent_spec(AgentKind::ClaudeCode, &Config::default());
        assert_eq!(model_choices(&spec), spec.models);
        spec.default_model = "opus[1m]".into();
        let models = model_choices(&spec);
        assert_eq!(models[0].id, "opus[1m]");
        assert_eq!(models.len(), spec.models.len() + 1);
    }

    #[test]
    fn agents_without_known_models_offer_the_agent_default() {
        let mut spec = agent_spec(AgentKind::OpenCode, &Config::default());
        spec.models.clear();
        spec.default_model.clear();
        let models = model_choices(&spec);
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].id, "");
        assert_eq!(models[0].label, labels::AGENT_DEFAULT_MODEL);
    }

    #[test]
    fn default_template_prefers_configured() {
        let template = |id: &str| PromptTemplate {
            id: id.into(),
            name: id.into(),
            description: None,
            body: String::new(),
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
}
